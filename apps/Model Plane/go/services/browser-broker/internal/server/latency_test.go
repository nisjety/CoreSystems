package server

import (
	"context"
	"fmt"
	"sort"
	"testing"
	"time"
)

// TestAcquireGrant_P95Latency asserts that the p95 latency of AcquireGrant
// stays below 200 ms — the Phase F streaming first-token threshold.
func TestAcquireGrant_P95Latency(t *testing.T) {
	const (
		iterations = 100
		p95Target  = 200 * time.Millisecond
	)

	srv := newTestServer()
	ctx := context.Background()

	durations := make([]time.Duration, 0, iterations)

	for i := 0; i < iterations; i++ {
		req := &AcquireGrantRequest{
			OrgId:      "org1",
			SessionKey: fmt.Sprintf("session-lat-%d", i),
			Mode:       "cloud",
			AllowedDomains: []string{
				"example.com",
			},
		}
		start := time.Now()
		_, err := srv.AcquireGrant(ctx, req)
		elapsed := time.Since(start)
		if err != nil {
			t.Fatalf("AcquireGrant iteration %d: unexpected error: %v", i, err)
		}
		durations = append(durations, elapsed)
	}

	sort.Slice(durations, func(i, j int) bool { return durations[i] < durations[j] })

	// p95 index: 95th percentile (0-based index = ceil(0.95 * N) - 1)
	p95Index := int(float64(iterations)*0.95) - 1
	if p95Index < 0 {
		p95Index = 0
	}
	p95 := durations[p95Index]

	t.Logf("AcquireGrant latency over %d calls — p50: %v, p95: %v, p99: %v",
		iterations,
		durations[int(float64(iterations)*0.50)-1],
		p95,
		durations[int(float64(iterations)*0.99)-1],
	)

	if p95 >= p95Target {
		t.Errorf("p95 latency %v exceeds target %v", p95, p95Target)
	}
}
