package pulse

import (
	"math"
	"testing"
)

func TestClarityUXBoundary(t *testing.T) {
	number := func(v float64) *float64 { return &v }
	for _, ux := range []*ClarityUXMetrics{
		{AverageScrollDepth: number(101)}, {RageClickPercent: number(-1)},
		{ActiveSeconds: number(math.NaN())}, {TotalSeconds: number(math.Inf(1))},
		{ActiveSeconds: number(60), TotalSeconds: number(59)},
	} {
		if ValidClarityUX(ux) {
			t.Fatal("invalid UX accepted")
		}
	}
	if !ValidClarityUX(&ClarityUXMetrics{DeadClickPercent: number(0), AverageScrollDepth: number(30.27), ActiveSeconds: number(37), TotalSeconds: number(59)}) || !ValidClarityUX(nil) {
		t.Fatal("valid or historical snapshot rejected")
	}
	got := clarityFixtureService(t, 200, func(v map[string]any) {
		v["metrics"].(map[string]any)["ux"] = map[string]any{"averageScrollDepth": 30.27, "deadClickPercent": 0, "activeSeconds": 37, "totalSeconds": 59}
	}).GetClarity(t.Context(), "https://example.com/")
	if got.Metrics == nil || got.Metrics.UX == nil || *got.Metrics.UX.AverageScrollDepth != 30.27 || *got.Metrics.UX.DeadClickPercent != 0 {
		t.Fatal("UX did not reach Marketing projection")
	}
}
