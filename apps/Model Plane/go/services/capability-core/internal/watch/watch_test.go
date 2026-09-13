package watch

import (
	"strings"
	"testing"
	"time"
)

func validWatch() Watch {
	return Watch{
		ID:               "wch_1",
		OrgID:            "org-1",
		SpaceRef:         "space-1",
		CreatorSubjectID: "user-1",
		SourceKind:       SourceKindProcessOutput,
		SourceRef:        "proc-1",
		Predicate:        Predicate{Kind: PredicateContains, Value: "ERROR"},
		State:            StateActive,
		TriggerMode:      TriggerOnce,
		ExpiresAt:        time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC),
	}
}

// Only ACTIVE polls, and every other state is final. A watch is never
// resurrected: the authority that justified the first one is not the authority
// that justifies the second, so re-watching has to be a new row that carries a
// new decision.
func TestOnlyActiveIsNonTerminal(t *testing.T) {
	t.Parallel()
	if StateActive.Terminal() {
		t.Fatal("ACTIVE must be the polling state")
	}
	for _, state := range []State{StateTriggered, StateExpired, StateCancelled, StateSourceGone} {
		if !state.Terminal() {
			t.Fatalf("%s must be terminal", state)
		}
	}
}

// SOURCE_GONE is not a synonym for EXPIRED, and the names have to survive
// refactoring because a person reading a list needs to tell "the thing you were
// watching finished and you saw all of it" from "we stopped looking".
func TestSourceGoneAndExpiredStayDistinct(t *testing.T) {
	t.Parallel()
	if StateSourceGone == StateExpired {
		t.Fatal("a drained source and an abandoned watch are different answers")
	}
	if StateSourceGone.String() != "SOURCE_GONE" || StateExpired.String() != "EXPIRED" {
		t.Fatalf("state names drifted: %s / %s", StateSourceGone, StateExpired)
	}
}

func TestValidateRejectsAnIncompleteWatch(t *testing.T) {
	t.Parallel()
	for name, mutate := range map[string]func(*Watch){
		"no id":       func(w *Watch) { w.ID = "" },
		"no org":      func(w *Watch) { w.OrgID = "" },
		"no space":    func(w *Watch) { w.SpaceRef = " " },
		"no subject":  func(w *Watch) { w.CreatorSubjectID = "" },
		"no source":   func(w *Watch) { w.SourceRef = "" },
		"no kind":     func(w *Watch) { w.SourceKind = "" },
		"bad trigger": func(w *Watch) { w.TriggerMode = "forever" },
		"bad cursor":  func(w *Watch) { w.CursorValue = -1 },
	} {
		w := validWatch()
		mutate(&w)
		if err := w.Validate(); err == nil {
			t.Fatalf("a watch with %s validated", name)
		}
	}
}

// No unbounded watches. A standing intent with no end is a standing cost, and
// the person who set it has long stopped expecting it.
func TestValidateRequiresAnExpiry(t *testing.T) {
	t.Parallel()
	w := validWatch()
	w.ExpiresAt = time.Time{}
	if err := w.Validate(); err == nil {
		t.Fatal("a watch with no expiry validated")
	}
}

// Creating an already-terminal watch is a row nothing will look at again.
func TestValidateRefusesAWatchBornTerminal(t *testing.T) {
	t.Parallel()
	for _, state := range []State{StateTriggered, StateExpired, StateCancelled, StateSourceGone} {
		w := validWatch()
		w.State = state
		if err := w.Validate(); err == nil {
			t.Fatalf("a watch created %s validated", state)
		}
	}
}

// The backoff is derived, so the only thing to prove is that it is bounded on
// both ends and never overflows into something absurd.
func TestPollIntervalIsBoundedOnBothEnds(t *testing.T) {
	t.Parallel()
	if got := PollInterval(0); got != BasePollInterval {
		t.Fatalf("idle 0 = %s, want the base interval", got)
	}
	if got := PollInterval(-3); got != BasePollInterval {
		t.Fatalf("a negative idle level = %s, want the base interval", got)
	}
	if got := PollInterval(1); got != 2*BasePollInterval {
		t.Fatalf("idle 1 = %s, want double the base", got)
	}
	for _, idle := range []int{8, 16, 64, 1 << 20} {
		if got := PollInterval(idle); got != MaxPollInterval {
			t.Fatalf("idle %d = %s, want the ceiling %s — an overflow here would make a watch poll instantly or never", idle, got, MaxPollInterval)
		}
	}
	// Monotonic up to the ceiling: a backoff that went down would make a quiet
	// source cost more over time, not less.
	previous := time.Duration(0)
	for idle := 0; idle < 8; idle++ {
		current := PollInterval(idle)
		if current < previous {
			t.Fatalf("interval decreased at idle %d: %s after %s", idle, current, previous)
		}
		previous = current
	}
}

func TestBoundSummaryMarksTruncationAndKeepsRunesIntact(t *testing.T) {
	t.Parallel()
	if got := BoundSummary("build failed\n"); got != "build failed" {
		t.Fatalf("trailing newline survived: %q", got)
	}
	long := strings.Repeat("a", MaxSummaryBytes*2)
	bounded := BoundSummary(long)
	if len(bounded) > MaxSummaryBytes {
		t.Fatalf("summary is %d bytes, over the column's %d", len(bounded), MaxSummaryBytes)
	}
	if !strings.HasSuffix(bounded, "…") {
		// A silently shortened line reads as a complete one, and a person
		// deciding whether a build failed on the strength of a summary should
		// be able to see there was more.
		t.Fatalf("truncation was not marked: %q", bounded)
	}
	// A multi-byte rune must not be cut in half — the result is stored in a
	// TEXT column and rendered in a browser.
	multibyte := strings.Repeat("é", MaxSummaryBytes)
	if !isValidUTF8(BoundSummary(multibyte)) {
		t.Fatal("truncation split a multi-byte rune")
	}
}

func isValidUTF8(s string) bool {
	for _, r := range s {
		if r == '�' {
			return false
		}
	}
	return true
}
