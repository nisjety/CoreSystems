package watch

import (
	"context"
	"strings"
	"testing"
)

// The vocabulary is closed, and this is the test that keeps it closed. Adding a
// kind without adding it here and to the migration's CHECK is how an
// expression language arrives one commit at a time.
func TestPredicateVocabularyIsClosed(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"regex", "matches", "glob", "jq", "", "CONTAINS"} {
		p := Predicate{Kind: kind, Value: "x"}
		if err := p.Validate(); err == nil {
			t.Fatalf("predicate kind %q was accepted; the grammar is meant to be closed", kind)
		}
	}
}

// A regex predicate would put the attacker on both ends of a catastrophic
// backtrack once a model can create a watch: it supplies the pattern AND the
// subject, in a process that is polling every other watch in the fleet. This
// test exists to make that refusal explicit rather than incidental.
func TestPredicateRefusesAnythingRegexShaped(t *testing.T) {
	t.Parallel()
	p := Predicate{Kind: "regex", Value: "^(a+)+$"}
	if err := p.Validate(); err == nil {
		t.Fatal("a regex predicate validated")
	}
	// And a literal that merely LOOKS like a regex is fine — it is matched as
	// text, so there is nothing to backtrack.
	literal := Predicate{Kind: PredicateContains, Value: "^(a+)+$"}
	if err := literal.Validate(); err != nil {
		t.Fatalf("a literal containing regex characters was refused: %v", err)
	}
	if !literal.MatchLine(Line{Text: "saw ^(a+)+$ in the log"}) {
		t.Fatal("a literal predicate must match as text")
	}
	if literal.MatchLine(Line{Text: "aaaaaaaa"}) {
		t.Fatal("a literal predicate matched as a pattern")
	}
}

func TestPredicateShapeRules(t *testing.T) {
	t.Parallel()
	// `contains` needs a value.
	if err := (Predicate{Kind: PredicateContains}).Validate(); err == nil {
		t.Fatal("a contains predicate with no value validated")
	}
	if err := (Predicate{Kind: PredicateContains, Value: "   "}).Validate(); err == nil {
		t.Fatal("a contains predicate with a blank value validated")
	}
	// Nothing else may carry one: a value that is never read looks like a
	// filter that is applied.
	for _, kind := range []string{PredicateAny, PredicateStateChange} {
		if err := (Predicate{Kind: kind, Value: "ERROR"}).Validate(); err == nil {
			t.Fatalf("a %q predicate carrying a value validated", kind)
		}
	}
	// Bounded, matching the column's CHECK.
	long := Predicate{Kind: PredicateContains, Value: strings.Repeat("x", MaxPredicateValueBytes+1)}
	if err := long.Validate(); err == nil {
		t.Fatal("an over-long predicate value validated")
	}
	// A state change has no stream; accepting one would let a caller write a
	// selector that silently does nothing.
	if err := (Predicate{Kind: PredicateStateChange, Stream: StreamStderr}).Validate(); err == nil {
		t.Fatal("a state_change predicate with a stream validated")
	}
	if err := (Predicate{Kind: PredicateAny, Stream: "stdin"}).Validate(); err == nil {
		t.Fatal("an unrecognized stream validated")
	}
}

func TestPredicateStreamSelector(t *testing.T) {
	t.Parallel()
	stderrOnly := Predicate{Kind: PredicateAny, Stream: StreamStderr}
	if stderrOnly.MatchLine(Line{Stream: StreamStdout, Text: "anything"}) {
		t.Fatal("a stderr-only predicate matched stdout")
	}
	if !stderrOnly.MatchLine(Line{Stream: StreamStderr, Text: "anything"}) {
		t.Fatal("a stderr-only predicate missed stderr")
	}
	both := Predicate{Kind: PredicateAny}
	if !both.MatchLine(Line{Stream: StreamStdout}) || !both.MatchLine(Line{Stream: StreamStderr}) {
		t.Fatal("an unselected stream must match both")
	}
}

// A state change is owner metadata and a line is unscreened payload. If a
// state_change predicate could match a line, a program could emit text that
// produced an event a consumer renders as the owning plane's own assertion.
func TestStateChangeNeverMatchesALine(t *testing.T) {
	t.Parallel()
	p := Predicate{Kind: PredicateStateChange}
	for _, text := range []string{"", "EXITED", "state_change", "process exited with code 0"} {
		if p.MatchLine(Line{Text: text}) {
			t.Fatalf("a state_change predicate matched the line %q — payload must never be able to forge owner metadata", text)
		}
	}
	if !p.WatchesStateChanges() {
		t.Fatal("a state_change predicate must report that it watches state changes")
	}
	if (Predicate{Kind: PredicateContains, Value: "x"}).WatchesStateChanges() {
		t.Fatal("a contains predicate does not watch state changes")
	}
}

// A row written before a vocabulary change must stop matching, not stop the
// fleet: this runs in a sweeper over rows another process wrote.
func TestAnUnknownKindMatchesNothingRatherThanPanicking(t *testing.T) {
	t.Parallel()
	p := Predicate{Kind: "kind-from-the-future"}
	if p.MatchLine(Line{Text: "anything at all"}) {
		t.Fatal("an unrecognized predicate kind matched")
	}
}

type stubAdapter struct{ kind string }

func (s stubAdapter) Kind() string { return s.kind }
func (s stubAdapter) Poll(context.Context, Watch) (PollResult, error) {
	return PollResult{}, nil
}

// Two adapters for one kind makes the sweeper's behaviour depend on map
// iteration order — a bug that shows up only in production and only sometimes.
func TestAdapterSetRefusesDuplicatesAndNils(t *testing.T) {
	t.Parallel()
	if _, err := NewAdapterSet(stubAdapter{kind: "a"}, stubAdapter{kind: "a"}); err == nil {
		t.Fatal("two adapters registered for one kind")
	}
	if _, err := NewAdapterSet(stubAdapter{kind: ""}); err == nil {
		t.Fatal("an adapter with no kind registered")
	}
	if _, err := NewAdapterSet(nil); err == nil {
		t.Fatal("a nil adapter registered")
	}
	set, err := NewAdapterSet(stubAdapter{kind: "a"}, stubAdapter{kind: "b"})
	if err != nil {
		t.Fatalf("NewAdapterSet: %v", err)
	}
	if len(set) != 2 {
		t.Fatalf("adapter set has %d entries, want 2", len(set))
	}
	// The empty set is valid and is exactly what step 1 ships.
	empty, err := NewAdapterSet()
	if err != nil || len(empty) != 0 {
		t.Fatalf("an empty adapter set must be valid: %v", err)
	}
}
