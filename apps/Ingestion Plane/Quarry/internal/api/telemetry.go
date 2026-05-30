package api

import (
	"sync/atomic"
	"time"
)

type Telemetry struct {
	startedAt     time.Time
	totalRequests int64
	errorRequests int64
	totalLatencyN int64
}

func NewTelemetry() *Telemetry { return &Telemetry{startedAt: time.Now()} }

func (t *Telemetry) Record(duration time.Duration, isError bool) {
	atomic.AddInt64(&t.totalRequests, 1)
	atomic.AddInt64(&t.totalLatencyN, duration.Nanoseconds())
	if isError {
		atomic.AddInt64(&t.errorRequests, 1)
	}
}

func (t *Telemetry) Snapshot() map[string]interface{} {
	total := atomic.LoadInt64(&t.totalRequests)
	errors := atomic.LoadInt64(&t.errorRequests)
	latN := atomic.LoadInt64(&t.totalLatencyN)
	avgMs := 0.0
	if total > 0 {
		avgMs = float64(latN) / float64(total) / float64(time.Millisecond)
	}
	errorRate := 0.0
	if total > 0 {
		errorRate = float64(errors) / float64(total)
	}

	uptimeSec := time.Since(t.startedAt).Seconds()
	requestsPerSec := 0.0
	if uptimeSec > 0 {
		requestsPerSec = float64(total) / uptimeSec
	}

	return map[string]interface{}{
		"requests_total": total,
		"errors_total":   errors,
		"error_rate":     errorRate,
		"avg_latency_ms": avgMs,
		"requests_per_sec": requestsPerSec,
	}
}
