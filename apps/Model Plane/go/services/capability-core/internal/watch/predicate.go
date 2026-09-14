package watch

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

// Predicate kinds. This vocabulary is CLOSED, and the closure is the design.
//
// # Why there is no regular expression here
//
// A watch predicate runs against output a model wrote, on a shared sweeper,
// against a subject a program emitted. Once a model can create a watch (a later
// slice), the same actor supplies BOTH the pattern and the subject — an
// attacker on both ends of a catastrophic-backtracking denial of service, in a
// process that is also polling every other watch in the fleet.
//
// A literal substring is not interesting enough to be dangerous, and it covers
// what people actually want: "tell me when it says ERROR". If a later slice
// needs more, the honest addition is a fixed set of NAMED predicates
// (`nonzero_exit`, `stderr_any`) whose implementations live here — not an
// expression language whose inputs live in the database.
const (
	// PredicateAny matches any new content at all: a liveness watch.
	PredicateAny = "any"
	// PredicateContains matches a complete line containing a literal.
	PredicateContains = "contains"
	// PredicateStateChange matches the source's own lifecycle moving. It never
	// matches a line — [Predicate.MatchLine] returns false for it — because a
	// state change is owner metadata and lines are unscreened payload, and the
	// two must not be able to produce each other's events.
	PredicateStateChange = "state_change"
)

// Stream selectors for line-oriented sources.
const (
	StreamAny    = ""
	StreamStdout = "stdout"
	StreamStderr = "stderr"
)

// MaxPredicateValueBytes matches the migration's CHECK.
const MaxPredicateValueBytes = 200

// Predicate is the whole matching grammar. One per watch: "any of these five
// things" is five watches, which keeps evaluation, authority and cancellation
// per-answer rather than per-set.
type Predicate struct {
	Kind   string
	Value  string
	Stream string
}

// Validate enforces the closed vocabulary and the shape rules.
func (p Predicate) Validate() error {
	switch p.Kind {
	case PredicateAny, PredicateStateChange:
		if p.Value != "" {
			// A value that is never read looks like a filter that is applied.
			// Refusing it is cheaper than explaining later why the watch
			// matched everything.
			return fmt.Errorf("a %q predicate takes no value", p.Kind)
		}
	case PredicateContains:
		if strings.TrimSpace(p.Value) == "" {
			return fmt.Errorf("a %q predicate requires a value", PredicateContains)
		}
		if len(p.Value) > MaxPredicateValueBytes {
			return fmt.Errorf("predicate value is longer than %d bytes", MaxPredicateValueBytes)
		}
	default:
		return fmt.Errorf("predicate kind %q is not recognized", p.Kind)
	}
	switch p.Stream {
	case StreamAny, StreamStdout, StreamStderr:
	default:
		return fmt.Errorf("predicate stream %q is not recognized", p.Stream)
	}
	if p.Kind == PredicateStateChange && p.Stream != StreamAny {
		// A state change has no stream. Accepting one would let a caller write
		// a watch whose selector silently does nothing.
		return fmt.Errorf("a %q predicate has no stream", PredicateStateChange)
	}
	return nil
}

// Line is one COMPLETE line from a line-oriented source.
//
// Complete is load-bearing. An adapter must not hand a partial line here: a
// predicate evaluated against a prefix fires on text the next chunk completes
// into something else — the same class of error as scrubbing a secret cut in
// half, which is why the process registry scrubs at line boundaries and carries
// `ends_with_newline` on every chunk in the first place.
type Line struct {
	// Cursor is the source's own position for this line, and what gets
	// committed if this line produces an event.
	Cursor int64
	Stream string
	Text   string
}

// MatchLine reports whether this line satisfies the predicate.
func (p Predicate) MatchLine(line Line) bool {
	if !p.matchesStream(line.Stream) {
		return false
	}
	switch p.Kind {
	case PredicateAny:
		return true
	case PredicateContains:
		return strings.Contains(line.Text, p.Value)
	default:
		// PredicateStateChange, and any kind Validate would have refused.
		// Falling through to false rather than panicking: this runs in a
		// sweeper over rows written by another process, and a row that
		// predates a vocabulary change must stop matching, not stop the fleet.
		return false
	}
}

func (p Predicate) matchesStream(stream string) bool {
	return p.Stream == StreamAny || p.Stream == stream
}

// WatchesStateChanges reports whether this predicate cares about the source's
// lifecycle.
//
// Every watch does, in the sense that a terminal source ends the watch. This
// asks the narrower question: does a state change EMIT an event for this watch?
func (p Predicate) WatchesStateChanges() bool {
	return p.Kind == PredicateStateChange
}

// PollResult is what an adapter saw at a watch's cursor.
//
// It must be idempotent in the cursor: called twice with the same cursor, an
// adapter returns the same answer. That is what makes "crash before cursor
// commit" recoverable by simply re-reading, with nothing to reconcile.
type PollResult struct {
	// Lines completed since the cursor, in order. Complete lines only.
	Lines []Line
	// Cursor to commit if this poll is accepted. An adapter that returns no
	// lines still moves this when the source has advanced past content that
	// did not match.
	Cursor int64
	// GapBefore means the source reported that content between the watch's
	// cursor and the first line here was trimmed and will never be returned.
	// Never silently swallowed: a watch that skipped output without saying so
	// is worse than one that says it did.
	GapBefore bool
	// SourceTerminal means the source has finished AND been drained. An
	// adapter must not set this while output remains, because the last lines
	// are exactly where a failing build says why it failed.
	SourceTerminal bool
	// TerminalSummary is the owner's own account of how the source ended
	// ("exited with code 2"). Emitted as [TrustOwnerMetadata], so it must
	// never carry source payload.
	TerminalSummary string
}

// ErrSourceGone means a source no longer exists, or no longer belongs to the
// watching Space.
//
// The two are deliberately ONE error, everywhere. Distinguishing "no such
// resource" from "a resource in another Space" would make a watch an oracle for
// which ids exist in an organization.
//
// An adapter returns this to END a watch rather than back it off, and it is the
// only adapter error that does. Everything else — an unreachable service, a
// refused credential — is transient, and terminating on transient failure would
// silently cancel a person's watch over an outage.
var ErrSourceGone = errors.New("the watched source is not available to this Space")

// SourceAdapter is what a watchable source must be able to do.
//
// Deliberately narrow. An adapter reads and reports; it does not decide whether
// to emit, does not touch the watch row, and does not know about authority —
// the sweeper owns all three, so a new adapter cannot accidentally acquire the
// power to bypass them.
type SourceAdapter interface {
	// Kind is the `source_kind` this adapter serves.
	Kind() string

	// Poll reads from the source at the watch's cursor.
	//
	// Implementations MUST apply their own Space check: the sweeper's service
	// credentials are org-wide, and `source_ref` is caller-supplied. An adapter
	// that resolves a resource without comparing its recorded Space against the
	// watch's own has handed one Space's content to another.
	Poll(ctx context.Context, w Watch) (PollResult, error)
}

// AdapterSet resolves a source kind to its adapter.
//
// Empty in step 1, on purpose: the sweeper claims rows and finds nothing to do,
// which is the whole of what step 1 ships. A watch whose kind has no adapter is
// left ACTIVE and backed off rather than terminated — an adapter can be
// deployed later, and terminating a person's watch because this binary is older
// than their watch would be the wrong answer.
type AdapterSet map[string]SourceAdapter

// NewAdapterSet indexes adapters by kind, refusing duplicates.
func NewAdapterSet(adapters ...SourceAdapter) (AdapterSet, error) {
	set := make(AdapterSet, len(adapters))
	for _, adapter := range adapters {
		if adapter == nil {
			return nil, fmt.Errorf("a nil source adapter cannot be registered")
		}
		kind := adapter.Kind()
		if strings.TrimSpace(kind) == "" {
			return nil, fmt.Errorf("a source adapter must declare a kind")
		}
		if _, exists := set[kind]; exists {
			// Two adapters for one kind means the sweeper's behaviour depends
			// on map iteration order, which is a bug that only shows up in
			// production and only sometimes.
			return nil, fmt.Errorf("source kind %q already has an adapter", kind)
		}
		set[kind] = adapter
	}
	return set, nil
}
