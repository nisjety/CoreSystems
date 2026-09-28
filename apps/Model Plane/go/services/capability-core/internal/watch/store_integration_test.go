//go:build integration

package watch

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// Why these tests exist at all, stated where the next person will read it:
//
// The store's unit-testable surface is its decision logic, and that is covered
// without a database. What is NOT coverable that way is the SQL — and the SQL
// is where this repo has actually been bitten. S4.2's process store passed
// every stub test (the text was right, the bound arguments were right) and
// still failed 14 of 17 integration tests, because one parameter was used both
// as an INTEGER column and inside bigint interval arithmetic and only a real
// planner deduces types. A 25-parameter INSERT, a claim with FOR UPDATE SKIP
// LOCKED, and an UPDATE fenced on state are all in the same category.
//
// Run with: go test -tags integration ./internal/watch/

// setupWatchStore spins a throwaway Postgres and applies session-core migration
// 0036.
//
// 0036 alone is enough: space_watches references nothing, and
// space_watch_events' only foreign key is onto space_watches in the same file.
// That independence is deliberate — a watch is a standing intent about a
// resource it identifies by reference, not a row joined to one.
func setupWatchStore(t *testing.T) (*Store, *pgxpool.Pool) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("session_core"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)

	migration := filepath.Join("..", "..", "..", "..", "..", "rust", "services", "session-core",
		"migrations", "0036_space_watches.sql")
	sql, err := os.ReadFile(migration)
	if err != nil {
		t.Fatalf("read %s: %v — if the migration moved, re-point this test rather than deleting it", migration, err)
	}
	if _, err := pool.Exec(ctx, string(sql)); err != nil {
		t.Fatalf("apply 0036: %v", err)
	}

	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	return store, pool
}

func newWatch(id string, p Predicate, mode string) Watch {
	return Watch{
		ID: id, OrgID: "org-1", SpaceRef: "space-1", CreatorSubjectID: "user-1",
		SourceKind: SourceKindProcessOutput, SourceRef: "proc-1",
		Predicate: p, State: StateActive, TriggerMode: mode,
		ExpiresAt: time.Now().UTC().Add(time.Hour),
		Authority: Authority{
			RecipientAudienceRef:      "space:space-1:recipient-audience:4",
			RecipientAudienceRevision: 4, AuthorityRevision: 3,
		},
	}
}

func TestCreateRoundTripsEveryColumn(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()

	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateContains, Value: "ERROR", Stream: StreamStderr}, TriggerOnce))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	got, err := store.Get(ctx, "org-1", created.ID)
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.Predicate.Kind != PredicateContains || got.Predicate.Value != "ERROR" || got.Predicate.Stream != StreamStderr {
		t.Fatalf("predicate did not round-trip: %+v", got.Predicate)
	}
	if got.State != StateActive || got.TriggerMode != TriggerOnce {
		t.Fatalf("state/mode did not round-trip: %s / %s", got.State, got.TriggerMode)
	}
	// The authority binding is the whole point of storing it — a sweeper that
	// read back zeros would compare current authority against nothing and
	// conclude nothing had changed.
	if got.Authority.RecipientAudienceRevision != 4 || got.Authority.AuthorityRevision != 3 {
		t.Fatalf("authority revisions did not round-trip: %+v", got.Authority)
	}
	if got.Authority.RecipientAudienceRef != "space:space-1:recipient-audience:4" {
		t.Fatalf("audience ref did not round-trip: %q", got.Authority.RecipientAudienceRef)
	}
	// The first poll is scheduled immediately: a person who just asked to be
	// told about something expects the answer to start arriving now.
	if got.NextPollAt.After(time.Now().UTC().Add(time.Second)) {
		t.Fatalf("next_poll_at is %s, want ~now", got.NextPollAt)
	}
}

func TestCreateRefusesADuplicateActiveWatch(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	p := Predicate{Kind: PredicateContains, Value: "ERROR"}

	if _, err := store.Create(ctx, newWatch("w1", p, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	_, err := store.Create(ctx, newWatch("w2", p, TriggerOnce))
	if !errors.Is(err, ErrDuplicateActiveWatch) {
		t.Fatalf("second identical watch returned %v, want ErrDuplicateActiveWatch — without this a retried create silently doubles the events a person receives", err)
	}
	// A DIFFERENT predicate is a different question and must be allowed.
	if _, err := store.Create(ctx, newWatch("w3", Predicate{Kind: PredicateContains, Value: "WARN"}, TriggerOnce)); err != nil {
		t.Fatalf("a different predicate was refused: %v", err)
	}
}

func TestClaimDueTakesOnlyDueActiveWatchesAndAdvancesThem(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()

	if _, err := store.Create(ctx, newWatch("due", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	future := newWatch("later", Predicate{Kind: PredicateContains, Value: "x"}, TriggerOnce)
	if _, err := store.Create(ctx, future); err != nil {
		t.Fatalf("Create: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE space_watches SET next_poll_at = now() + interval '1 hour' WHERE id = 'later'`); err != nil {
		t.Fatalf("push later: %v", err)
	}

	claimed, err := store.ClaimDue(ctx, 10)
	if err != nil {
		t.Fatalf("ClaimDue: %v", err)
	}
	if len(claimed) != 1 || claimed[0].ID != "due" {
		t.Fatalf("claimed %d watches %v, want only the due one", len(claimed), claimed)
	}

	// The claim advanced next_poll_at BEFORE any work, so an immediate second
	// sweep — another replica, or the same one a tick later — picks up nothing.
	// Without this the two would poll the same source in parallel.
	again, err := store.ClaimDue(ctx, 10)
	if err != nil {
		t.Fatalf("second ClaimDue: %v", err)
	}
	if len(again) != 0 {
		t.Fatalf("a just-claimed watch was claimed again by %d sweeps", len(again)+1)
	}
}

func TestClaimDueSkipsExpiredAndTerminalWatches(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()

	if _, err := store.Create(ctx, newWatch("expired", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE space_watches SET expires_at = now() - interval '1 minute' WHERE id = 'expired'`); err != nil {
		t.Fatalf("expire: %v", err)
	}
	if _, err := store.Create(ctx, newWatch("cancelled", Predicate{Kind: PredicateContains, Value: "x"}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	if err := store.Terminate(ctx, "org-1", "cancelled", StateCancelled, "member unwatched"); err != nil {
		t.Fatalf("Terminate: %v", err)
	}

	claimed, err := store.ClaimDue(ctx, 10)
	if err != nil {
		t.Fatalf("ClaimDue: %v", err)
	}
	if len(claimed) != 0 {
		// An expired watch that still polls is a poller reading a resource its
		// authority has run out on.
		t.Fatalf("claimed %d watches, want none — an expired or terminal watch must never observe anything", len(claimed))
	}

	expired, err := store.ExpireOverdue(ctx, 10)
	if err != nil {
		t.Fatalf("ExpireOverdue: %v", err)
	}
	if expired != 1 {
		t.Fatalf("expired %d watches, want 1", expired)
	}
	got, err := store.Get(ctx, "org-1", "expired")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateExpired {
		t.Fatalf("state = %s, want EXPIRED", got.State)
	}
	// A cancelled watch is NOT re-terminated as expired: the fence on ACTIVE
	// means the first terminal answer wins.
	cancelled, err := store.Get(ctx, "org-1", "cancelled")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if cancelled.State != StateCancelled {
		t.Fatalf("a cancelled watch became %s", cancelled.State)
	}
}

func TestCommitPollWritesTheEventAndTheCursorTogether(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}

	emission := Emission{ID: "wev_1", Kind: EventMatch, Summary: "ERROR: boom", Trust: TrustUnscreened, Cursor: 5}
	if err := store.CommitPoll(ctx, *created, []Emission{emission}, 5, false, StateActive); err != nil {
		t.Fatalf("CommitPoll: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.CursorValue != 5 {
		t.Fatalf("cursor = %d, want 5", got.CursorValue)
	}
	if got.State != StateTriggered {
		t.Fatalf("a `once` watch that emitted is %s, want TRIGGERED", got.State)
	}
	if got.LastEventSummary != "ERROR: boom" || got.LastEventAt == nil {
		t.Fatalf("last event not recorded: %q / %v", got.LastEventSummary, got.LastEventAt)
	}
	if got.IdlePolls != 0 {
		t.Fatalf("idle_polls = %d after an event; any event resets the backoff", got.IdlePolls)
	}
}

// The crash-before-cursor-commit case, and the reason the unique index exists:
// a re-derived poll re-inserts the same event at the same cursor and must be a
// no-op rather than a second event.
func TestCommitPollIsIdempotentAtTheSameCursor(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerContinuous))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}

	emission := Emission{ID: "wev_1", Kind: EventMatch, Summary: "line", Trust: TrustUnscreened, Cursor: 3}
	if err := store.CommitPoll(ctx, *created, []Emission{emission}, 3, false, StateActive); err != nil {
		t.Fatalf("first CommitPoll: %v", err)
	}
	// Same cursor, DIFFERENT event id — a redelivery mints a fresh id, so
	// deduplication cannot rely on the primary key.
	replay := Emission{ID: "wev_2", Kind: EventMatch, Summary: "line", Trust: TrustUnscreened, Cursor: 3}
	if err := store.CommitPoll(ctx, *created, []Emission{replay}, 3, false, StateActive); err != nil {
		t.Fatalf("replayed CommitPoll: %v", err)
	}

	var count int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM space_watch_events WHERE watch_id = 'w1'`).Scan(&count); err != nil {
		t.Fatalf("count: %v", err)
	}
	if count != 1 {
		t.Fatalf("a replayed poll produced %d events, want 1", count)
	}
}

// A cursor that moves backwards would re-emit everything between the two
// positions on the next poll. An adapter that produced one is broken, and
// accepting it would turn that bug into duplicate events rather than a refusal.
func TestCommitPollRefusesABackwardsCursor(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerContinuous))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	if err := store.CommitPoll(ctx, *created, nil, 10, true, StateActive); err != nil {
		t.Fatalf("CommitPoll: %v", err)
	}
	advanced, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if err := store.CommitPoll(ctx, *advanced, nil, 4, true, StateActive); err == nil {
		t.Fatal("a backwards cursor was accepted")
	}
}

// The unwatch race, resolved in favour of the person who asked to stop: the
// UPDATE is fenced on the watch still being ACTIVE, so a cancellation that
// lands mid-poll wins. The events already observed are still recorded — they
// are a true account of what happened — but the watch does not resume.
func TestACancellationDuringAPollWins(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerContinuous))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	// The member unwatches while the poll is in flight.
	if err := store.Terminate(ctx, "org-1", "w1", StateCancelled, "member unwatched"); err != nil {
		t.Fatalf("Terminate: %v", err)
	}
	// The in-flight poll now commits against a cancelled row.
	emission := Emission{ID: "wev_1", Kind: EventMatch, Summary: "line", Trust: TrustUnscreened, Cursor: 2}
	if err := store.CommitPoll(ctx, *created, []Emission{emission}, 2, false, StateActive); err != nil {
		t.Fatalf("CommitPoll against a cancelled watch errored: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateCancelled {
		t.Fatalf("state = %s, want CANCELLED — the cancellation must win the race", got.State)
	}
	if got.CursorValue != 0 {
		t.Fatalf("a cancelled watch's cursor moved to %d", got.CursorValue)
	}
}

// Backoff: an empty poll raises the level, an event resets it, and the
// derived interval is what next_poll_at actually moves by.
func TestIdlePollsBackOffAndResetOnAnEvent(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}

	current := *created
	for expected := 1; expected <= 3; expected++ {
		if err := store.CommitPoll(ctx, current, nil, int64(expected), true, StateActive); err != nil {
			t.Fatalf("idle CommitPoll %d: %v", expected, err)
		}
		got, err := store.Get(ctx, "org-1", "w1")
		if err != nil {
			t.Fatalf("Get: %v", err)
		}
		if got.IdlePolls != expected {
			t.Fatalf("after %d idle polls idle_polls = %d", expected, got.IdlePolls)
		}
		current = *got
	}

	emission := Emission{ID: "wev_1", Kind: EventMatch, Summary: "ERROR", Trust: TrustUnscreened, Cursor: 9}
	if err := store.CommitPoll(ctx, current, []Emission{emission}, 9, false, StateActive); err != nil {
		t.Fatalf("CommitPoll: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.IdlePolls != 0 {
		t.Fatalf("idle_polls = %d after an event; a source that just spoke is worth listening to closely", got.IdlePolls)
	}
}

// A failing source must never silently cancel a person's watch. Only the
// member, the clock, and the source itself end one.
func TestRecordFailureBacksOffWithoutTerminating(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}

	current := *created
	for i := 0; i < MaxFailuresBeforeBackoff+2; i++ {
		if err := store.RecordFailure(ctx, current); err != nil {
			t.Fatalf("RecordFailure %d: %v", i, err)
		}
		got, err := store.Get(ctx, "org-1", "w1")
		if err != nil {
			t.Fatalf("Get: %v", err)
		}
		if got.State != StateActive {
			t.Fatalf("a watch terminated after %d failures; a transient outage is not a cancellation", i+1)
		}
		current = *got
	}
	if current.ConsecutiveFailures < MaxFailuresBeforeBackoff {
		t.Fatalf("consecutive_failures = %d, want at least %d", current.ConsecutiveFailures, MaxFailuresBeforeBackoff)
	}
}

// A terminal source is recorded and stopped in the SAME transaction as its last
// event. A separate call would leave a window where the watch polls a source it
// has already reported as gone.
func TestCommitPollAppliesATerminalStateWithItsFinalEvent(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	created, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerContinuous))
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	final := Emission{ID: "wev_1", Kind: EventSourceGone, Summary: "exited with code 2", Trust: TrustOwnerMetadata, Cursor: 7}
	if err := store.CommitPoll(ctx, *created, []Emission{final}, 7, false, StateSourceGone); err != nil {
		t.Fatalf("CommitPoll: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateSourceGone {
		t.Fatalf("state = %s, want SOURCE_GONE", got.State)
	}
	if got.CursorValue != 7 {
		t.Fatalf("the final cursor was not committed: %d", got.CursorValue)
	}
}

func TestGetIsScopedToTheOrganization(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	if _, err := store.Get(ctx, "org-2", "w1"); !errors.Is(err, ErrWatchNotFound) {
		t.Fatalf("another organization resolved the watch: %v", err)
	}
}
