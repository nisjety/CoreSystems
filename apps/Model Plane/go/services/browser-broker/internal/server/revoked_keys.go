package server

import (
	"sync"
	"time"
)

// RevokedKeyMemory is how long the broker remembers a revoked API key
// (INNH-10). It outlives the longest key grant (MaxKeyGrantTTL, 900 s) plus
// the longest exchanged key token (120 s) with room to spare, so a token
// minted before the revoke event can neither acquire a new grant nor keep
// validating one the sweep missed.
const RevokedKeyMemory = 20 * time.Minute

// maxRevokedKeys bounds the list. Past it (after pruning) the list reports
// every key as revoked: a flood of revoke events stops key grants rather
// than forgetting a revoked key.
const maxRevokedKeys = 65536

// revokedKeys is the broker's in-memory list of recently revoked API keys.
// It is per broker instance and does not survive a restart; the durable
// store's RevokeByKey sweep is the lasting effect, this list closes the race
// between that sweep and a key token that is still valid.
type revokedKeys struct {
	mu    sync.Mutex
	until map[string]time.Time
	now   func() time.Time
	// closedUntil: a key that did not fit was revoked; until its memory would
	// have ended, every key reads as revoked (fail closed).
	closedUntil time.Time
}

func newRevokedKeys(now func() time.Time) *revokedKeys {
	if now == nil {
		now = time.Now
	}
	return &revokedKeys{until: make(map[string]time.Time), now: now}
}

// mark records keyID as revoked for RevokedKeyMemory from now.
func (r *revokedKeys) mark(keyID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := r.now()
	if _, known := r.until[keyID]; !known && len(r.until) >= maxRevokedKeys {
		for id, until := range r.until {
			if !now.Before(until) {
				delete(r.until, id)
			}
		}
		if len(r.until) >= maxRevokedKeys {
			r.closedUntil = now.Add(RevokedKeyMemory)
			return
		}
	}
	r.until[keyID] = now.Add(RevokedKeyMemory)
}

// revoked reports whether keyID was revoked within RevokedKeyMemory, or the
// list overflowed within that window (fail closed).
func (r *revokedKeys) revoked(keyID string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := r.now()
	if now.Before(r.closedUntil) {
		return true
	}
	until, ok := r.until[keyID]
	return ok && now.Before(until)
}
