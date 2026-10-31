package siem

import (
	"testing"
	"time"
)

func TestBackoffDoublesFromFiveSecondsToAFifteenMinuteCap(t *testing.T) {
	want := map[int]time.Duration{
		0: 0, 1: 5 * time.Second, 2: 10 * time.Second, 3: 20 * time.Second, 4: 40 * time.Second,
		5: 80 * time.Second, 6: 160 * time.Second, 7: 320 * time.Second, 8: 640 * time.Second,
		9: 15 * time.Minute, 10: 15 * time.Minute, 1000: 15 * time.Minute,
	}
	for failures, delay := range want {
		if got := Backoff(failures); got != delay {
			t.Errorf("Backoff(%d) = %v, want %v", failures, got, delay)
		}
	}
	if Backoff(-3) != 0 {
		t.Errorf("negative failures must not back off")
	}
}
