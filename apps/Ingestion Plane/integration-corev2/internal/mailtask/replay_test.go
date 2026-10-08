package mailtask

import (
	"testing"
	"time"
)

func TestReplayGuardSeenIsReadOnly(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	// Asking must never record: a retry after a failed delivery would
	// otherwise be mistaken for a duplicate of the attempt that failed.
	for attempt := 1; attempt <= 3; attempt++ {
		if guard.Seen("key-1", now.Add(time.Duration(attempt)*time.Minute)) {
			t.Fatalf("Seen #%d reported a repeat for a key that was never remembered", attempt)
		}
	}
}

func TestReplayGuardRememberMakesALaterSightingARepeat(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	if guard.Seen("key-1", now) {
		t.Fatal("first sighting must not be reported as a repeat")
	}
	guard.Remember("key-1", now)
	if !guard.Seen("key-1", now.Add(time.Minute)) {
		t.Fatal("a sighting within the TTL window after Remember must be reported as a repeat")
	}
}

func TestReplayGuardForgetsAfterTTLExpires(t *testing.T) {
	guard := NewReplayGuard(time.Minute)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.Remember("key-1", now)
	if guard.Seen("key-1", now.Add(2*time.Minute)) {
		t.Fatal("an id past its TTL must be treated as unseen again")
	}
	if got := guard.Begin("key-1", now.Add(2*time.Minute)); got != BeginAcquired {
		t.Fatalf("Begin past the TTL = %v, want BeginAcquired", got)
	}
}

func TestReplayGuardRememberPrunesExpiredEntries(t *testing.T) {
	guard := NewReplayGuard(time.Minute)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.Remember("old", now)
	guard.Remember("new", now.Add(2*time.Minute))
	guard.mu.Lock()
	defer guard.mu.Unlock()
	if _, kept := guard.seen["old"]; kept {
		t.Fatal("an expired entry must be pruned when a later key is remembered")
	}
}

func TestReplayGuardDistinguishesKeys(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.Remember("key-1", now)
	if guard.Seen("key-2", now) {
		t.Fatal("a different key must not collide with an unrelated one")
	}
	if got := guard.Begin("key-2", now); got != BeginAcquired {
		t.Fatalf("Begin for an unrelated key = %v, want BeginAcquired", got)
	}
}

func TestReplayGuardBeginClaimsAKeyUntilEnd(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	if got := guard.Begin("key-1", now); got != BeginAcquired {
		t.Fatalf("first Begin = %v, want BeginAcquired", got)
	}
	if got := guard.Begin("key-1", now); got != BeginInFlight {
		t.Fatalf("Begin while the first is in flight = %v, want BeginInFlight", got)
	}
	// An in-flight claim is not a verdict: Seen stays false until Remember.
	if guard.Seen("key-1", now) {
		t.Fatal("an in-flight key must not be reported as already seen")
	}
	guard.End("key-1")
	if got := guard.Begin("key-1", now); got != BeginAcquired {
		t.Fatalf("Begin after End without Remember = %v, want BeginAcquired (a failed delivery must be retryable)", got)
	}
}

func TestReplayGuardBeginAfterRememberIsADuplicateEvenWithoutEnd(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	if got := guard.Begin("key-1", now); got != BeginAcquired {
		t.Fatalf("first Begin = %v, want BeginAcquired", got)
	}
	guard.Remember("key-1", now)
	// Remember lands before End, so there is no instant at which the key is
	// neither in flight nor remembered; a concurrent Begin sees a duplicate,
	// not an in-flight claim it could retry forever.
	if got := guard.Begin("key-1", now); got != BeginDuplicate {
		t.Fatalf("Begin between Remember and End = %v, want BeginDuplicate", got)
	}
	guard.End("key-1")
	if got := guard.Begin("key-1", now); got != BeginDuplicate {
		t.Fatalf("Begin after Remember and End = %v, want BeginDuplicate", got)
	}
}

func TestReplayGuardOnNilReceiverIsASafeNoOp(t *testing.T) {
	var guard *ReplayGuard
	now := time.Now()
	if guard.Seen("key-1", now) {
		t.Fatal("a nil guard must be a safe no-op, never claim a duplicate")
	}
	guard.Remember("key-1", now)
	if got := guard.Begin("key-1", now); got != BeginAcquired {
		t.Fatalf("nil guard Begin = %v, want BeginAcquired so processing proceeds", got)
	}
	guard.End("key-1")
}

func TestReplayGuardEmptyKeyIsNeverDeduplicated(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.Remember("", now)
	for attempt := 1; attempt <= 2; attempt++ {
		if got := guard.Begin("", now); got != BeginAcquired {
			t.Fatalf("Begin #%d for an empty key = %v, want BeginAcquired", attempt, got)
		}
	}
	if guard.Seen("", now) {
		t.Fatal("an empty key has nothing to dedupe on and must never be seen")
	}
}
