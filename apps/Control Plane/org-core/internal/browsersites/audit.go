package browsersites

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// F-f06-11 (WS21-S6, audit K3-11): every browser site policy write, and every member choice
// write or delete, queues a content-free audit intent in the same transaction as the write. The
// publisher loop in cmd/server publishes the intent from browser_site_audit_outbox (migration 036).
// A choice carries its host and level; a policy carries its mode and rule count. No rule host,
// note or other free text is ever part of an intent.

// EnvAuditEnabled switches the browser site audit outbox on. Only the literal "true" turns it
// on. Off, a write queues no intent and is unchanged. Intents already queued still publish,
// because the publisher loop runs whatever the switch says.
const EnvAuditEnabled = "ORG_BROWSER_SITE_AUDIT_ENABLED"

// AuditEnabled reads EnvAuditEnabled strictly, as the other org-core switches do: a typo, "1"
// or a padded value leaves the outbox off.
func AuditEnabled(getenv func(string) string) bool {
	return getenv(EnvAuditEnabled) == "true"
}

// The two audit events. audit-core ingests each subject as a Control event.
const (
	BrowserSitePolicyChanged = "browser_site_policy_changed"
	BrowserSitePrefChanged   = "browser_site_pref_changed"
)

// browserSiteAuditSubjectPrefix is the authority prefix of both subjects. Migration 036 pins the
// two full subjects with a CHECK, so a row cannot name any other subject.
const browserSiteAuditSubjectPrefix = "verevon.audit.v2.control.org-core."

// browserSiteAuditOutboxTable is a constant, never caller input: the statements below are built
// with it.
const browserSiteAuditOutboxTable = "browser_site_audit_outbox"

// browserSiteAuditMaxAttempts bounds publication retries before a row is dead-lettered. It matches
// the attempts CHECK of migration 036.
const browserSiteAuditMaxAttempts = 8

// browserSiteAuditFlushLimit bounds the rows one inline flush claims.
const browserSiteAuditFlushLimit = 100

// ErrAuditActorRequired refuses a write made with the switch on but without a verified actor and
// role. The write is refused before it starts, so no change is stored without its intent.
var ErrAuditActorRequired = errors.New("a browser site audit needs the verified actor and its role")

// AuditPublisher is satisfied by org-core's NATS client (PublishAudit), the same seam as
// org.AuditPublisher. It is declared here so this package does not import org.
type AuditPublisher interface {
	PublishAudit(ctx context.Context, subject, eventID string, payload map[string]any) error
}

// AuditFlushResult counts one flush of the browser site audit outbox.
type AuditFlushResult struct {
	Published    int `json:"published"`
	DeadLettered int `json:"dead_lettered"`
}

// AuditOutboxRow is one claimed intent: its event id, subject, organisation and payload.
type AuditOutboxRow struct {
	EventID  string
	OrgID    string
	Subject  string
	Payload  map[string]any
	Attempts int
}

// auditOutbox is the outbox side of a Store that the flush needs. *Repository has it. A Store
// without it has no outbox to flush, so a flush reports that it cannot run.
type auditOutbox interface {
	claimAuditOutbox(ctx context.Context, limit int) ([]AuditOutboxRow, error)
	markAuditPublished(ctx context.Context, eventID string) error
	markAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error)
}

// writerKey carries the verified writer of one browser site write from the Service into the
// Repository's transaction. Only the Service sets it, from the caller the handler authorised, so
// the intent names the same actor and role as the write it records.
type writerKey struct{}

// writer is who made one write and whether that write queues its intent. audit is the switch the
// Service read once at startup.
type writer struct {
	id    string
	role  string
	audit bool
}

func withWriter(ctx context.Context, w writer) context.Context {
	return context.WithValue(ctx, writerKey{}, w)
}

// writerFrom returns the writer the Service put on ctx. A write without one queues no intent.
func writerFrom(ctx context.Context) (writer, bool) {
	w, ok := ctx.Value(writerKey{}).(writer)
	return w, ok
}

// auditChange is one write to record: its event, its organisation, the revision it landed at, the
// canonical host it named (identity only), the history row it was written with, and the details
// the intent carries. The details hold the content-free fields of the change and nothing else.
type auditChange struct {
	orgID     string
	event     string
	revision  int64
	host      string
	historyID int64
	details   map[string]any
}

// enqueueAudit inserts one audit intent in the write's own transaction, so the intent exists if
// and only if the write committed. It does nothing unless the Service put a writer with the switch
// on on ctx. With the switch on, a writer without an id or role is refused.
func enqueueAudit(ctx context.Context, tx pgx.Tx, change auditChange) error {
	w, ok := writerFrom(ctx)
	if !ok || !w.audit {
		return nil
	}
	if strings.TrimSpace(w.id) == "" || strings.TrimSpace(w.role) == "" {
		return ErrAuditActorRequired
	}
	eventID, payload := auditPayload(change, w, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode browser site audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO `+browserSiteAuditOutboxTable+` (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, change.orgID, browserSiteAuditSubjectPrefix+change.event, body); err != nil {
		return fmt.Errorf("record browser site audit outbox: %w", err)
	}
	return nil
}

// auditPayload is the content-free intent: the event, the organisation, the actor and its role,
// the revision, and the change's details. The event id hashes the write's identity: the
// organisation, the event, the revision, the host, the actor and the history row. The actor and
// the history row are in the hash because two members writing one host at one policy revision,
// or one member writing it twice, are two events. A publish retry carries the same id, so
// JetStream drops the duplicate.
func auditPayload(change auditChange, w writer, occurredAt time.Time) (string, map[string]any) {
	identity := sha256.Sum256([]byte(strings.Join([]string{
		change.orgID,
		change.event,
		strconv.FormatInt(change.revision, 10),
		change.host,
		w.id,
		strconv.FormatInt(change.historyID, 10),
	}, "\x00")))
	eventID := "browsersite:org-core:" + hex.EncodeToString(identity[:])
	return eventID, map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": change.orgID, "user_id": w.id, "actor_role": w.role,
		"plane": "control", "producer": "org-core", "event": change.event,
		"subject": "browser_site:" + change.orgID, "resource_id": change.orgID, "outcome": "ok",
		"details": change.details,
	}
}

// SetAuditEnabled sets the audit switch. The composition root reads EnvAuditEnabled once at
// startup and passes the result here. With the switch off a write queues no intent.
func (s *Service) SetAuditEnabled(on bool) {
	s.auditOn = on
}

// SetAuditPublisher wires the durable audit dispatcher the flush publishes through. Pass a nil
// interface, never a typed nil: a typed nil is not nil to the check in FlushBrowserSiteAuditOutbox.
func (s *Service) SetAuditPublisher(publisher AuditPublisher) {
	s.auditPublisher = publisher
}

// writeContext returns ctx carrying the verified writer for the write's transaction. With the
// switch on, a write needs an actor and a role to be audited, so one without them is refused before
// it starts. With the switch off the writer is carried but queues nothing.
func (s *Service) writeContext(ctx context.Context, actorID, actorRole string) (context.Context, error) {
	actorID = strings.TrimSpace(actorID)
	actorRole = strings.TrimSpace(actorRole)
	if s.auditOn && (actorID == "" || actorRole == "") {
		return ctx, ErrAuditActorRequired
	}
	return withWriter(ctx, writer{id: actorID, role: actorRole, audit: s.auditOn}), nil
}

// flushAfterWrite publishes the intents a committed write queued, once, as best effort. A failed
// publish stays pending for the publisher loop and never fails the write.
func (s *Service) flushAfterWrite(ctx context.Context, orgID string) {
	if !s.auditOn || s.auditPublisher == nil {
		return
	}
	if _, err := s.FlushBrowserSiteAuditOutbox(ctx, browserSiteAuditFlushLimit); err != nil {
		log.Printf("browser sites: audit flush after a write for org=%s failed, left pending: %v", orgID, err)
	}
}

// FlushBrowserSiteAuditOutbox claims up to limit pending intents and publishes them. A published
// intent is marked; a failed one is marked with its error and retried later, and dead-lettered
// after browserSiteAuditMaxAttempts. A Service without a publisher, or whose store has no outbox,
// cannot flush and says so.
func (s *Service) FlushBrowserSiteAuditOutbox(ctx context.Context, limit int) (AuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return AuditFlushResult{}, errors.New("browser site audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return AuditFlushResult{}, errors.New("browser site audit publisher is unavailable")
	}
	outbox, ok := s.store.(auditOutbox)
	if !ok {
		return AuditFlushResult{}, errors.New("browser site audit outbox is unavailable")
	}
	rows, err := outbox.claimAuditOutbox(ctx, limit)
	if err != nil {
		return AuditFlushResult{}, err
	}
	result := AuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := outbox.markAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := outbox.markAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}

// claimAuditOutbox claims up to limit pending intents for one publish pass, on the owner pool. The
// claim sets processing_at inside its own transaction, so a second pass skips a row that is being
// published, until a minute passes without a mark.
func (r *Repository) claimAuditOutbox(ctx context.Context, limit int) ([]AuditOutboxRow, error) {
	if r == nil || r.db == nil {
		return nil, errors.New("browser site repository not configured")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin browser site audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM `+browserSiteAuditOutboxTable+`
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE `+browserSiteAuditOutboxTable+` o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, browserSiteAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim browser site audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]AuditOutboxRow, 0)
	for rows.Next() {
		var row AuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan browser site audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode browser site audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate browser site audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit browser site audit outbox claim: %w", err)
	}
	return claimed, nil
}

// markAuditPublished records that one intent reached the audit stream. It must match exactly one
// pending row.
func (r *Repository) markAuditPublished(ctx context.Context, eventID string) error {
	if r == nil || r.db == nil {
		return errors.New("browser site repository not configured")
	}
	result, err := r.db.Pool.Exec(ctx, `
UPDATE `+browserSiteAuditOutboxTable+`
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark browser site audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return errors.New("browser site audit acknowledgement did not match a pending event")
	}
	return nil
}

// markAuditPublishFailed records a failed publish: the attempt, the error (truncated) and the next
// attempt time, with exponential back-off capped at five minutes. It reports whether the row is now
// dead-lettered.
func (r *Repository) markAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	if r == nil || r.db == nil {
		return false, errors.New("browser site repository not configured")
	}
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.db.Pool.QueryRow(ctx, `
UPDATE `+browserSiteAuditOutboxTable+`
SET attempts = attempts + 1,
    processing_at = NULL,
    next_attempt_at = NOW() + LEAST(
      INTERVAL '5 minutes',
      INTERVAL '1 second' * power(2, LEAST(attempts, 8))
    ),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3,
    updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL
RETURNING dead_lettered_at IS NOT NULL`, eventID, browserSiteAuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, errors.New("browser site audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record browser site audit publish failure: %w", err)
	}
	return deadLettered, nil
}
