package watch

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// ErrWatchNotFound means no such watch in this organization.
var ErrWatchNotFound = errors.New("watch not found")

// ErrDuplicateActiveWatch means an identical ACTIVE watch already exists.
// Surfaced as its own error because a caller should show the existing watch
// rather than report a failure: the person asked to be told about something,
// and they already will be.
var ErrDuplicateActiveWatch = errors.New("an identical active watch already exists")

// watchDatabase is the narrow slice of pgxpool.Pool this store uses, so the SQL
// and the decision logic are unit-testable against a stub and only the
// transaction paths need a live Postgres.
type watchDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
	Begin(context.Context) (pgx.Tx, error)
}

const watchColumns = `id, org_id, space_ref, creator_subject_id, ` +
	`source_kind, source_ref, predicate_kind, predicate_value, predicate_stream, ` +
	`cursor_value, cursor_committed_at, state, trigger_mode, ` +
	`next_poll_at, idle_polls, last_polled_at, ` +
	`last_event_at, last_event_summary, consecutive_failures, expires_at, ` +
	`delivery_target_ref, ` +
	`recipient_audience_ref, recipient_audience_hash, resource_authorization_ref, ` +
	`privacy_policy_ref, authority_revision, membership_revision, privacy_revision, ` +
	`recipient_audience_revision, entitlement_revision, created_at, updated_at`

// Store is the durable watch registry backed by space_watches and
// space_watch_events (session-core migration 0036).
type Store struct {
	pool  watchDatabase
	nowFn func() time.Time
}

// NewStore constructs a store over an already-configured pool.
func NewStore(pool watchDatabase) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("a watch store requires a database pool")
	}
	return &Store{pool: pool, nowFn: func() time.Time { return time.Now().UTC() }}, nil
}

// Create records a new ACTIVE watch.
//
// The first poll is scheduled immediately rather than one interval out: a
// person who just asked to be told about something expects the answer to start
// arriving now, and a source that has already produced matching output should
// not be missed because the watch was created a second too late.
func (s *Store) Create(ctx context.Context, w Watch) (*Watch, error) {
	if err := w.Validate(); err != nil {
		return nil, err
	}
	now := s.nowFn()
	if w.ExpiresAt.Before(now) {
		return nil, fmt.Errorf("watch expires_at is already in the past")
	}
	tag, err := s.pool.Exec(ctx, `
		INSERT INTO space_watches (
			id, org_id, space_ref, creator_subject_id,
			source_kind, source_ref, predicate_kind, predicate_value, predicate_stream,
			cursor_value, state, trigger_mode, next_poll_at, expires_at,
			delivery_target_ref,
			recipient_audience_ref, recipient_audience_hash, resource_authorization_ref,
			privacy_policy_ref, authority_revision, membership_revision, privacy_revision,
			recipient_audience_revision, entitlement_revision, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
		        $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $25)
		ON CONFLICT DO NOTHING
	`,
		w.ID, w.OrgID, w.SpaceRef, w.CreatorSubjectID,
		w.SourceKind, w.SourceRef, w.Predicate.Kind, w.Predicate.Value, w.Predicate.Stream,
		w.CursorValue, int16(StateActive), w.TriggerMode, now, w.ExpiresAt,
		w.DeliveryTargetRef,
		w.Authority.RecipientAudienceRef, w.Authority.RecipientAudienceHash,
		w.Authority.ResourceAuthorizationRef, w.Authority.PrivacyPolicyRef,
		w.Authority.AuthorityRevision, w.Authority.MembershipRevision,
		w.Authority.PrivacyRevision, w.Authority.RecipientAudienceRevision,
		w.Authority.EntitlementRevision, now)
	if err != nil {
		return nil, fmt.Errorf("create watch: %w", err)
	}
	if tag.RowsAffected() == 0 {
		// The partial unique index refused it: an identical ACTIVE watch for
		// the same creator already exists. Watching the same thing for the same
		// reason twice is a duplicate, not two answers — and without the index
		// a retried create silently doubles the events a person receives.
		return nil, ErrDuplicateActiveWatch
	}
	w.State = StateActive
	w.NextPollAt = now
	w.CreatedAt = now
	w.UpdatedAt = now
	return &w, nil
}

// Get resolves one watch within an organization.
func (s *Store) Get(ctx context.Context, orgID, id string) (*Watch, error) {
	if orgID == "" || id == "" {
		return nil, fmt.Errorf("org_id and id are required")
	}
	row := s.pool.QueryRow(ctx, `SELECT `+watchColumns+`
		FROM space_watches WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL`, id, orgID)
	w, err := scanWatch(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrWatchNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get watch: %w", err)
	}
	return w, nil
}

// CountActiveForSpace reports how many watches are currently ACTIVE in a Space.
//
// Read separately from Create rather than folded into its INSERT, and that is a
// deliberate loosening: two concurrent creates can each see 7 and both land,
// putting a Space at 9. Acceptable here in a way it was not for S4.2's process
// limits, because a watch consumes a poll slot rather than an OS process —
// being one over the bound costs a little sweeper time, not a resource the host
// has to find. Folding it into the statement would mean a counting subquery on
// every create for a bound nobody is racing to exceed.
func (s *Store) CountActiveForSpace(ctx context.Context, orgID, spaceRef string) (int, error) {
	if orgID == "" || spaceRef == "" {
		return 0, fmt.Errorf("org_id and space_ref are required")
	}
	var count int
	if err := s.pool.QueryRow(ctx, `
		SELECT count(*) FROM space_watches
		WHERE org_id = $1 AND space_ref = $2 AND state = $3 AND deleted_at IS NULL
	`, orgID, spaceRef, int16(StateActive)).Scan(&count); err != nil {
		return 0, fmt.Errorf("count active watches: %w", err)
	}
	return count, nil
}

// ListForSpace returns a Space's watches, newest first.
//
// Terminal watches are hidden unless asked for: "what am I being told about" is
// the question a person opens this list with, and a month of finished watches
// buries the answer.
func (s *Store) ListForSpace(ctx context.Context, orgID, spaceRef string, includeFinished bool) ([]Watch, error) {
	if orgID == "" || spaceRef == "" {
		return nil, fmt.Errorf("org_id and space_ref are required")
	}
	rows, err := s.pool.Query(ctx, `
		SELECT `+watchColumns+`
		FROM space_watches
		WHERE org_id = $1 AND space_ref = $2 AND deleted_at IS NULL
		  AND ($3 OR state = $4)
		ORDER BY id DESC
		LIMIT 200
	`, orgID, spaceRef, includeFinished, int16(StateActive))
	if err != nil {
		return nil, fmt.Errorf("list watches: %w", err)
	}
	defer rows.Close()
	var out []Watch
	for rows.Next() {
		w, scanErr := scanWatch(rows)
		if scanErr != nil {
			return nil, fmt.Errorf("scan watch: %w", scanErr)
		}
		out = append(out, *w)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list watches: %w", err)
	}
	return out, nil
}

// ClaimDue takes up to limit watches whose next_poll_at has passed and advances
// each one's next_poll_at before returning it.
//
// # Why the claim commits BEFORE the work
//
// The cron sweeper does everything — including a network call to Control — in
// one transaction holding row locks on every claimed row. That is survivable at
// one fire per minute. A watch polls every two seconds and its work is a remote
// read, so the same shape would hold locks across an RPC for every watch in the
// fleet at once.
//
// Instead the claim is its own short transaction: select FOR UPDATE SKIP LOCKED,
// push next_poll_at forward by the current backoff, commit. A second replica
// then cannot re-pick the row, and no transaction is open while the adapter
// talks to the network. The cost is that a sweeper which dies after claiming
// delays that watch by one interval — the right trade against holding locks
// across a remote call, and invisible next to a 2-30s poll interval anyway.
//
// Expired watches are not claimed. [Store.ExpireOverdue] terminates them, so a
// watch past its end never performs a read it has no authority for.
func (s *Store) ClaimDue(ctx context.Context, limit int) ([]Watch, error) {
	if limit <= 0 {
		return nil, fmt.Errorf("claim limit must be positive")
	}
	now := s.nowFn()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("claim watches: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	rows, err := tx.Query(ctx, `
		SELECT `+watchColumns+`
		FROM space_watches
		WHERE state = $1 AND deleted_at IS NULL
		  AND next_poll_at <= $2
		  AND expires_at > $2
		ORDER BY next_poll_at
		FOR UPDATE SKIP LOCKED
		LIMIT $3
	`, int16(StateActive), now, limit)
	if err != nil {
		return nil, fmt.Errorf("claim watches: %w", err)
	}
	var claimed []Watch
	for rows.Next() {
		w, scanErr := scanWatch(rows)
		if scanErr != nil {
			rows.Close()
			return nil, fmt.Errorf("scan watch: %w", scanErr)
		}
		claimed = append(claimed, *w)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("claim watches: %w", err)
	}

	for i := range claimed {
		next := now.Add(PollInterval(claimed[i].IdlePolls))
		if _, err := tx.Exec(ctx, `
			UPDATE space_watches SET next_poll_at = $1, last_polled_at = $2, updated_at = $2
			WHERE id = $3
		`, next, now, claimed[i].ID); err != nil {
			return nil, fmt.Errorf("claim watch %s: %w", claimed[i].ID, err)
		}
		claimed[i].NextPollAt = next
		claimed[i].LastPolledAt = &now
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("claim watches: %w", err)
	}
	return claimed, nil
}

// Emission is one event a poll decided to record, with the cursor it belongs at.
type Emission struct {
	ID      string
	Kind    string
	Summary string
	Trust   string
	// Cursor is the position this event was observed AT, which together with
	// the watch and the kind makes it idempotent.
	Cursor int64
}

// CommitPoll records the outcome of one poll: any event, the new cursor, and
// the backoff.
//
// # The ordering is the whole correctness argument
//
// The event insert and the cursor move happen in ONE transaction, event first.
//
//   - An event without a committed cursor is re-derived on the next poll (the
//     adapter is idempotent in the cursor) and deduplicated by the unique index
//     on (watch_id, cursor_value, kind). Harmless.
//   - A committed cursor without its event would silently skip a match, and
//     nothing downstream could ever discover it. That is the failure this
//     ordering exists to make impossible.
//
// Terminal states are applied here too, so a source that ended is recorded and
// stopped in the same transaction that records its last event — a separate call
// would leave a window where the watch polls a source it has already reported
// as gone.
func (s *Store) CommitPoll(ctx context.Context, w Watch, events []Emission, cursor int64, idle bool, terminal State) error {
	if w.ID == "" || w.OrgID == "" {
		return fmt.Errorf("watch id and org_id are required to commit a poll")
	}
	if cursor < w.CursorValue {
		// A cursor that moves backwards would re-emit everything between the
		// two positions on the next poll. An adapter that produced one is
		// broken, and accepting it would turn that bug into duplicate events
		// rather than a refusal.
		return fmt.Errorf("cursor must not move backwards (%d -> %d)", w.CursorValue, cursor)
	}
	now := s.nowFn()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("commit poll: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	lastSummary := ""
	for _, event := range events {
		if err := validateEmission(event); err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_watch_events (id, watch_id, org_id, cursor_value, kind, summary, trust, emitted_at)
			VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
			ON CONFLICT (watch_id, cursor_value, kind) DO NOTHING
		`, event.ID, w.ID, w.OrgID, event.Cursor, event.Kind,
			BoundSummary(event.Summary), event.Trust, now); err != nil {
			return fmt.Errorf("record watch event: %w", err)
		}
		lastSummary = BoundSummary(event.Summary)
	}

	// The backoff level. Any event at all resets it: a source that just spoke
	// is a source worth listening to closely.
	idlePolls := 0
	if idle && len(events) == 0 {
		idlePolls = w.IdlePolls + 1
		if idlePolls > maxIdleShift {
			idlePolls = maxIdleShift
		}
	}
	nextState := StateActive
	if terminal != StateActive {
		nextState = terminal
	} else if len(events) > 0 && w.TriggerMode == TriggerOnce {
		nextState = StateTriggered
	}

	if _, err := tx.Exec(ctx, `
		UPDATE space_watches
		SET cursor_value = $1,
		    cursor_committed_at = $2,
		    idle_polls = $3,
		    next_poll_at = $4,
		    state = $5,
		    consecutive_failures = 0,
		    last_event_at = CASE WHEN $6 THEN $2 ELSE last_event_at END,
		    last_event_summary = CASE WHEN $6 THEN $7 ELSE last_event_summary END,
		    updated_at = $2
		WHERE id = $8 AND org_id = $9 AND state = $10
	`, cursor, now, idlePolls, now.Add(PollInterval(idlePolls)), int16(nextState),
		len(events) > 0, lastSummary, w.ID, w.OrgID, int16(StateActive)); err != nil {
		return fmt.Errorf("commit watch cursor: %w", err)
	}
	// The `state = ACTIVE` fence in the UPDATE is deliberate and unchecked on
	// purpose: if a member cancelled the watch while this poll was in flight,
	// the update matches nothing and the cancellation wins. The event rows
	// already inserted are still correct — they record what was observed — and
	// the watch stays cancelled. That is the unwatch race, resolved in favour
	// of the person who asked to stop.
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit poll: %w", err)
	}
	return nil
}

// RecordFailure notes a failed poll and backs the watch off.
//
// It never terminates. A transient outage in the source's owning service must
// not silently cancel a person's watch — the only things that end a watch are
// the member, the clock, and the source itself.
func (s *Store) RecordFailure(ctx context.Context, w Watch) error {
	now := s.nowFn()
	failures := w.ConsecutiveFailures + 1
	idlePolls := w.IdlePolls
	if failures >= MaxFailuresBeforeBackoff {
		idlePolls = maxIdleShift
	} else if idlePolls < maxIdleShift {
		idlePolls++
	}
	_, err := s.pool.Exec(ctx, `
		UPDATE space_watches
		SET consecutive_failures = $1, idle_polls = $2, next_poll_at = $3, updated_at = $4
		WHERE id = $5 AND org_id = $6 AND state = $7
	`, failures, idlePolls, now.Add(PollInterval(idlePolls)), now,
		w.ID, w.OrgID, int16(StateActive))
	if err != nil {
		return fmt.Errorf("record watch failure: %w", err)
	}
	return nil
}

// Terminate ends a watch.
//
// Fenced on the watch still being ACTIVE, so two terminations race to one
// winner and a cancelled watch is never re-terminated as expired. Terminating
// an already-terminal watch is a no-op rather than an error: the caller's
// intent — that this watch stops — is already satisfied.
func (s *Store) Terminate(ctx context.Context, orgID, id string, state State, summary string) error {
	if !state.Terminal() {
		return fmt.Errorf("%s is not a terminal state", state)
	}
	now := s.nowFn()
	_, err := s.pool.Exec(ctx, `
		UPDATE space_watches
		SET state = $1,
		    last_event_summary = CASE WHEN $2 = '' THEN last_event_summary ELSE $2 END,
		    updated_at = $3
		WHERE id = $4 AND org_id = $5 AND state = $6 AND deleted_at IS NULL
	`, int16(state), BoundSummary(summary), now, id, orgID, int16(StateActive))
	if err != nil {
		return fmt.Errorf("terminate watch: %w", err)
	}
	return nil
}

// ExpireOverdue terminates every ACTIVE watch past its expiry.
//
// Separate from the claim rather than folded into it: a watch that has expired
// must stop being polled even if no sweeper ever claims it again, and a set of
// rows that are due but not claimable is the kind of state that quietly grows.
func (s *Store) ExpireOverdue(ctx context.Context, limit int) (int64, error) {
	if limit <= 0 {
		return 0, fmt.Errorf("expire limit must be positive")
	}
	now := s.nowFn()
	tag, err := s.pool.Exec(ctx, `
		UPDATE space_watches
		SET state = $1, updated_at = $2
		WHERE id IN (
			SELECT id FROM space_watches
			WHERE state = $3 AND deleted_at IS NULL AND expires_at <= $2
			ORDER BY expires_at
			LIMIT $4
		)
	`, int16(StateExpired), now, int16(StateActive), limit)
	if err != nil {
		return 0, fmt.Errorf("expire watches: %w", err)
	}
	return tag.RowsAffected(), nil
}

func validateEmission(event Emission) error {
	if event.ID == "" {
		return fmt.Errorf("watch event id is required")
	}
	switch event.Kind {
	case EventMatch, EventStateChange, EventGap, EventSourceGone, EventExpired:
	default:
		return fmt.Errorf("watch event kind %q is not recognized", event.Kind)
	}
	switch event.Trust {
	case TrustUnscreened, TrustOwnerMetadata:
	default:
		return fmt.Errorf("watch event trust %q is not recognized", event.Trust)
	}
	// A match is derived from watched content by definition, so it can never be
	// owner metadata. Without this, an adapter could launder a payload into the
	// label a consumer renders as trustworthy — which is the entire failure the
	// trust column exists to prevent.
	if event.Kind == EventMatch && event.Trust != TrustUnscreened {
		return fmt.Errorf("a %q event is derived from watched content and must be %q", EventMatch, TrustUnscreened)
	}
	if event.Cursor < 0 {
		return fmt.Errorf("watch event cursor must not be negative")
	}
	return nil
}

type rowScanner interface {
	Scan(dest ...any) error
}

func scanWatch(row rowScanner) (*Watch, error) {
	var (
		w     Watch
		state int16
	)
	if err := row.Scan(
		&w.ID, &w.OrgID, &w.SpaceRef, &w.CreatorSubjectID,
		&w.SourceKind, &w.SourceRef, &w.Predicate.Kind, &w.Predicate.Value, &w.Predicate.Stream,
		&w.CursorValue, &w.CursorCommittedAt, &state, &w.TriggerMode,
		&w.NextPollAt, &w.IdlePolls, &w.LastPolledAt,
		&w.LastEventAt, &w.LastEventSummary, &w.ConsecutiveFailures, &w.ExpiresAt,
		&w.DeliveryTargetRef,
		&w.Authority.RecipientAudienceRef, &w.Authority.RecipientAudienceHash,
		&w.Authority.ResourceAuthorizationRef, &w.Authority.PrivacyPolicyRef,
		&w.Authority.AuthorityRevision, &w.Authority.MembershipRevision,
		&w.Authority.PrivacyRevision, &w.Authority.RecipientAudienceRevision,
		&w.Authority.EntitlementRevision, &w.CreatedAt, &w.UpdatedAt,
	); err != nil {
		return nil, err
	}
	w.State = State(state)
	return &w, nil
}
