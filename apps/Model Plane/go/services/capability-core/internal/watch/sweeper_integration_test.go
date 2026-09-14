//go:build integration

package watch

import (
	"context"
	"errors"
	"testing"
	"time"
)

// The sweeper's end-to-end behaviour, against a real store.
//
// decide() is pure and covered without a database, but the loop AROUND it —
// claim, authorize, poll, commit, terminate — touches the store on every
// branch, and the branches that matter most are the ones that decide whether a
// watch observes anything at all.
//
// Run with: go test -tags integration ./internal/watch/

type scriptedAdapter struct {
	kind   string
	result PollResult
	err    error
	calls  int
}

func (s *scriptedAdapter) Kind() string { return s.kind }

func (s *scriptedAdapter) Poll(context.Context, Watch) (PollResult, error) {
	s.calls++
	if s.err != nil {
		return PollResult{}, s.err
	}
	return s.result, nil
}

type allowAll struct{ calls int }

func (a *allowAll) AuthorizeObserve(context.Context, Watch) error { a.calls++; return nil }

type denyAll struct{}

func (denyAll) AuthorizeObserve(context.Context, Watch) error {
	return errors.New("membership was revoked")
}

func sweeperOver(t *testing.T, store *Store, adapter SourceAdapter, authz Reauthorizer) *Sweeper {
	t.Helper()
	set, err := NewAdapterSet(adapter)
	if err != nil {
		t.Fatalf("NewAdapterSet: %v", err)
	}
	sweeper, err := NewSweeper(store, set, authz)
	if err != nil {
		t.Fatalf("NewSweeper: %v", err)
	}
	n := 0
	sweeper.newID = func() string {
		n++
		return "wev_test_" + time.Now().UTC().Format("150405.000000") + "_" + string(rune('a'+n%26))
	}
	return sweeper
}

// A nil reauthorizer must REFUSE, not skip. A nil check that falls through to
// "observe anyway" is the kind of default that ships and is then forgotten, and
// what it defaults past is the check that stops a watch created weeks ago under
// a membership since revoked.
func TestASweeperWithNoReauthorizerNeverObserves(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{
		kind:   SourceKindProcessOutput,
		result: PollResult{Cursor: 3, Lines: []Line{{Cursor: 3, Text: "anything"}}},
	}

	if _, err := sweeperOver(t, store, adapter, nil).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if adapter.calls != 0 {
		t.Fatal("a watch was polled with no reauthorizer configured")
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateActive {
		t.Fatalf("state = %s; a missing reauthorizer is a deployment gap, not a statement about the watch", got.State)
	}
	if got.ConsecutiveFailures == 0 {
		t.Fatal("the watch was not backed off")
	}
}

// Authority BEFORE observation, and a refusal ends the watch rather than
// leaving it to retry — a poller quietly re-probing a resource it was refused
// is not a thing to ship.
func TestARevokedAuthorityEndsTheWatchWithoutObserving(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{kind: SourceKindProcessOutput}

	if _, err := sweeperOver(t, store, adapter, denyAll{}).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if adapter.calls != 0 {
		t.Fatal("the source was read after authority was refused")
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateSourceGone {
		t.Fatalf("state = %s, want SOURCE_GONE", got.State)
	}
}

func TestAMatchingPollEmitsAndCommits(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{
		kind: SourceKindProcessOutput,
		result: PollResult{
			Cursor: 5,
			Lines: []Line{
				{Cursor: 4, Stream: StreamStdout, Text: "compiling"},
				{Cursor: 5, Stream: StreamStderr, Text: "ERROR: undefined symbol"},
			},
		},
	}
	authz := &allowAll{}

	if _, err := sweeperOver(t, store, adapter, authz).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if authz.calls != 1 {
		t.Fatalf("authority was checked %d times, want once per poll", authz.calls)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateTriggered {
		t.Fatalf("state = %s, want TRIGGERED for a `once` watch that matched", got.State)
	}
	if got.CursorValue != 5 {
		t.Fatalf("cursor = %d, want 5", got.CursorValue)
	}
	if got.LastEventSummary != "ERROR: undefined symbol" {
		t.Fatalf("last event = %q", got.LastEventSummary)
	}

	var kind, trust string
	if err := pool.QueryRow(ctx,
		`SELECT kind, trust FROM space_watch_events WHERE watch_id = 'w1'`).Scan(&kind, &trust); err != nil {
		t.Fatalf("read the event: %v", err)
	}
	if kind != EventMatch || trust != TrustUnscreened {
		t.Fatalf("event = %s/%s; a match is derived from watched content and is never owner metadata", kind, trust)
	}
}

// ErrSourceGone is the only adapter error that ends a watch.
func TestASourceGoneErrorTerminatesTheWatch(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{kind: SourceKindProcessOutput, err: ErrSourceGone}

	if _, err := sweeperOver(t, store, adapter, &allowAll{}).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateSourceGone {
		t.Fatalf("state = %s, want SOURCE_GONE", got.State)
	}
}

// Everything else is transient. Terminating on an unreachable service would
// silently cancel a person's watch over an outage.
func TestATransientAdapterErrorOnlyBacksOff(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{kind: SourceKindProcessOutput, err: errors.New("sandbox-manager is restarting")}

	if _, err := sweeperOver(t, store, adapter, &allowAll{}).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateActive {
		t.Fatalf("state = %s; an outage is not a cancellation", got.State)
	}
	if got.ConsecutiveFailures != 1 {
		t.Fatalf("consecutive_failures = %d, want 1", got.ConsecutiveFailures)
	}
}

// A binary older than a watch's source_kind is a deployment-ordering question,
// and ending a person's watch is the wrong answer to it.
func TestAWatchWithNoAdapterIsBackedOffNotTerminated(t *testing.T) {
	store, _ := setupWatchStore(t)
	ctx := context.Background()
	unknown := newWatch("w1", Predicate{Kind: PredicateAny}, TriggerOnce)
	unknown.SourceKind = "ingestion_job"
	if _, err := store.Create(ctx, unknown); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{kind: SourceKindProcessOutput}

	if _, err := sweeperOver(t, store, adapter, &allowAll{}).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if adapter.calls != 0 {
		t.Fatal("an adapter served a kind it does not declare")
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateActive {
		t.Fatalf("state = %s, want ACTIVE — the adapter may be deployed later", got.State)
	}
}

// The drain-then-terminate ordering, end to end: the final match and the
// terminal event land together and the watch stops.
func TestADrainedSourceRecordsItsLastMatchAndStops(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("w1", Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	adapter := &scriptedAdapter{
		kind: SourceKindProcessOutput,
		result: PollResult{
			Cursor:          9,
			Lines:           []Line{{Cursor: 9, Stream: StreamStderr, Text: "ERROR: could not link"}},
			SourceTerminal:  true,
			TerminalSummary: "the process exited with code 2",
		},
	}

	if _, err := sweeperOver(t, store, adapter, &allowAll{}).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	got, err := store.Get(ctx, "org-1", "w1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateSourceGone {
		t.Fatalf("state = %s, want SOURCE_GONE", got.State)
	}
	var events int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM space_watch_events WHERE watch_id = 'w1'`).Scan(&events); err != nil {
		t.Fatalf("count events: %v", err)
	}
	if events != 2 {
		// The last lines are exactly where a failing build says why it failed.
		t.Fatalf("recorded %d events, want the final match AND the terminal event", events)
	}
}
