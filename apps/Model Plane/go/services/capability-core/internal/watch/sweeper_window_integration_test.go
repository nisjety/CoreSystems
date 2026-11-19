//go:build integration

package watch

import (
	"context"
	"testing"
	"time"
)

// D18: the sweeper against a real store, under a member's history window.
//
// The window comes from the fresh Control observe decision, so what is hidden is hidden BEFORE anything is
// recorded, and the cursor still moves past it.
//
// Run with: go test -tags integration ./internal/watch/

func windowFloor() time.Time { return time.Date(2026, 11, 18, 9, 30, 0, 0, time.UTC) }

// A `once` watch answers with the first match the member may see. A match from before their window is not
// recorded, and is not what the watch is spent on.
func TestAWindowedWatchRecordsTheFirstMatchInsideTheWindowAndNothingBefore(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("ww1", Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	floor := windowFloor()
	adapter := &scriptedAdapter{
		kind: SourceKindProcessOutput,
		result: PollResult{
			Cursor: 5,
			Lines: []Line{
				{Cursor: 3, Stream: StreamStderr, Text: "ERROR: before the window", CapturedAt: floor.Add(-time.Hour)},
				{Cursor: 4, Stream: StreamStdout, Text: "fine", CapturedAt: floor.Add(time.Minute)},
				{Cursor: 5, Stream: StreamStderr, Text: "ERROR: inside the window", CapturedAt: floor.Add(2 * time.Minute)},
			},
		},
	}
	authority := windowedAuthority{grant: ObserveGrant{HistoryFloorAt: &floor}}

	if _, err := sweeperOver(t, store, adapter, authority).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	rows, err := pool.Query(ctx, `SELECT cursor_value, summary FROM space_watch_events WHERE watch_id = 'ww1' ORDER BY cursor_value`)
	if err != nil {
		t.Fatalf("read events: %v", err)
	}
	defer rows.Close()
	var recorded []string
	for rows.Next() {
		var cursor int64
		var summary string
		if err := rows.Scan(&cursor, &summary); err != nil {
			t.Fatalf("scan: %v", err)
		}
		recorded = append(recorded, summary)
	}
	if len(recorded) != 1 || recorded[0] != "ERROR: inside the window" {
		t.Fatalf("recorded %v, want only the match inside the window", recorded)
	}
	got, err := store.Get(ctx, "org-1", "ww1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateTriggered || got.LastEventSummary != "ERROR: inside the window" {
		t.Fatalf("state = %s, last event = %q; the watch answers with the match the member may see", got.State, got.LastEventSummary)
	}
}

// When everything the poll found is before the window, nothing is recorded, the watch is not spent, and its
// cursor still moves so the hidden lines are not met again.
func TestAWindowedWatchThatOnlyFoundHiddenLinesRecordsNothingAndMovesOn(t *testing.T) {
	store, pool := setupWatchStore(t)
	ctx := context.Background()
	if _, err := store.Create(ctx, newWatch("ww2", Predicate{Kind: PredicateAny}, TriggerOnce)); err != nil {
		t.Fatalf("Create: %v", err)
	}
	floor := windowFloor()
	adapter := &scriptedAdapter{
		kind: SourceKindProcessOutput,
		result: PollResult{
			Cursor: 7, GapBefore: true,
			Lines: []Line{
				{Cursor: 6, Text: "printed before", CapturedAt: floor.Add(-time.Hour)},
				{Cursor: 7, Text: "no instant"},
			},
		},
	}
	authority := windowedAuthority{grant: ObserveGrant{HistoryFloorAt: &floor}}

	if _, err := sweeperOver(t, store, adapter, authority).RunOnce(ctx); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	var events int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM space_watch_events WHERE watch_id = 'ww2'`).Scan(&events); err != nil {
		t.Fatalf("count events: %v", err)
	}
	if events != 0 {
		t.Fatalf("%d events were recorded for output the window hides, gap notices included", events)
	}
	got, err := store.Get(ctx, "org-1", "ww2")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateActive {
		t.Fatalf("state = %s; a watch that was shown nothing has not answered", got.State)
	}
	if got.CursorValue != 7 {
		t.Fatalf("cursor = %d, want 7: the watch moves past what it hid", got.CursorValue)
	}
}
