package collection

import (
	"testing"
	"time"
)

func TestMissingZeroExpiredAndRevokedAreDifferent(t *testing.T) {
	now := time.Date(2026, 10, 29, 12, 0, 0, 0, time.UTC)
	observed := now.Add(-time.Hour).Format(time.RFC3339)
	for _, tc := range []struct {
		status, reason, want string
		count                *int
		receipt              string
	}{
		{"not_loaded", "", "pending", N(0), ""},
		{"connected", "", "empty", N(0), observed},
		{"connected", "", "available", N(2), observed},
		{"unavailable", "authorization_required", "reconnect", N(99), observed},
		{"unavailable", "snapshot_expired", "stale", N(99), observed},
		{"connected", "", "stale", N(2), now.Add(-25 * time.Hour).Format(time.RFC3339)},
		{"no_data", "", "empty", nil, observed},
		{"connected", "", "failed", N(2), "not-a-receipt"},
		{"plan_required", "", "setup", nil, observed},
	} {
		d := Finish(Dataset{}, tc.status, tc.reason, tc.receipt, tc.count, 24*time.Hour, now)
		if d.Status != tc.want {
			t.Fatalf("%s/%s became %s", tc.status, tc.reason, d.Status)
		}
		if (d.Status == "pending" || d.Status == "reconnect" || tc.reason == "snapshot_expired") && d.Records != nil {
			t.Fatal("invented or disclosed absent records", d)
		}
		if tc.status == "connected" && tc.receipt == observed && d.Records == nil {
			t.Fatal("discarded measured zero or count", d)
		}
	}
}
