// Package watch is the S4.3 general Watch primitive: a standing intent to be
// told when something changes.
//
// # What a Watch is not
//
//   - Not a delivery mechanism. S4.4 owns delivery — outbox, claim, receipt,
//     and an honest `unknown` outcome. A watch records WHERE an event should go
//     as a reference and stops at `recorded`. Emitting a notification from here
//     would promise at-least-once delivery with none of that behind it.
//   - Not a live tail. Polling has lag by construction (see [PollInterval]).
//     If a surface needs a live tail, that is a different mechanism, not this
//     one with a smaller interval.
//   - Not a topic subscription. A watch is bound to ONE resource in ONE Space
//     under ONE member's authority. "Tell me about everything in this org" is
//     not expressible, and that is the point.
//
// # Why capability-core owns it
//
// A Watch is structurally a Schedule. A `cron_schedules` row is created by a
// Space member under a Control decision, stores the Space ref and all five
// authority revisions, is advanced by a sweeper claiming rows FOR UPDATE SKIP
// LOCKED, and is reauthorized freshly per fire because — in that sweeper's own
// words — a long-lived schedule record is not authority by itself. Every one of
// those sentences is true of a watch. The only difference is the trigger: a
// clock, versus a source's cursor moving.
//
// # The cursor is the contract
//
// Every verification this primitive owes — crash before cursor commit,
// duplicate event, unwatch race — is a statement about a cursor. A source that
// cannot say "I have consumed up to here, durably" cannot be a watch source.
// The commit rule is in [Store.CommitPoll]: the event and the cursor move in
// one transaction, event first.
package watch

import (
	"fmt"
	"strings"
	"time"
)

// State is a watch's lifecycle position. Every terminal state is terminal: a
// watch is never resurrected, because the authority that justified the first
// one is not the authority that justifies the second. Re-watching is a new row.
type State int16

const (
	// StateActive is the only state that polls.
	StateActive State = 1
	// StateTriggered is a `once` watch that has emitted its event. A
	// `continuous` watch emits and stays StateActive.
	StateTriggered State = 2
	// StateExpired means expires_at passed. "We stopped looking."
	StateExpired State = 3
	// StateCancelled means a member unwatched.
	StateCancelled State = 4
	// StateSourceGone means the source reached a terminal state and was
	// drained, or its authority was revoked.
	//
	// Deliberately NOT folded into StateExpired: "the thing you were watching
	// finished and you saw all of it" is an answer, and "we stopped looking" is
	// not. A person reading a list needs to tell those apart.
	StateSourceGone State = 5
)

// Terminal reports whether no further polling can occur.
func (s State) Terminal() bool { return s != StateActive }

// String renders the state for logs and the read path.
func (s State) String() string {
	switch s {
	case StateActive:
		return "ACTIVE"
	case StateTriggered:
		return "TRIGGERED"
	case StateExpired:
		return "EXPIRED"
	case StateCancelled:
		return "CANCELLED"
	case StateSourceGone:
		return "SOURCE_GONE"
	default:
		return "UNSPECIFIED"
	}
}

// Trigger modes. `continuous` is accepted by the schema but not implemented by
// any adapter in this pass: a continuous watch on a chatty source is a
// delivery-RATE problem, which is S4.4's to answer.
const (
	TriggerOnce       = "once"
	TriggerContinuous = "continuous"
)

// SourceKindProcessOutput is the first adapter's kind (step 2). Declared here
// rather than in the adapter so the store's validation does not import it.
const SourceKindProcessOutput = "process_output"

// Event kinds. `gap` is its own kind rather than a flag on a match: output that
// was produced and then trimmed by retention is not the same as output that
// never existed, and a reader summarising a log has to be told which it is
// looking at.
const (
	EventMatch       = "match"
	EventStateChange = "state_change"
	EventGap         = "gap"
	EventSourceGone  = "source_gone"
	EventExpired     = "expired"
)

// Trust levels. A COLUMN rather than a convention, because a consumer that
// cannot tell these apart will eventually render one as the other — and a watch
// is the first thing that carries a source's own bytes OUT of the run that
// produced them.
const (
	// TrustUnscreened is anything derived from watched content: bytes a model
	// chose, from a program a model wrote. Redaction is not trust; the source
	// already scrubbed the line and it is still attacker-chosen text.
	TrustUnscreened = "unscreened_source_payload"
	// TrustOwnerMetadata is a fact the owning plane asserts — a state change,
	// an exit code. Never a payload.
	TrustOwnerMetadata = "owner_metadata"
)

// MaxSummaryBytes bounds what one event carries into a list. Matches the
// migration's CHECK, so a violation is caught in Go before Postgres has to.
const MaxSummaryBytes = 256

// Polling bounds. The adoption plan permits polling "only behind an adapter
// with explicit lag/rate/budget behavior", so these are the explicit half.
//
// The honest cost, stated rather than buried: up to BasePollInterval of lag on
// the first event and up to MaxPollInterval on a source that has been quiet.
//
// BasePollInterval is the one number here with no evidence behind it. A person
// waiting on a build tolerates two seconds and a tighter loop buys nothing a
// human notices — but the first real watch on a real build should be used to
// correct it rather than treated as settled.
const (
	BasePollInterval = 2 * time.Second
	MaxPollInterval  = 30 * time.Second
	// maxIdleShift keeps 1<<idlePolls from overflowing on a watch that has been
	// idle for a very long time. Any value at or past this is already clamped
	// to MaxPollInterval anyway.
	maxIdleShift = 16
)

// PollInterval derives the current backoff from the idle level.
//
// Derived rather than stored, so there is one source of truth: a counter and a
// duration kept side by side are two things that can disagree, and the one that
// disagrees silently is the duration.
func PollInterval(idlePolls int) time.Duration {
	if idlePolls < 0 {
		idlePolls = 0
	}
	if idlePolls > maxIdleShift {
		idlePolls = maxIdleShift
	}
	interval := BasePollInterval * time.Duration(1<<uint(idlePolls))
	if interval > MaxPollInterval || interval <= 0 {
		return MaxPollInterval
	}
	return interval
}

// MaxFailuresBeforeBackoff is the failure budget. A watch that keeps failing
// backs off to the ceiling; it does NOT terminate, because a transient
// sandbox-manager outage must not silently cancel a person's watch. The only
// things that end a watch are the member, the clock, and the source.
const MaxFailuresBeforeBackoff = 5

// Watch is one standing intent. Field order follows the migration.
type Watch struct {
	ID               string
	OrgID            string
	SpaceRef         string
	CreatorSubjectID string

	SourceKind string
	SourceRef  string

	Predicate Predicate

	CursorValue       int64
	CursorCommittedAt *time.Time

	State       State
	TriggerMode string

	NextPollAt   time.Time
	IdlePolls    int
	LastPolledAt *time.Time

	LastEventAt         *time.Time
	LastEventSummary    string
	ConsecutiveFailures int
	ExpiresAt           time.Time

	DeliveryTargetRef string

	Authority Authority

	CreatedAt time.Time
	UpdatedAt time.Time
}

// Authority is what a Control decision left on the record at create time.
//
// Kept so the sweeper has something to COMPARE against when it re-derives
// current authority before emitting. The record is not the authority; this is
// the evidence of what the authority was when the watch was allowed to exist.
type Authority struct {
	RecipientAudienceRef      string
	RecipientAudienceHash     string
	ResourceAuthorizationRef  string
	PrivacyPolicyRef          string
	AuthorityRevision         int64
	MembershipRevision        int64
	PrivacyRevision           int64
	RecipientAudienceRevision int64
	EntitlementRevision       int64
}

// Event is one thing a watch observed.
type Event struct {
	ID          string
	WatchID     string
	OrgID       string
	CursorValue int64
	Kind        string
	Summary     string
	Trust       string
	EmittedAt   time.Time
}

// Validate checks a watch a caller is asking to create.
//
// Mirrors the migration's CHECK constraints deliberately rather than relying on
// them: a constraint violation arrives as an opaque Postgres error several
// layers from the caller, and "predicate_value must be non-empty for a contains
// watch" is a sentence the caller can act on. The database keeps its copy
// because Go is not the only thing that will ever write these rows.
func (w Watch) Validate() error {
	for label, value := range map[string]string{
		"id":                 w.ID,
		"org_id":             w.OrgID,
		"space_ref":          w.SpaceRef,
		"creator_subject_id": w.CreatorSubjectID,
		"source_kind":        w.SourceKind,
		"source_ref":         w.SourceRef,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("watch %s is required", label)
		}
	}
	if err := w.Predicate.Validate(); err != nil {
		return err
	}
	switch w.TriggerMode {
	case TriggerOnce, TriggerContinuous:
	default:
		return fmt.Errorf("watch trigger_mode %q is not recognized", w.TriggerMode)
	}
	if w.State != StateActive {
		// A watch is created ACTIVE or not at all. Creating one already
		// terminal would be a row nothing will ever look at again, which is a
		// caller bug rather than a state.
		return fmt.Errorf("a watch is created ACTIVE, not %s", w.State)
	}
	if w.ExpiresAt.IsZero() {
		// No unbounded watches. A standing intent with no end is a standing
		// cost, and the person who set it has long stopped expecting it.
		return fmt.Errorf("watch expires_at is required")
	}
	if w.CursorValue < 0 {
		return fmt.Errorf("watch cursor_value must not be negative")
	}
	return nil
}

// BoundSummary truncates a summary to the column's limit, on a rune boundary so
// a multi-byte character is never cut in half.
//
// Truncation is marked. A silently shortened line reads as a complete one, and
// a person deciding whether a build failed on the strength of a summary should
// be able to see that there was more.
func BoundSummary(raw string) string {
	cleaned := strings.TrimRight(raw, "\r\n")
	if len(cleaned) <= MaxSummaryBytes {
		return cleaned
	}
	const marker = "…"
	limit := MaxSummaryBytes - len(marker)
	cut := limit
	for cut > 0 && !isRuneStart(cleaned[cut]) {
		cut--
	}
	return cleaned[:cut] + marker
}

func isRuneStart(b byte) bool { return b&0xC0 != 0x80 }
