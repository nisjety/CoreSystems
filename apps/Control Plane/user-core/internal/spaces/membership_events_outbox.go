package spaces

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"log"
	"sort"
	"strings"
	"time"
)

// WS04-S10: durable membership events.
//
// The membership ledger (space_membership_events, migration 034) is the
// outbox. Handlers still publish inline after commit (fast path, fire and
// forget); this worker is the durable path:
//
//  1. claim: a pending user row (published_at IS NULL, subject_type 'user',
//     an action that flips a user row active or inactive) is claimed with
//     FOR UPDATE SKIP LOCKED in one statement that also leases it
//     (next_publish_at = now + lease, publish_attempts + 1), so two replicas
//     never publish the same row and a crashed worker's row comes back after
//     the lease;
//  2. publish with Nats-Msg-Id = event_id and wait for the JetStream PubAck;
//  3. complete (published_at) or back off (next_publish_at, last_error).
//
// The payload states the CURRENT fact (active and role are read from
// space_memberships when the row is claimed), so a retried older row can
// never override a newer one. A re-announce sweep, every ReannounceEvery,
// republishes the current fact of every user membership changed in the last
// ReannounceWindow, which heals a consumer that missed an event or was
// provisioned after it. Ids, roles and counts only: no actor id, no names,
// no topic, no content, so Zero Data Retention organisations behave
// identically and no model is called.

// MembershipChangedSubject is the cross-plane subject Session Core subscribes
// to (control-shared-nats.conf grants user-core-shared publish on it, and the
// CONTROLPLANE stream captures controlplane.>).
const MembershipChangedSubject = "controlplane.space.membership_changed"

// ActionReannounce is the payload action of a sweep re-announcement. It is
// never a ledger action: a re-announcement has no ledger row.
const ActionReannounce = "reannounce"

// MembershipEventPublisher publishes one event and returns only after the
// broker acknowledged the write (JetStream PubAck).
type MembershipEventPublisher interface {
	PublishWithAck(ctx context.Context, subject, msgID string, payload map[string]any) error
}

// MembershipEventsOutboxConfig tunes the worker. Zero values take defaults.
type MembershipEventsOutboxConfig struct {
	PollEvery        time.Duration // outbox poll period (default 2s)
	BatchSize        int           // rows claimed per poll (default 100)
	Lease            time.Duration // claim lease (default 60s)
	ReannounceEvery  time.Duration // re-announce period (default 10m)
	ReannounceWindow time.Duration // memberships changed within this are re-announced (default 24h)
}

func (c MembershipEventsOutboxConfig) withDefaults() MembershipEventsOutboxConfig {
	if c.PollEvery <= 0 {
		c.PollEvery = 2 * time.Second
	}
	if c.BatchSize <= 0 {
		c.BatchSize = 100
	}
	if c.Lease <= 0 {
		c.Lease = 60 * time.Second
	}
	if c.ReannounceEvery <= 0 {
		c.ReannounceEvery = 10 * time.Minute
	}
	if c.ReannounceWindow <= 0 {
		c.ReannounceWindow = 24 * time.Hour
	}
	return c
}

const (
	reannouncePageSize = 500
	reannounceMaxPages = 10
	outboxErrorMax     = 200
)

// claimedMembershipEvent is one leased outbox row with the current fact.
type claimedMembershipEvent struct {
	seq       int64
	eventID   string
	spaceRef  string
	orgID     string
	spaceSeq  int64
	action    string
	actorKind string
	subjectID string
	toRole    string
	attempts  int
	// current membership row, when it still exists
	hasCurrent  bool
	curActive   bool
	currentRole string
}

// activeFact is the fact the event states. The live membership row wins over
// the ledger action so a stale row cannot override a newer one.
func (e claimedMembershipEvent) activeFact() bool {
	if e.hasCurrent {
		return e.curActive
	}
	return e.action == ActionMemberAdded || e.action == ActionMemberJoined
}

func (e claimedMembershipEvent) payload() map[string]any {
	role := e.toRole
	if e.hasCurrent && e.curActive {
		role = e.currentRole
	}
	return map[string]any{
		"space_ref":  e.spaceRef,
		"org_id":     e.orgID,
		"subject_id": e.subjectID,
		"active":     e.activeFact(),
		"event_id":   e.eventID,
		"space_seq":  e.spaceSeq,
		"action":     e.action,
		"role":       role,
		"actor_kind": e.actorKind,
	}
}

// RunMembershipEventsOutbox runs the durable publisher and the re-announce
// sweep until ctx is cancelled. Call it from a goroutine.
func (r *Repository) RunMembershipEventsOutbox(ctx context.Context, publisher MembershipEventPublisher, cfg MembershipEventsOutboxConfig) {
	if r == nil || publisher == nil {
		return
	}
	cfg = cfg.withDefaults()
	poll := time.NewTicker(cfg.PollEvery)
	defer poll.Stop()
	reannounce := time.NewTicker(cfg.ReannounceEvery)
	defer reannounce.Stop()
	for {
		for range 20 {
			n, err := r.PublishPendingMembershipEvents(ctx, publisher, cfg)
			if err != nil {
				if !errors.Is(err, context.Canceled) {
					log.Printf("user-core Space membership outbox: %v", err)
				}
				break
			}
			if n < cfg.BatchSize {
				break
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-reannounce.C:
			if err := r.ReannounceMembershipFacts(ctx, publisher, cfg); err != nil && !errors.Is(err, context.Canceled) {
				log.Printf("user-core Space membership re-announce: %v", err)
			}
		case <-poll.C:
		}
	}
}

func outboxBackoff(attempt int) time.Duration {
	if attempt < 1 {
		attempt = 1
	}
	return time.Duration(min(attempt*attempt, 300)) * time.Second
}

func truncateOutboxError(err error) string {
	message := strings.TrimSpace(err.Error())
	if len(message) > outboxErrorMax {
		message = message[:outboxErrorMax]
	}
	return message
}

// PublishPendingMembershipEvents claims up to cfg.BatchSize pending ledger
// rows, publishes each with a PubAck and records the outcome. It returns the
// number of rows claimed. A publish failure backs the row off and is not an
// error of the batch.
func (r *Repository) PublishPendingMembershipEvents(ctx context.Context, publisher MembershipEventPublisher, cfg MembershipEventsOutboxConfig) (int, error) {
	cfg = cfg.withDefaults()
	rows, err := r.db.Pool.Query(ctx, `
		WITH picked AS (
			SELECT event_id FROM space_membership_events
			WHERE published_at IS NULL
			  AND subject_type = 'user'
			  AND action IN ('member_added','member_joined','member_removed','member_left')
			  AND (next_publish_at IS NULL OR next_publish_at <= NOW())
			ORDER BY seq
			LIMIT $1
			FOR UPDATE SKIP LOCKED
		)
		UPDATE space_membership_events e
		SET publish_attempts = e.publish_attempts + 1,
		    next_publish_at  = NOW() + make_interval(secs => $2)
		FROM picked
		WHERE e.event_id = picked.event_id
		RETURNING e.seq, e.event_id, e.space_ref, e.org_id, e.space_seq, e.action, e.actor_kind,
		          e.subject_id, COALESCE(e.to_role, ''), e.publish_attempts,
		          (SELECT m.active FROM space_memberships m
		            WHERE m.space_ref = e.space_ref AND m.subject_type = 'user' AND m.subject_id = e.subject_id),
		          COALESCE((SELECT m.role FROM space_memberships m
		            WHERE m.space_ref = e.space_ref AND m.subject_type = 'user' AND m.subject_id = e.subject_id), '')`,
		cfg.BatchSize, cfg.Lease.Seconds())
	if err != nil {
		return 0, fmt.Errorf("claim membership events: %w", err)
	}
	var claimed []claimedMembershipEvent
	for rows.Next() {
		var e claimedMembershipEvent
		var curActive *bool
		if err := rows.Scan(&e.seq, &e.eventID, &e.spaceRef, &e.orgID, &e.spaceSeq, &e.action, &e.actorKind,
			&e.subjectID, &e.toRole, &e.attempts, &curActive, &e.currentRole); err != nil {
			rows.Close()
			return 0, fmt.Errorf("scan membership event: %w", err)
		}
		if curActive != nil {
			e.hasCurrent, e.curActive = true, *curActive
		}
		claimed = append(claimed, e)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, fmt.Errorf("claim membership events: %w", err)
	}
	sort.Slice(claimed, func(i, j int) bool { return claimed[i].seq < claimed[j].seq })

	for _, e := range claimed {
		if err := publisher.PublishWithAck(ctx, MembershipChangedSubject, e.eventID, e.payload()); err != nil {
			if ctx.Err() != nil {
				return len(claimed), ctx.Err()
			}
			if _, uerr := r.db.Pool.Exec(ctx, `
				UPDATE space_membership_events
				SET next_publish_at = NOW() + make_interval(secs => $3), last_error = $4
				WHERE event_id = $1 AND publish_attempts = $2 AND published_at IS NULL`,
				e.eventID, e.attempts, outboxBackoff(e.attempts).Seconds(), truncateOutboxError(err)); uerr != nil {
				log.Printf("user-core Space membership outbox: record failure for %s: %v", e.eventID, uerr)
			}
			continue
		}
		if _, err := r.db.Pool.Exec(ctx, `
			UPDATE space_membership_events
			SET published_at = NOW(), next_publish_at = NULL, last_error = NULL
			WHERE event_id = $1 AND published_at IS NULL`, e.eventID); err != nil {
			// The event is at the broker; an unrecorded completion only
			// repeats the publish after the lease, deduplicated by Msg-Id.
			log.Printf("user-core Space membership outbox: record completion for %s: %v", e.eventID, err)
		}
	}
	return len(claimed), nil
}

// reannounceMsgID is stable within one ReannounceEvery bucket (so replicas
// that sweep together deduplicate at the broker) and differs between
// buckets (so a later sweep is delivered again).
func reannounceMsgID(spaceRef, subjectID string, bucket int64) string {
	sum := sha256.Sum256([]byte(fmt.Sprintf("%s\x00%s\x00%d", spaceRef, subjectID, bucket)))
	return "sma_" + hex.EncodeToString(sum[:16])
}

// ReannounceMembershipFacts republishes the current active fact of every user
// membership changed within cfg.ReannounceWindow. The fact is read from
// space_memberships at sweep time, so it can never contradict a newer change.
// Idempotent: the consumer treats the same fact twice as a no-op.
func (r *Repository) ReannounceMembershipFacts(ctx context.Context, publisher MembershipEventPublisher, cfg MembershipEventsOutboxConfig) error {
	cfg = cfg.withDefaults()
	bucket := time.Now().UTC().Unix() / int64(cfg.ReannounceEvery.Seconds())
	var afterAt time.Time
	var afterRef, afterSubject string
	first := true
	type fact struct {
		updatedAt         time.Time
		spaceRef, subject string
		active            bool
		role, orgID       string
		spaceSeq          int64
	}
	for page := 0; page < reannounceMaxPages; page++ {
		rows, err := r.db.Pool.Query(ctx, `
			SELECT m.updated_at, m.space_ref, m.subject_id, m.active, m.role, s.org_id,
			       COALESCE(ar.authority_revision, 0)
			FROM space_memberships m
			JOIN registered_spaces s ON s.space_ref = m.space_ref
			LEFT JOIN space_authority_revisions ar ON ar.space_ref = m.space_ref
			WHERE m.subject_type = 'user'
			  AND m.updated_at > NOW() - make_interval(secs => $1)
			  AND ($2::boolean OR (m.updated_at, m.space_ref, m.subject_id) > ($3::timestamptz, $4::text, $5::text))
			ORDER BY m.updated_at, m.space_ref, m.subject_id
			LIMIT $6`,
			cfg.ReannounceWindow.Seconds(), first, afterAt, afterRef, afterSubject, reannouncePageSize)
		if err != nil {
			return fmt.Errorf("read memberships to re-announce: %w", err)
		}
		var facts []fact
		for rows.Next() {
			var f fact
			if err := rows.Scan(&f.updatedAt, &f.spaceRef, &f.subject, &f.active, &f.role, &f.orgID, &f.spaceSeq); err != nil {
				rows.Close()
				return fmt.Errorf("scan membership to re-announce: %w", err)
			}
			facts = append(facts, f)
		}
		rows.Close()
		if err := rows.Err(); err != nil {
			return fmt.Errorf("read memberships to re-announce: %w", err)
		}
		for _, f := range facts {
			msgID := reannounceMsgID(f.spaceRef, f.subject, bucket)
			role := ""
			if f.active {
				role = f.role
			}
			if err := publisher.PublishWithAck(ctx, MembershipChangedSubject, msgID, map[string]any{
				"space_ref":  f.spaceRef,
				"org_id":     f.orgID,
				"subject_id": f.subject,
				"active":     f.active,
				"event_id":   msgID,
				"space_seq":  f.spaceSeq,
				"action":     ActionReannounce,
				"role":       role,
				"actor_kind": ActorKindService,
			}); err != nil {
				if ctx.Err() != nil {
					return ctx.Err()
				}
				// The broker is failing: stop this sweep and let the next
				// tick retry rather than hammering it row by row.
				return fmt.Errorf("publish re-announcement: %w", err)
			}
		}
		if len(facts) < reannouncePageSize {
			return nil
		}
		last := facts[len(facts)-1]
		afterAt, afterRef, afterSubject, first = last.updatedAt, last.spaceRef, last.subject, false
	}
	return nil
}
