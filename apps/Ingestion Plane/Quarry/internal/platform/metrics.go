package platform

import (
	"context"
	"fmt"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

const meterName = "quarry"

// Metrics holds the named OTel instruments used throughout Quarry.
// Obtain a single instance via NewMetrics() during startup.
type Metrics struct {
	// HTTP / scrape throughput
	ScrapeRequests  metric.Int64Counter
	ScrapeErrors    metric.Int64Counter
	ScrapeLatencyMs metric.Int64Histogram

	// AI-core RPC
	AIRequests metric.Int64Counter
	AIErrors   metric.Int64Counter
	AITokens   metric.Int64Counter

	// Engine selection
	EngineSelected metric.Int64Counter // attributes: engine=<name>

	// Research jobs
	ResearchIterations metric.Int64Counter
	ResearchSources    metric.Int64Counter

	// Change tracking
	ChangesDetected metric.Int64Counter
}

// NewMetrics creates and registers all named metric instruments.
// Must be called after InitOTel so that the global MeterProvider is set.
func NewMetrics() (*Metrics, error) {
	m := otel.Meter(meterName)

	scrapeRequests, err := m.Int64Counter("quarry.scrape.requests",
		metric.WithDescription("Total scrape requests handled"))
	if err != nil {
		return nil, fmt.Errorf("metric scrape.requests: %w", err)
	}

	scrapeErrors, err := m.Int64Counter("quarry.scrape.errors",
		metric.WithDescription("Total scrape errors"))
	if err != nil {
		return nil, fmt.Errorf("metric scrape.errors: %w", err)
	}

	scrapeLatency, err := m.Int64Histogram("quarry.scrape.latency_ms",
		metric.WithDescription("Scrape latency in milliseconds"),
		metric.WithUnit("ms"))
	if err != nil {
		return nil, fmt.Errorf("metric scrape.latency_ms: %w", err)
	}

	aiRequests, err := m.Int64Counter("quarry.ai.requests",
		metric.WithDescription("Total ai-core RPC calls"))
	if err != nil {
		return nil, fmt.Errorf("metric ai.requests: %w", err)
	}

	aiErrors, err := m.Int64Counter("quarry.ai.errors",
		metric.WithDescription("Failed ai-core RPC calls"))
	if err != nil {
		return nil, fmt.Errorf("metric ai.errors: %w", err)
	}

	aiTokens, err := m.Int64Counter("quarry.ai.tokens",
		metric.WithDescription("Tokens consumed by ai-core RPCs"))
	if err != nil {
		return nil, fmt.Errorf("metric ai.tokens: %w", err)
	}

	engineSelected, err := m.Int64Counter("quarry.engine.selected",
		metric.WithDescription("Engine selection counts by engine name"))
	if err != nil {
		return nil, fmt.Errorf("metric engine.selected: %w", err)
	}

	researchIter, err := m.Int64Counter("quarry.research.iterations",
		metric.WithDescription("Research job iterations executed"))
	if err != nil {
		return nil, fmt.Errorf("metric research.iterations: %w", err)
	}

	researchSrc, err := m.Int64Counter("quarry.research.sources",
		metric.WithDescription("Unique sources discovered in research jobs"))
	if err != nil {
		return nil, fmt.Errorf("metric research.sources: %w", err)
	}

	changesDetected, err := m.Int64Counter("quarry.tracking.changes",
		metric.WithDescription("Content changes detected by change tracker"))
	if err != nil {
		return nil, fmt.Errorf("metric tracking.changes: %w", err)
	}

	return &Metrics{
		ScrapeRequests:     scrapeRequests,
		ScrapeErrors:       scrapeErrors,
		ScrapeLatencyMs:    scrapeLatency,
		AIRequests:         aiRequests,
		AIErrors:           aiErrors,
		AITokens:           aiTokens,
		EngineSelected:     engineSelected,
		ResearchIterations: researchIter,
		ResearchSources:    researchSrc,
		ChangesDetected:    changesDetected,
	}, nil
}

// nopMetrics returns a zero-value Metrics that uses no-op instruments so that
// callers can record metrics unconditionally without nil guards.
func nopMetrics() *Metrics {
	// Go zero-values for interface fields are nil; skip recording silently.
	return &Metrics{}
}

// RecordScrape records a completed scrape attempt.
func (met *Metrics) RecordScrape(ctx context.Context, latencyMs int64, success bool) {
	if met == nil {
		return
	}
	if met.ScrapeRequests != nil {
		met.ScrapeRequests.Add(ctx, 1)
	}
	if !success && met.ScrapeErrors != nil {
		met.ScrapeErrors.Add(ctx, 1)
	}
	if met.ScrapeLatencyMs != nil {
		met.ScrapeLatencyMs.Record(ctx, latencyMs)
	}
}
