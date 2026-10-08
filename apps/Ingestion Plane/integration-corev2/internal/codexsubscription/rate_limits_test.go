package codexsubscription

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func TestPublicRateLimitWindowIsObservedAndDoesNotExposePlanMetadata(t *testing.T) {
	now := time.Now().UTC().Truncate(time.Second)
	reset := float64(now.Add(2 * time.Hour).Unix())
	minutes := float64(300)
	used := 41.5
	report := RateLimitReport{
		ObservedAt: now,
		Primary: publicRateLimitWindow(&codexRateLimitSnapshot{
			Primary: &codexRateLimitWindow{UsedPercent: used, WindowDurationMins: &minutes, ResetsAt: &reset},
		}, true),
	}
	if report.Primary == nil || report.Primary.UsedPercent != used || report.Primary.WindowMinutes == nil || *report.Primary.WindowMinutes != 300 {
		t.Fatalf("primary window = %#v", report.Primary)
	}
	if report.Primary.ResetsAt == nil || !report.Primary.ResetsAt.Equal(now.Add(2*time.Hour)) {
		t.Fatalf("reset = %v", report.Primary.ResetsAt)
	}
	encoded, err := json.Marshal(report)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(encoded), "planType") || strings.Contains(string(encoded), "limitId") {
		t.Fatalf("provider identifiers leaked into allowance response: %s", encoded)
	}
}

func TestPublicRateLimitWindowRejectsInvalidUsage(t *testing.T) {
	for _, used := range []float64{-1, 101} {
		window := publicRateLimitWindow(&codexRateLimitSnapshot{Primary: &codexRateLimitWindow{UsedPercent: used}}, true)
		if window != nil {
			t.Fatalf("usedPercent %v should be omitted, got %#v", used, window)
		}
	}
}
