package main

import (
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// T44: each run reason maps to one exit code. The usage and lock codes are
// fixed by the plan table, and no two of the eight codes share a number. Pure.
func TestExitCodesFollowReason(t *testing.T) {
	cases := []struct {
		name   string
		reason doffinsync.Reason
		want   int
	}{
		{"done", doffinsync.ReasonDone, 0},
		{"partial", doffinsync.ReasonPartial, 2},
		{"paused", doffinsync.ReasonPaused, 3},
		{"key_rejected", doffinsync.ReasonKeyRejected, 4},
		{"cancelled", doffinsync.ReasonCancelled, 5},
		{"failed", doffinsync.ReasonFailed, 6},
		{"unknown reason is a failure", doffinsync.Reason("unknown"), 6},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := exitCodeFor(c.reason); got != c.want {
				t.Errorf("exitCodeFor(%q) = %d, want %d", c.reason, got, c.want)
			}
		})
	}

	if exitUsage != 1 {
		t.Errorf("exitUsage = %d, want 1", exitUsage)
	}
	if exitLocked != 7 {
		t.Errorf("exitLocked = %d, want 7", exitLocked)
	}

	named := map[string]int{
		"done":      exitDone,
		"partial":   exitPartial,
		"paused":    exitPaused,
		"key":       exitKeyRejected,
		"cancelled": exitCancelled,
		"failed":    exitFailed,
		"usage":     exitUsage,
		"lock":      exitLocked,
	}
	byCode := map[int]string{}
	for name, code := range named {
		if other, dup := byCode[code]; dup {
			t.Errorf("exit code %d is shared by %s and %s", code, other, name)
		}
		byCode[code] = name
	}
}
