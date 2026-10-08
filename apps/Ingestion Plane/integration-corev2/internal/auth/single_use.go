package auth

import (
	"strings"
	"sync"
	"time"
)

// single_use.go: replay protection for delegated (acting_user_id) tokens.
//
// Auth Core's RFC 8693 token exchange mints one short-lived integration
// token per tool call, and that token is a COMPLETE standalone bearer: it
// names the human end user the service acts for and needs no second secret
// to present. A captured copy would therefore be enough, on its own, to read
// that user's personal mail/drive/calendar for the token's remaining
// lifetime. Auth Core stamps every exchanged token with its own unguessable
// `jti`; this guard accepts each one exactly once, so a copy lifted from a
// log line or an APM header capture is dead the moment the legitimate request
// that carried it has been served.
//
// The guard sits on the resource server, not the issuer, because only the
// resource server ever sees the token again. (A seen-set on the run's
// session-core subject token at the issuer is not equivalent: one run holds
// one such bearer and exchanges it once per tool call, so single-use there
// fails every read of a run after its first and does nothing to bound a
// captured exchanged token.)
//
// Process-local by design: an entry only has to outlive the token's own
// verification window, not a restart. With more than one replica a captured
// token can still be replayed once per replica inside its window, the same
// trade-off every other process-local replay cache in this codebase makes.

const (
	// delegatedTokenRetentionLeeway is added to a token's `exp` before its
	// `jti` may be forgotten. It must be at least the verifier's clock
	// leeway (controlplane.AuthClient accepts tokens up to 30s past `exp`),
	// or a token could be replayed in the gap after its entry was pruned but
	// before it stopped verifying.
	delegatedTokenRetentionLeeway = time.Minute
	// delegatedTokenFallbackRetention bounds an entry whose token carries no
	// usable expiry, which a verified token never does.
	delegatedTokenFallbackRetention = 10 * time.Minute
	// delegatedTokenMaxEntries fails closed rather than growing without
	// bound. Every entry needs a validly signed, unexpired token, so this is
	// far above any real per-minute exchange rate.
	delegatedTokenMaxEntries = 200_000
)

type singleUseCache struct {
	mu   sync.Mutex
	seen map[string]time.Time
	now  func() time.Time
}

func newSingleUseCache(now func() time.Time) *singleUseCache {
	if now == nil {
		now = time.Now
	}
	return &singleUseCache{seen: make(map[string]time.Time), now: now}
}

// consume records id and reports whether this is the first use. It returns
// false for an empty id (a delegated token with no `jti` cannot be made
// single-use, so it is refused rather than waved through), for an id that has
// already been consumed, and when the cache is saturated.
func (c *singleUseCache) consume(id string, expiresAt time.Time) bool {
	id = strings.TrimSpace(id)
	if id == "" {
		return false
	}
	now := c.now()
	c.mu.Lock()
	defer c.mu.Unlock()
	for key, forgetAt := range c.seen {
		if !forgetAt.After(now) {
			delete(c.seen, key)
		}
	}
	if _, used := c.seen[id]; used {
		return false
	}
	if len(c.seen) >= delegatedTokenMaxEntries {
		return false
	}
	forgetAt := now.Add(delegatedTokenFallbackRetention)
	if !expiresAt.IsZero() {
		forgetAt = expiresAt.Add(delegatedTokenRetentionLeeway)
	}
	c.seen[id] = forgetAt
	return true
}

// delegatedTokenUse is shared by every InternalOrBearer instance so a token
// presented to one route group cannot be replayed against another.
var delegatedTokenUse = newSingleUseCache(time.Now)
