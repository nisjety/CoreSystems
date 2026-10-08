package mailtask

import (
	"sync"
	"time"
)

// ReplayGuard is an in-memory, best-effort cache of provider message ids whose
// processing reached a terminal verdict, plus the set of ids being processed
// right now. Inbound-parse webhooks are at-least-once delivery (spec §6,
// "provider retry storms"), and Postmark retries any delivery we answered with
// a non-2xx status.
//
// The rule that matters: an id is remembered (Remember) only after an outcome
// the HTTP layer answers 200 for and that must never be retried -- accepted,
// alias_not_found, sender_not_verified. An id whose processing failed
// downstream (gateway_unavailable, alias_resolver_unavailable) is never
// remembered, because Postmark was told to retry it; recording it as seen
// would turn that retry into a silent 200 drop and lose the mail. The cache
// is also never written on the way in: Seen is read-only, and the in-flight
// set (Begin/End) is what keeps two simultaneous deliveries of one id from
// both reaching the gateway.
//
// This guard is an optimisation, not the idempotency guarantee. A retry that
// follows an ambiguous failure (the gateway created the thread, then timed out
// before answering) is deliberately reprocessed; the authoritative guarantee
// against a second thread is downstream, in session-core's session_key
// semantics (spec §6) once thread creation is idempotent on that key (plan
// task J4). Losing this cache (a restart) only costs a retry one extra round
// trip through the pipeline, never a correctness gap on its own.
type ReplayGuard struct {
	mu       sync.Mutex
	seen     map[string]time.Time
	inFlight map[string]struct{}
	ttl      time.Duration
}

// BeginResult is the verdict Begin gives for one delivery of a message id.
type BeginResult int

const (
	// BeginAcquired means the caller now owns this id: it must process the
	// message and then call End, calling Remember first if (and only if) the
	// outcome was terminal.
	BeginAcquired BeginResult = iota
	// BeginDuplicate means the id already reached a terminal verdict within
	// the TTL window; the delivery is a replay to be acknowledged and dropped.
	BeginDuplicate
	// BeginInFlight means another delivery of the same id is being processed
	// right now. The caller must not process it and must not tell the
	// provider it succeeded: that other delivery may still fail.
	BeginInFlight
)

// NewReplayGuard builds a guard that forgets an id after ttl has passed.
func NewReplayGuard(ttl time.Duration) *ReplayGuard {
	return &ReplayGuard{
		seen:     make(map[string]time.Time),
		inFlight: make(map[string]struct{}),
		ttl:      ttl,
	}
}

// Seen reports whether key (scoped by the caller -- typically
// "postmark\x00<MessageID>") reached a terminal verdict within the TTL window.
// It is read-only: it never records key, so asking cannot cause a later retry
// to be mistaken for a duplicate. Safe for concurrent use.
func (g *ReplayGuard) Seen(key string, now time.Time) bool {
	if g == nil || key == "" {
		return false
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.seenLocked(key, now)
}

// Remember records that key reached a terminal verdict that must never be
// retried, so a replay within the TTL window is reported as a duplicate. Call
// it only for such outcomes -- never after a downstream failure the provider
// was told to retry. Safe for concurrent use.
func (g *ReplayGuard) Remember(key string, now time.Time) {
	if g == nil || key == "" {
		return
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	for existingKey, expiresAt := range g.seen {
		if !now.Before(expiresAt) {
			delete(g.seen, existingKey)
		}
	}
	g.seen[key] = now.Add(g.ttl)
}

// Begin claims key for processing. The duplicate check and the in-flight claim
// happen under one lock, so a delivery that finishes between a caller's Seen
// and its Begin cannot be processed a second time. A BeginAcquired caller must
// call End (typically deferred) whatever the outcome; that is what clears the
// claim after a failure so the provider's retry is processed. A nil guard or
// an empty key (nothing to dedupe on) always acquires. Safe for concurrent use.
func (g *ReplayGuard) Begin(key string, now time.Time) BeginResult {
	if g == nil || key == "" {
		return BeginAcquired
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.seenLocked(key, now) {
		return BeginDuplicate
	}
	if _, busy := g.inFlight[key]; busy {
		return BeginInFlight
	}
	g.inFlight[key] = struct{}{}
	return BeginAcquired
}

// End releases the in-flight claim Begin granted for key. Call Remember first
// when the outcome was terminal, so there is no instant at which key is
// neither in flight nor remembered. Safe for concurrent use.
func (g *ReplayGuard) End(key string) {
	if g == nil || key == "" {
		return
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	delete(g.inFlight, key)
}

func (g *ReplayGuard) seenLocked(key string, now time.Time) bool {
	expiresAt, ok := g.seen[key]
	return ok && now.Before(expiresAt)
}
