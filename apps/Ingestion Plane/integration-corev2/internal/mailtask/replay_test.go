package mailtask

import (
	"testing"
	"time"
)

func TestReplayGuardSeenBeforeDetectsARepeat(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	if guard.SeenBefore("key-1", now) {
		t.Fatal("first sighting must not be reported as a repeat")
	}
	if !guard.SeenBefore("key-1", now.Add(time.Minute)) {
		t.Fatal("second sighting within the TTL window must be reported as a repeat")
	}
}

func TestReplayGuardForgetsAfterTTLExpires(t *testing.T) {
	guard := NewReplayGuard(time.Minute)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.SeenBefore("key-1", now)
	if guard.SeenBefore("key-1", now.Add(2*time.Minute)) {
		t.Fatal("an id past its TTL must be treated as unseen again")
	}
}

func TestReplayGuardDistinguishesKeys(t *testing.T) {
	guard := NewReplayGuard(time.Hour)
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)

	guard.SeenBefore("key-1", now)
	if guard.SeenBefore("key-2", now) {
		t.Fatal("a different key must not collide with an unrelated one")
	}
}

func TestReplayGuardOnNilReceiverNeverReportsARepeat(t *testing.T) {
	var guard *ReplayGuard
	if guard.SeenBefore("key-1", time.Now()) {
		t.Fatal("a nil guard must be a safe no-op, never claim a duplicate")
	}
}
