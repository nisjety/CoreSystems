package mailtask

import (
	"sync"
	"time"
)

// ReplayGuard is an in-memory, best-effort dedup cache keyed by the
// provider's message id. Inbound-parse webhooks are at-least-once delivery
// (spec §6, "provider retry storms"); this is a fast, cheap first line of
// defense against reprocessing an identical retry within this process's
// lifetime. It is NOT the sole idempotency guarantee -- the authoritative one
// is downstream, in session-core's session_key semantics and the gateway's
// idempotent execution-core turn (spec §6), both keyed off the same
// provider_message_id this cache is keyed by. Losing this cache (a restart)
// only means a retry does one extra round trip through the pipeline before
// the downstream idempotency key catches it -- never a correctness gap on
// its own.
type ReplayGuard struct {
	mu   sync.Mutex
	seen map[string]time.Time
	ttl  time.Duration
}

// NewReplayGuard builds a guard that forgets an id after ttl has passed.
func NewReplayGuard(ttl time.Duration) *ReplayGuard {
	return &ReplayGuard{seen: make(map[string]time.Time), ttl: ttl}
}

// SeenBefore records key (scoped by the caller -- typically
// "postmark\x00<MessageID>") and reports whether it was already recorded
// within the TTL window. Safe for concurrent use.
func (g *ReplayGuard) SeenBefore(key string, now time.Time) bool {
	if g == nil || key == "" {
		return false
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	for existingKey, expiresAt := range g.seen {
		if !now.Before(expiresAt) {
			delete(g.seen, existingKey)
		}
	}
	if expiresAt, ok := g.seen[key]; ok && now.Before(expiresAt) {
		return true
	}
	g.seen[key] = now.Add(g.ttl)
	return false
}
