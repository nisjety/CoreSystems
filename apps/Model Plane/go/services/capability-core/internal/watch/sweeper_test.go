package watch

import (
	"fmt"
	"strings"
	"testing"
)

// A sweeper whose emission ids are predictable, so tests can assert on shape
// rather than on a UUID.
func testSweeper() *Sweeper {
	n := 0
	return &Sweeper{
		newID: func() string {
			n++
			return fmt.Sprintf("wev_%d", n)
		},
	}
}

func activeWatch(p Predicate, mode string, cursor int64) Watch {
	return Watch{
		ID: "wch_1", OrgID: "org-1", SpaceRef: "space-1",
		SourceKind: SourceKindProcessOutput, SourceRef: "proc-1",
		Predicate: p, TriggerMode: mode, State: StateActive, CursorValue: cursor,
	}
}

func TestDecideEmitsOneMatchPerMatchingLine(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous, 0)
	events, cursor := testSweeper().decide(w, PollResult{
		Cursor: 4,
		Lines: []Line{
			{Cursor: 1, Stream: StreamStdout, Text: "compiling"},
			{Cursor: 2, Stream: StreamStderr, Text: "ERROR: undefined symbol"},
			{Cursor: 3, Stream: StreamStdout, Text: "still going"},
			{Cursor: 4, Stream: StreamStderr, Text: "ERROR: and again"},
		},
	})
	if len(events) != 2 {
		t.Fatalf("emitted %d events, want 2 matches", len(events))
	}
	if events[0].Cursor != 2 || events[1].Cursor != 4 {
		// The event is attached to the line's OWN cursor, which is what makes
		// it idempotent under the (watch, cursor, kind) unique index.
		t.Fatalf("events carry cursors %d/%d, want the matching lines' 2/4", events[0].Cursor, events[1].Cursor)
	}
	if cursor != 4 {
		t.Fatalf("committed cursor = %d, want the poll's own 4", cursor)
	}
	for _, event := range events {
		if event.Trust != TrustUnscreened {
			t.Fatalf("a match was labelled %q; content a program emitted is never owner metadata", event.Trust)
		}
	}
}

// A `once` watch answers once. Continuing to scan would record matches nobody
// asked for and move the cursor past content a re-watch would never see.
func TestDecideStopsAtTheFirstMatchForAOnceWatch(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateAny}, TriggerOnce, 0)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor: 3,
		Lines: []Line{
			{Cursor: 1, Text: "one"},
			{Cursor: 2, Text: "two"},
			{Cursor: 3, Text: "three"},
		},
	})
	if len(events) != 1 {
		t.Fatalf("a once watch emitted %d events", len(events))
	}
	if events[0].Cursor != 1 {
		t.Fatalf("the once watch answered at cursor %d, want the first match", events[0].Cursor)
	}
}

// Output that was produced and then trimmed is not the same as output that
// never existed, and a person summarising a log has to be told which they are
// looking at.
func TestDecideReportsAGapAtTheWatchesOwnCursor(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous, 7)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor:    40,
		GapBefore: true,
		Lines:     []Line{{Cursor: 40, Text: "ERROR: late"}},
	})
	if len(events) != 2 || events[0].Kind != EventGap {
		t.Fatalf("expected a gap first, got %+v", events)
	}
	if events[0].Cursor != 7 {
		// Attached to where the hole actually was, not to whatever happened to
		// be read next.
		t.Fatalf("the gap was recorded at cursor %d, want the watch's own 7", events[0].Cursor)
	}
	if events[0].Trust != TrustOwnerMetadata {
		t.Fatal("a gap is the registry's own assertion, not payload")
	}
	if !strings.Contains(events[0].Summary, "retention") {
		t.Fatalf("the gap summary does not say what happened: %q", events[0].Summary)
	}
}

// A gap fires even when nothing matched afterwards: the person still needs to
// know their watch has a hole in it.
func TestDecideReportsAGapWithNoMatches(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce, 2)
	events, cursor := testSweeper().decide(w, PollResult{Cursor: 9, GapBefore: true})
	if len(events) != 1 || events[0].Kind != EventGap {
		t.Fatalf("expected exactly a gap, got %+v", events)
	}
	if cursor != 9 {
		t.Fatalf("cursor = %d, want 9", cursor)
	}
}

func TestDecideReportsATerminalSourceAsOwnerMetadata(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous, 0)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor:          5,
		Lines:           []Line{{Cursor: 5, Text: "ERROR: fatal"}},
		SourceTerminal:  true,
		TerminalSummary: "exited with code 2",
	})
	if len(events) != 2 {
		t.Fatalf("expected the match and the terminal event, got %+v", events)
	}
	last := events[len(events)-1]
	if last.Kind != EventSourceGone {
		t.Fatalf("last event is %q, want %q", last.Kind, EventSourceGone)
	}
	if last.Trust != TrustOwnerMetadata {
		t.Fatal("a terminal summary is the owning plane's assertion")
	}
	if last.Summary != "exited with code 2" {
		t.Fatalf("terminal summary = %q", last.Summary)
	}
}

// The last lines are exactly where a failing build says why it failed, so the
// match must survive the same poll that observes the source ending.
func TestDecideKeepsTheFinalMatchOnATerminalPoll(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce, 0)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor:         9,
		Lines:          []Line{{Cursor: 9, Text: "ERROR: could not link"}},
		SourceTerminal: true,
	})
	if !hasKind(events, EventMatch) {
		t.Fatal("the final match was dropped by the terminal transition")
	}
	if !hasKind(events, EventSourceGone) {
		t.Fatal("the terminal event was not recorded")
	}
}

// An adapter that reported a cursor behind the watch's own would re-emit
// everything between the two positions. decide clamps; CommitPoll refuses.
func TestDecideNeverMovesTheCursorBackwards(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateAny}, TriggerContinuous, 12)
	_, cursor := testSweeper().decide(w, PollResult{Cursor: 3})
	if cursor != 12 {
		t.Fatalf("cursor went backwards to %d from 12", cursor)
	}
}

func TestDecideEmitsNothingWhenNothingMatches(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce, 0)
	events, cursor := testSweeper().decide(w, PollResult{
		Cursor: 3,
		Lines: []Line{
			{Cursor: 1, Text: "compiling"},
			{Cursor: 2, Text: "linking"},
			{Cursor: 3, Text: "done"},
		},
	})
	if len(events) != 0 {
		t.Fatalf("emitted %d events for a non-matching poll", len(events))
	}
	// The cursor still advances: the watch consumed that output and must not
	// re-read it.
	if cursor != 3 {
		t.Fatalf("cursor = %d, want 3 — a non-matching poll still consumes", cursor)
	}
}

// The pairing rule, asserted directly: a match is derived from watched content
// by definition. Without this an adapter could launder a payload into the label
// a consumer renders as trustworthy — the entire failure the trust column
// exists to prevent.
func TestAMatchMayNotBeLabelledOwnerMetadata(t *testing.T) {
	t.Parallel()
	err := validateEmission(Emission{
		ID: "wev_1", Kind: EventMatch, Trust: TrustOwnerMetadata, Summary: "x",
	})
	if err == nil {
		t.Fatal("a match labelled as owner metadata was accepted")
	}
	if err := validateEmission(Emission{
		ID: "wev_1", Kind: EventMatch, Trust: TrustUnscreened, Summary: "x",
	}); err != nil {
		t.Fatalf("a correctly labelled match was refused: %v", err)
	}
}

func TestValidateEmissionClosesItsVocabularies(t *testing.T) {
	t.Parallel()
	for name, event := range map[string]Emission{
		"no id":         {Kind: EventMatch, Trust: TrustUnscreened},
		"unknown kind":  {ID: "wev_1", Kind: "notified", Trust: TrustOwnerMetadata},
		"unknown trust": {ID: "wev_1", Kind: EventGap, Trust: "verified"},
		"bad cursor":    {ID: "wev_1", Kind: EventGap, Trust: TrustOwnerMetadata, Cursor: -1},
	} {
		if err := validateEmission(event); err == nil {
			t.Fatalf("an emission with %s was accepted", name)
		}
	}
}

// A chunk is a flush of MANY lines, so several matching lines commonly share
// one cursor — and the event key is (watch_id, cursor_value, kind). Emitting
// one event per line would make the second collide with the first and be
// silently dropped by ON CONFLICT DO NOTHING, which loses a match rather than
// deduplicating a replay.
//
// This is the constraint step 2's adapter made concrete: it attributes every
// line to its chunk's seq, which is exactly when the collision becomes
// reachable.
func TestAtMostOneMatchEventPerCursorPosition(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous, 0)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor: 7,
		Lines: []Line{
			// Three matching lines, all from the same flush.
			{Cursor: 7, Text: "ERROR: first"},
			{Cursor: 7, Text: "ERROR: second"},
			{Cursor: 7, Text: "ERROR: third"},
		},
	})
	if len(events) != 1 {
		t.Fatalf("emitted %d events at one cursor; all but the first would be silently dropped by the unique index", len(events))
	}
	if events[0].Summary != "ERROR: first" {
		t.Fatalf("the event names %q, want the first matching line", events[0].Summary)
	}
}

// Distinct cursors are distinct events, so a continuous watch still reports
// every chunk that matched.
func TestMatchesAtDifferentCursorsAreSeparateEvents(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerContinuous, 0)
	events, _ := testSweeper().decide(w, PollResult{
		Cursor: 9,
		Lines: []Line{
			{Cursor: 7, Text: "ERROR: first flush"},
			{Cursor: 9, Text: "ERROR: second flush"},
		},
	})
	if len(events) != 2 {
		t.Fatalf("emitted %d events, want one per chunk", len(events))
	}
	if events[0].Cursor == events[1].Cursor {
		t.Fatal("two events share a cursor; the unique index would drop one")
	}
}
