package watch

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/google/uuid"
)

// DefaultClaimLimit bounds one sweep. Small on purpose: the claim advances
// next_poll_at before any network work, so a large batch would push a long tail
// of watches an interval into the future while the first ones are still being
// polled. Several small sweeps spread the same load without that skew.
const DefaultClaimLimit = 20

// DefaultSweepInterval is how often the loop looks for due watches. It is
// deliberately shorter than [BasePollInterval] so a watch that becomes due
// between ticks is picked up on the next one rather than waiting a full extra
// interval — the sweep cadence should not add lag on top of the poll cadence.
const DefaultSweepInterval = time.Second

// Reauthorizer re-derives current authority for one watch immediately before it
// is allowed to observe anything.
//
// This is the cron sweeper's FireAuthorizer rule, unchanged and for the same
// reason: a long-lived record is not authority by itself. A watch created weeks
// ago under a membership that has since been revoked, or a Space whose
// recipient audience has moved, must stop rather than emit.
//
// Returning an error terminates the watch as [StateSourceGone] — "the thing you
// were watching is no longer yours to see" is an answer, and leaving it ACTIVE
// to retry forever would be a poller quietly probing a resource it was refused.
type Reauthorizer interface {
	AuthorizeObserve(ctx context.Context, w Watch) error
}

// Sweeper claims due watches, polls each one's adapter, and commits what it saw.
//
// # What step 1 ships
//
// Everything except an adapter. With an empty [AdapterSet] the sweeper claims
// rows, finds no adapter for their kind, backs them off, and commits nothing —
// which is precisely the "claims rows and does nothing with them" the design
// asked for. Step 2 registers the first adapter and the same loop starts doing
// real work with no change here.
type Sweeper struct {
	store    *Store
	adapters AdapterSet
	authz    Reauthorizer
	interval time.Duration
	limit    int
	newID    func() string
}

// NewSweeper constructs a sweeper. A nil or empty adapter set is valid and
// means "nothing is watchable yet".
func NewSweeper(store *Store, adapters AdapterSet, authz Reauthorizer) (*Sweeper, error) {
	if store == nil {
		return nil, fmt.Errorf("a watch sweeper requires a store")
	}
	return &Sweeper{
		store:    store,
		adapters: adapters,
		authz:    authz,
		interval: DefaultSweepInterval,
		limit:    DefaultClaimLimit,
		newID:    func() string { return "wev_" + uuid.New().String() },
	}, nil
}

// Run sweeps until the context is cancelled.
func (s *Sweeper) Run(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if _, err := s.RunOnce(ctx); err != nil && !errors.Is(err, context.Canceled) {
				slog.Warn("watch sweep failed", "error", err)
			}
		}
	}
}

// RunOnce expires overdue watches, claims due ones, and polls each. Returns how
// many watches were polled.
//
// A failure on one watch never aborts the sweep: one unreachable source must
// not stop every other Space's watches from being served.
func (s *Sweeper) RunOnce(ctx context.Context) (int, error) {
	if _, err := s.store.ExpireOverdue(ctx, s.limit); err != nil {
		// Reported, not fatal. An expiry that did not happen this tick happens
		// next tick, and the claim independently refuses to pick up an expired
		// watch — so a failure here delays tidying, it does not let an expired
		// watch observe anything.
		slog.Warn("expiring overdue watches failed", "error", err)
	}
	claimed, err := s.store.ClaimDue(ctx, s.limit)
	if err != nil {
		return 0, err
	}
	polled := 0
	for i := range claimed {
		if ctx.Err() != nil {
			return polled, ctx.Err()
		}
		s.pollOne(ctx, claimed[i])
		polled++
	}
	return polled, nil
}

func (s *Sweeper) pollOne(ctx context.Context, w Watch) {
	adapter, ok := s.adapters[w.SourceKind]
	if !ok {
		// No adapter for this kind on this binary. Left ACTIVE and backed off
		// rather than terminated: an adapter can be deployed later, and ending
		// a person's watch because this process is older than their watch
		// would be the wrong answer to a deployment-ordering question.
		if err := s.store.RecordFailure(ctx, w); err != nil {
			slog.Warn("backing off an unadapted watch failed", "watch", w.ID, "error", err)
		}
		return
	}

	// An unconfigured reauthorizer REFUSES rather than skips. A nil check that
	// falls through to "observe anyway" is the kind of default that ships and is
	// then forgotten, and what it would be defaulting past is the check that
	// stops a watch created weeks ago under a membership since revoked. Backed
	// off rather than terminated, because this is a deployment gap and not a
	// statement about the watch.
	if s.authz == nil {
		slog.Warn("refusing to poll a watch: no reauthorizer is configured",
			"watch", w.ID, "space", w.SpaceRef)
		if err := s.store.RecordFailure(ctx, w); err != nil {
			slog.Warn("backing off an unauthorizable watch failed", "watch", w.ID, "error", err)
		}
		return
	}

	result, err := adapter.Poll(ctx, w)
	if err != nil {
		// ErrSourceGone is the ONLY adapter error that ends a watch. Everything
		// else — an unreachable service, a refused credential — is transient,
		// and terminating on those would silently cancel a person's watch over
		// an outage.
		if errors.Is(err, ErrSourceGone) {
			slog.Info("terminating a watch whose source is gone", "watch", w.ID, "source", w.SourceRef)
			if termErr := s.store.Terminate(ctx, w.OrgID, w.ID, StateSourceGone,
				"the watched source is no longer available"); termErr != nil {
				slog.Warn("terminating a gone-source watch failed", "watch", w.ID, "error", termErr)
			}
			return
		}
		slog.Debug("watch poll failed", "watch", w.ID, "kind", w.SourceKind, "error", err)
		if failErr := s.store.RecordFailure(ctx, w); failErr != nil {
			slog.Warn("recording a watch failure failed", "watch", w.ID, "error", failErr)
		}
		return
	}

	events, cursor := s.decide(w, result)

	// Authority immediately BEFORE the disclosure, not before the read.
	//
	// A poll that matched nothing read into this process's memory on
	// capability-core's own service credential — bound to the watch's Space by
	// the adapter — and discarded it. Nothing durable, nothing a human can see.
	// What needs fresh human authority is turning a read into a RECORDED event,
	// so the check sits here, and a quiet watch costs Control nothing.
	if len(events) > 0 {
		if err := s.authz.AuthorizeObserve(ctx, w); err != nil {
			if errors.Is(err, ErrAuthorityUnavailable) {
				// Control could not answer, which says nothing about the member.
				// The cursor is deliberately NOT committed: the adapter is
				// idempotent in it, so re-reading recovers exactly these events
				// once Control is reachable. Committing here would skip a match
				// permanently.
				slog.Warn("deferring a watch observation; current authority is unavailable",
					"watch", w.ID, "error", err)
				if failErr := s.store.RecordFailure(ctx, w); failErr != nil {
					slog.Warn("backing off a watch failed", "watch", w.ID, "error", failErr)
				}
				return
			}
			slog.Info("terminating a watch whose authority no longer holds",
				"watch", w.ID, "space", w.SpaceRef, "error", err)
			if termErr := s.store.Terminate(ctx, w.OrgID, w.ID, StateSourceGone,
				"authority for this watch is no longer current"); termErr != nil {
				slog.Warn("terminating an unauthorized watch failed", "watch", w.ID, "error", termErr)
			}
			return
		}
	}

	terminal := StateActive
	if result.SourceTerminal {
		terminal = StateSourceGone
	}
	if err := s.store.CommitPoll(ctx, w, events, cursor, len(events) == 0, terminal); err != nil {
		slog.Warn("committing a watch poll failed", "watch", w.ID, "error", err)
	}
}

// decide turns one poll into the events to record and the cursor to commit.
//
// Pure, so the whole emission contract is unit-testable without a database or a
// source: what matches, what a gap produces, how a terminal source is reported,
// and — the one that matters most — that a `once` watch emits at most one event.
func (s *Sweeper) decide(w Watch, result PollResult) ([]Emission, int64) {
	var events []Emission
	cursor := result.Cursor
	if cursor < w.CursorValue {
		cursor = w.CursorValue
	}

	// A gap is reported FIRST and at the watch's own cursor, so it is attached
	// to where the hole actually was rather than to whatever happened to be
	// read next. Never swallowed: a watch that skipped output without saying so
	// is worse than one that says it did.
	if result.GapBefore {
		events = append(events, Emission{
			ID:      s.newID(),
			Kind:    EventGap,
			Summary: "output before this point was dropped by retention and cannot be recovered",
			Trust:   TrustOwnerMetadata,
			Cursor:  w.CursorValue,
		})
	}

	// At most ONE match event per cursor position, which for a line-oriented
	// source means one per chunk.
	//
	// This is not a throttle, it is a correctness requirement. A chunk is a
	// flush of many lines, so several matching lines commonly share one seq —
	// and the event key is (watch_id, cursor_value, kind). Emitting one event
	// per line would make the second collide with the first and be silently
	// dropped by ON CONFLICT DO NOTHING, which loses a match rather than
	// deduplicating a replay. Recording one event that names the first matching
	// line is honest and idempotent; recording several is neither.
	matchedCursors := map[int64]bool{}
	for _, line := range result.Lines {
		if w.TriggerMode == TriggerOnce && hasKind(events, EventMatch) {
			// A `once` watch answers once. Continuing to scan would record
			// matches nobody asked for and move the cursor past content a
			// re-watch would then never see.
			break
		}
		if !w.Predicate.MatchLine(line) || matchedCursors[line.Cursor] {
			continue
		}
		matchedCursors[line.Cursor] = true
		events = append(events, Emission{
			ID:   s.newID(),
			Kind: EventMatch,
			// The matched line itself, bounded and already scrubbed by the
			// source. Labelled unscreened because it is: a scrubbed line is
			// still text a model chose.
			Summary: line.Text,
			Trust:   TrustUnscreened,
			Cursor:  line.Cursor,
		})
	}

	if result.SourceTerminal {
		summary := result.TerminalSummary
		if summary == "" {
			summary = "the watched source finished"
		}
		events = append(events, Emission{
			ID:   s.newID(),
			Kind: EventSourceGone,
			// Owner metadata: the owning plane's own account of how its
			// resource ended. An adapter that puts payload here is refused by
			// validateEmission's kind/trust pairing.
			Summary: summary,
			Trust:   TrustOwnerMetadata,
			Cursor:  cursor,
		})
	}
	return events, cursor
}

func hasKind(events []Emission, kind string) bool {
	for _, event := range events {
		if event.Kind == kind {
			return true
		}
	}
	return false
}
