package quoteengine

import (
	"context"
	"errors"
	"testing"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/carrier/mock"
)

func testRequest() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From:    carrier.Address{PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:      carrier.Address{PostalCode: "7010", City: "Trondheim", Country: "NO"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

func TestEngine_GetQuotes_AllSucceed(t *testing.T) {
	adapters := toQuoters(mock.DefaultCarriers())
	engine := New(adapters, time.Second)

	results := engine.GetQuotes(context.Background(), testRequest())

	if len(results) != len(adapters) {
		t.Fatalf("got %d results, want %d", len(results), len(adapters))
	}
	for _, r := range results {
		if r.Err != nil {
			t.Errorf("carrier %s: unexpected error: %v", r.CarrierCode, r.Err)
		}
	}
}

func TestEngine_GetQuotes_SlowCarrierDoesNotBlockOthers(t *testing.T) {
	slow := &mockQuoter{code: "slow", latency: 500 * time.Millisecond}
	fastA := &mockQuoter{code: "fast-a"}
	fastB := &mockQuoter{code: "fast-b"}

	engine := New([]Quoter{slow, fastA, fastB}, 50*time.Millisecond)

	start := time.Now()
	results := engine.GetQuotes(context.Background(), testRequest())
	elapsed := time.Since(start)

	if elapsed > 200*time.Millisecond {
		t.Errorf("GetQuotes took %v, want close to the 50ms per-carrier timeout (fan-out must not serialize)", elapsed)
	}

	byCode := indexByCarrier(results)
	if !errors.Is(byCode["slow"].Err, context.DeadlineExceeded) {
		t.Errorf("slow carrier: got err=%v, want context.DeadlineExceeded", byCode["slow"].Err)
	}
	if byCode["fast-a"].Err != nil || byCode["fast-b"].Err != nil {
		t.Errorf("fast carriers should not be affected by the slow one: a=%v b=%v",
			byCode["fast-a"].Err, byCode["fast-b"].Err)
	}
}

func TestEngine_GetQuotes_OneCarrierErrorDoesNotAffectOthers(t *testing.T) {
	failing := &mockQuoter{code: "failing", err: errors.New("upstream 500")}
	ok := &mockQuoter{code: "ok"}

	engine := New([]Quoter{failing, ok}, time.Second)
	results := engine.GetQuotes(context.Background(), testRequest())

	byCode := indexByCarrier(results)
	if byCode["failing"].Err == nil {
		t.Error("expected failing carrier to report its error")
	}
	if byCode["ok"].Err != nil {
		t.Errorf("unaffected carrier should succeed, got err=%v", byCode["ok"].Err)
	}
	if len(byCode["ok"].Quotes) == 0 {
		t.Error("unaffected carrier should still return its quote")
	}
}

func indexByCarrier(results []Result) map[string]Result {
	m := make(map[string]Result, len(results))
	for _, r := range results {
		m[r.CarrierCode] = r
	}
	return m
}

func toQuoters(adapters []carrier.Adapter) []Quoter {
	q := make([]Quoter, len(adapters))
	for i, a := range adapters {
		q[i] = a
	}
	return q
}

// mockQuoter is a minimal, test-local Quoter (independent of the mock
// package's Adapter) so fan-out timing/error behavior can be asserted
// without depending on mock.Adapter's own pricing logic.
type mockQuoter struct {
	code    string
	mode    carrier.Mode
	latency time.Duration
	err     error
}

func (m *mockQuoter) Info() carrier.Info {
	return carrier.Info{Code: m.code, Name: m.code, Segment: carrier.SegmentBoth, Mode: m.mode}
}

func (m *mockQuoter) Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error) {
	if m.latency > 0 {
		select {
		case <-time.After(m.latency):
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	if m.err != nil {
		return nil, m.err
	}
	return []carrier.Quote{{CarrierCode: m.code, CarrierName: m.code}}, nil
}
