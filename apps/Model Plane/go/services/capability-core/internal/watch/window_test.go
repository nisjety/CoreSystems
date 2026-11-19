package watch

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"
	"time"
)

// D18: a member's history window on what a watch may record.
//
// The window is read from the CURRENT Control observe decision, so a window that moved after the watch was
// created is the one applied, and everything it hides is hidden before anything is recorded.

func at(t *testing.T, rfc3339 string) time.Time {
	t.Helper()
	parsed, err := time.Parse(time.RFC3339Nano, rfc3339)
	if err != nil {
		t.Fatalf("parse %q: %v", rfc3339, err)
	}
	return parsed.UTC()
}

func floorGrant(t *testing.T, rfc3339 string) ObserveGrant {
	t.Helper()
	floor := at(t, rfc3339)
	return ObserveGrant{HistoryFloorAt: &floor}
}

func observe(t *testing.T, status int, body string) (ObserveGrant, error) {
	t.Helper()
	server := controlStub(t, status, body)
	return testObserveAuthorizer(t, server.URL).AuthorizeObserve(context.Background(),
		activeWatch(Predicate{Kind: "contains", Value: "ERROR"}, "every", 3))
}

func TestTheObserveGrantReadsTheMembersWindowOffTheSignedDecision(t *testing.T) {
	t.Parallel()
	t.Run("a since_date window is an instant", func(t *testing.T) {
		t.Parallel()
		grant, err := observe(t, http.StatusOK, `{"data":{"decision":{"history_floor_at":"2026-11-18T09:30:00.123456789Z"},"token":"v2.x.y.z"}}`)
		if err != nil {
			t.Fatalf("a windowed decision was refused: %v", err)
		}
		if grant.HistoryFloorAt == nil || !grant.HistoryFloorAt.Equal(at(t, "2026-11-18T09:30:00.123456789Z")) {
			t.Fatalf("floor = %v, want the signed instant", grant.HistoryFloorAt)
		}
		if grant.HistoryFloorAt.Location() != time.UTC {
			t.Fatalf("the instant is kept in %v, want UTC", grant.HistoryFloorAt.Location())
		}
		if !grant.Windowed() {
			t.Fatal("a grant with an instant is a window")
		}
	})
	t.Run("a since_join member window carries the revision beside the instant", func(t *testing.T) {
		t.Parallel()
		grant, err := observe(t, http.StatusOK, `{"data":{"decision":{"history_floor_at":"2026-11-18T09:30:00Z","history_floor_revision":5}}}`)
		if err != nil {
			t.Fatalf("a windowed decision was refused: %v", err)
		}
		if grant.HistoryFloorAt == nil || grant.HistoryFloorRevision != 5 {
			t.Fatalf("grant = %+v, want the instant and revision 5", grant)
		}
	})
	t.Run("a since_join room window is a revision alone", func(t *testing.T) {
		t.Parallel()
		grant, err := observe(t, http.StatusOK, `{"data":{"decision":{"history_floor_revision":5}}}`)
		if err != nil {
			t.Fatalf("a windowed decision was refused: %v", err)
		}
		if grant.HistoryFloorAt != nil || grant.HistoryFloorRevision != 5 || !grant.Windowed() {
			t.Fatalf("grant = %+v, want revision 5 and no instant", grant)
		}
	})
	t.Run("no window claim is no window", func(t *testing.T) {
		t.Parallel()
		for _, body := range []string{
			`{"data":{"decision":{"decision_ref":"d-1"}}}`,
			`{"data":{"decision":{"history_floor_at":null,"history_floor_revision":0}}}`,
			`{"data":{}}`,
		} {
			grant, err := observe(t, http.StatusOK, body)
			if err != nil {
				t.Fatalf("%s: %v", body, err)
			}
			if grant.Windowed() {
				t.Fatalf("%s: grant = %+v, want no window", body, grant)
			}
		}
	})
}

// An answer whose window cannot be read is not an approval. Reading it as no window would show a member the
// whole room, so it is Control being unable to answer: the watch backs off and the cursor is not committed.
func TestAnUnreadableWindowIsNotAnApproval(t *testing.T) {
	t.Parallel()
	for name, body := range map[string]string{
		"an instant that is text":     `{"data":{"decision":{"history_floor_at":"last tuesday"}}}`,
		"an instant that is a number": `{"data":{"decision":{"history_floor_at":1700000000}}}`,
		"the zero instant":            `{"data":{"decision":{"history_floor_at":"0001-01-01T00:00:00Z"}}}`,
		"the Unix epoch":              `{"data":{"decision":{"history_floor_at":"1970-01-01T00:00:00Z"}}}`,
		"a negative revision":         `{"data":{"decision":{"history_floor_revision":-1}}}`,
		"a revision that is text":     `{"data":{"decision":{"history_floor_revision":"5"}}}`,
		"a body that is not JSON":     `nope`,
		"an empty body":               ``,
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			grant, err := observe(t, http.StatusOK, body)
			if err == nil {
				t.Fatalf("an unreadable window was approved as %+v", grant)
			}
			if !errors.Is(err, ErrAuthorityUnavailable) {
				t.Fatalf("an unreadable window must defer the watch, not end it: %v", err)
			}
			if grant.Windowed() {
				t.Fatalf("a refused answer still carried a window: %+v", grant)
			}
		})
	}
}

func TestAGrantWithNoWindowNarrowsNothing(t *testing.T) {
	t.Parallel()
	if (ObserveGrant{}).Windowed() {
		t.Fatal("the zero grant is a window")
	}
	result := PollResult{Cursor: 3, GapBefore: true, SourceTerminal: true, Lines: []Line{{Cursor: 3, Text: "x"}}}
	// The sweeper does not narrow an unwindowed grant at all; this is the property of the function if it did.
	narrowed := (ObserveGrant{}).narrow(result)
	if len(narrowed.Lines) != 1 || narrowed.terminalHidden || !narrowed.GapBefore || !narrowed.SourceTerminal {
		t.Fatalf("an unwindowed grant hid something: %+v", narrowed)
	}
}

func TestATimestampWindowShowsOnlyLinesCapturedInsideIt(t *testing.T) {
	t.Parallel()
	floor := at(t, "2026-11-18T09:30:00Z")
	grant := floorGrant(t, "2026-11-18T09:30:00Z")
	result := PollResult{
		Cursor: 6,
		Lines: []Line{
			{Cursor: 1, Text: "before", CapturedAt: floor.Add(-time.Microsecond)},
			{Cursor: 2, Text: "no instant"},
			{Cursor: 3, Text: "at the floor", CapturedAt: floor},
			{Cursor: 4, Text: "after", CapturedAt: floor.Add(time.Hour)},
		},
	}
	shown := grant.narrow(result).Lines
	if len(shown) != 2 || shown[0].Text != "at the floor" || shown[1].Text != "after" {
		t.Fatalf("shown = %+v, want only the lines captured at or after the floor", shown)
	}
	if len(result.Lines) != 4 {
		t.Fatal("narrowing changed the poll it was given")
	}
	// The cursor is the poll's: the watch moves past what was hidden instead of meeting it again.
	if got := grant.narrow(result).Cursor; got != 6 {
		t.Fatalf("cursor = %d, want the poll's own 6", got)
	}
}

// Decided again over the lines the window shows, a `once` watch answers with the first match the member may
// see, not a match from before their window.
func TestAOnceWatchUnderAWindowAnswersWithTheFirstMatchTheMemberMaySee(t *testing.T) {
	t.Parallel()
	floor := at(t, "2026-11-18T09:30:00Z")
	grant := floorGrant(t, "2026-11-18T09:30:00Z")
	w := activeWatch(Predicate{Kind: PredicateContains, Value: "ERROR"}, TriggerOnce, 0)
	result := PollResult{
		Cursor: 5,
		Lines: []Line{
			{Cursor: 1, Stream: StreamStderr, Text: "ERROR: from before the window", CapturedAt: floor.Add(-time.Hour)},
			{Cursor: 2, Stream: StreamStdout, Text: "fine", CapturedAt: floor.Add(time.Minute)},
			{Cursor: 3, Stream: StreamStderr, Text: "ERROR: inside the window", CapturedAt: floor.Add(2 * time.Minute)},
			{Cursor: 4, Stream: StreamStderr, Text: "ERROR: a second one", CapturedAt: floor.Add(3 * time.Minute)},
		},
	}
	sweeper := testSweeper()
	candidates, _ := sweeper.decide(w, result)
	if len(candidates) != 1 || candidates[0].Summary != "ERROR: from before the window" {
		t.Fatalf("the unwindowed decision answered %+v; the premise of the test is the pre-floor match", candidates)
	}
	events, cursor := sweeper.decide(w, grant.narrow(result))
	if len(events) != 1 || events[0].Summary != "ERROR: inside the window" || events[0].Cursor != 3 {
		t.Fatalf("events = %+v, want the first match inside the window", events)
	}
	if cursor != 5 {
		t.Fatalf("cursor = %d, want the poll's own 5", cursor)
	}
	// When nothing inside the window matches, nothing is recorded and the cursor still moves.
	quiet := PollResult{Cursor: 2, Lines: result.Lines[:1]}
	events, cursor = sweeper.decide(w, grant.narrow(quiet))
	if len(events) != 0 || cursor != 2 {
		t.Fatalf("events = %+v cursor = %d; a hidden match records nothing and the watch moves on", events, cursor)
	}
}

func TestATimestampWindowWithholdsAGapItCannotPlace(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateAny}, TriggerContinuous, 0)
	floor := at(t, "2026-11-18T09:30:00Z")
	result := PollResult{
		Cursor: 9, GapBefore: true,
		Lines: []Line{{Cursor: 9, Text: "later", CapturedAt: floor.Add(time.Minute)}},
	}
	events, _ := testSweeper().decide(w, floorGrant(t, "2026-11-18T09:30:00Z").narrow(result))
	for _, event := range events {
		if event.Kind == EventGap {
			t.Fatal("a gap was told to a member whose window cannot place the trimmed output")
		}
	}
	if len(events) != 1 || events[0].Kind != EventMatch {
		t.Fatalf("events = %+v, want only the line inside the window", events)
	}
	// Without a window the same poll still reports its gap, first.
	events, _ = testSweeper().decide(w, result)
	if len(events) != 2 || events[0].Kind != EventGap {
		t.Fatalf("events = %+v, want the gap and the line", events)
	}
}

func TestTheEndOfASourceIsToldOnlyWhenItEndedInsideTheWindow(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateAny}, TriggerContinuous, 0)
	floor := at(t, "2026-11-18T09:30:00Z")
	grant := floorGrant(t, "2026-11-18T09:30:00Z")
	ended := func(at time.Time) PollResult {
		return PollResult{Cursor: 4, SourceTerminal: true, TerminalSummary: "the process finished successfully", EndedAt: at}
	}
	hasGone := func(events []Emission) bool {
		for _, event := range events {
			if event.Kind == EventSourceGone {
				return true
			}
		}
		return false
	}
	for name, tc := range map[string]struct {
		endedAt time.Time
		told    bool
	}{
		"ended before the window": {floor.Add(-time.Minute), false},
		"ended with no instant":   {time.Time{}, false},
		"ended at the floor":      {floor, true},
		"ended after the window":  {floor.Add(time.Hour), true},
	} {
		events, _ := testSweeper().decide(w, grant.narrow(ended(tc.endedAt)))
		if hasGone(events) != tc.told {
			t.Fatalf("%s: source_gone told = %t, want %t (%+v)", name, hasGone(events), tc.told, events)
		}
	}
	// What is withheld is the telling, never the state: the poll is still terminal, so the watch still ends.
	narrowed := grant.narrow(ended(floor.Add(-time.Minute)))
	if !narrowed.SourceTerminal {
		t.Fatal("narrowing a terminal source made it look alive; the watch would poll a finished source forever")
	}
}

func TestARevisionWindowHidesASourceRegisteredBeforeIt(t *testing.T) {
	t.Parallel()
	w := activeWatch(Predicate{Kind: PredicateAny}, TriggerContinuous, 0)
	grant := ObserveGrant{HistoryFloorRevision: 5}
	poll := func(revision int64) PollResult {
		return PollResult{
			Cursor: 2, GapBefore: true, SourceTerminal: true, SourceRevision: revision,
			Lines: []Line{{Cursor: 2, Text: "line"}},
		}
	}
	// A source registered under an older audience belongs to the room's past: nothing of it is told, not its
	// lines, not its gap and not its end. The state still ends the watch.
	events, _ := testSweeper().decide(w, grant.narrow(poll(4)))
	if len(events) != 0 {
		t.Fatalf("events = %+v; a source before the window shows nothing", events)
	}
	if !grant.narrow(poll(4)).SourceTerminal {
		t.Fatal("hiding the source's end must not make the watch keep polling it")
	}
	// A source with no stamp cannot be placed, so the window hides it (a ceiling on the same stamp would not).
	events, _ = testSweeper().decide(w, grant.narrow(poll(0)))
	if len(events) != 0 {
		t.Fatalf("events = %+v; an unstamped source cannot be shown under a revision window", events)
	}
	// A source registered at or after the window is wholly inside it, with its gap and its end.
	for _, revision := range []int64{5, 6} {
		events, _ = testSweeper().decide(w, grant.narrow(poll(revision)))
		kinds := make([]string, 0, len(events))
		for _, event := range events {
			kinds = append(kinds, event.Kind)
		}
		if strings.Join(kinds, ",") != "gap,match,source_gone" {
			t.Fatalf("revision %d: events = %v, want the gap, the line and the end", revision, kinds)
		}
	}
}

// A member's join window carries both: a source must clear every window that is set.
func TestBothWindowsMustAdmit(t *testing.T) {
	t.Parallel()
	floor := at(t, "2026-11-18T09:30:00Z")
	instant := floor
	grant := ObserveGrant{HistoryFloorAt: &instant, HistoryFloorRevision: 5}
	line := Line{Cursor: 1, Text: "x", CapturedAt: floor.Add(time.Minute)}
	for name, tc := range map[string]struct {
		revision int64
		line     Line
		shown    bool
	}{
		"both clear":         {5, line, true},
		"the revision fails": {4, line, false},
		"the instant fails":  {5, Line{Cursor: 1, Text: "x", CapturedAt: floor.Add(-time.Minute)}, false},
		"neither clears":     {4, Line{Cursor: 1, Text: "x"}, false},
	} {
		got := grant.narrow(PollResult{Cursor: 1, SourceRevision: tc.revision, Lines: []Line{tc.line}})
		if (len(got.Lines) == 1) != tc.shown {
			t.Fatalf("%s: shown = %t, want %t", name, len(got.Lines) == 1, tc.shown)
		}
	}
}
