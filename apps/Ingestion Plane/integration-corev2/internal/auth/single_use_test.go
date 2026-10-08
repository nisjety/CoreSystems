package auth

import (
	"testing"
	"time"
)

type fakeClock struct{ at time.Time }

func (c *fakeClock) now() time.Time { return c.at }

func TestSingleUseCacheHonorsAnIDExactlyOnce(t *testing.T) {
	clock := &fakeClock{at: time.Date(2026, 9, 29, 12, 0, 0, 0, time.UTC)}
	cache := newSingleUseCache(clock.now)
	expires := clock.at.Add(90 * time.Second)

	if !cache.consume("jti-1", expires) {
		t.Fatal("first use of a token id was refused")
	}
	if cache.consume("jti-1", expires) {
		t.Fatal("second use of the same token id was accepted: a captured copy would be replayable")
	}
	if !cache.consume("jti-2", expires) {
		t.Fatal("a different token id was refused: consumption must be per token, not global")
	}
}

func TestSingleUseCacheRefusesATokenWithNoID(t *testing.T) {
	cache := newSingleUseCache(time.Now)
	for _, id := range []string{"", "   "} {
		if cache.consume(id, time.Now().Add(time.Minute)) {
			t.Fatalf("id %q was accepted: a token without an id can never be made single-use", id)
		}
	}
}

// An entry must outlive the token's own verification window (exp plus the
// verifier's 30s leeway), or the gap between "forgotten" and "no longer
// verifies" is a replay window. After that it must be forgotten so the cache
// does not grow forever.
func TestSingleUseCacheKeepsAnEntryThroughTheVerificationWindowThenForgetsIt(t *testing.T) {
	clock := &fakeClock{at: time.Date(2026, 9, 29, 12, 0, 0, 0, time.UTC)}
	cache := newSingleUseCache(clock.now)
	expires := clock.at.Add(90 * time.Second)
	if !cache.consume("jti-1", expires) {
		t.Fatal("first use refused")
	}

	// 30s of verifier leeway past exp: the token still verifies, so the id
	// must still be remembered.
	clock.at = expires.Add(30 * time.Second)
	if cache.consume("jti-1", expires) {
		t.Fatal("id was forgotten while the token could still verify")
	}

	clock.at = expires.Add(delegatedTokenRetentionLeeway + time.Second)
	if !cache.consume("jti-other", clock.at.Add(time.Minute)) {
		t.Fatal("unrelated id refused")
	}
	if _, kept := cache.seen["jti-1"]; kept {
		t.Fatal("expired id was never pruned")
	}
}

func TestSingleUseCacheFailsClosedWhenSaturated(t *testing.T) {
	clock := &fakeClock{at: time.Date(2026, 9, 29, 12, 0, 0, 0, time.UTC)}
	cache := newSingleUseCache(clock.now)
	forgetAt := clock.at.Add(time.Hour)
	for i := 0; i < delegatedTokenMaxEntries; i++ {
		cache.seen[string(rune('a'+i%26))+time.Duration(i).String()] = forgetAt
	}
	if len(cache.seen) < delegatedTokenMaxEntries {
		t.Fatalf("test setup built %d entries, want %d", len(cache.seen), delegatedTokenMaxEntries)
	}
	if cache.consume("one-too-many", clock.at.Add(time.Minute)) {
		t.Fatal("a saturated cache accepted a new id instead of failing closed")
	}
}
