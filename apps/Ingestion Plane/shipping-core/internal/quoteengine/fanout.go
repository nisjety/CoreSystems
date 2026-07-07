// Package quoteengine fans a single QuoteRequest out to every registered
// carrier.Adapter concurrently, enforcing an independent timeout per
// carrier so one slow or failing carrier never delays or blocks the
// others — the mechanism behind the PRD's sub-5-second comparison NFR.
package quoteengine

import (
	"context"
	"time"

	"golang.org/x/sync/errgroup"

	"shipping-core/internal/carrier"
)

// Quoter is the subset of carrier.Adapter this package depends on. Defined
// here, at the consumer, rather than depending on the full Adapter
// interface — quoteengine never books, labels, or tracks, so it shouldn't
// couple to those methods.
type Quoter interface {
	Info() carrier.Info
	Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error)
}

// Result is one carrier's outcome for a single fan-out call. Exactly one
// of Quotes or Err is meaningful: Err set means the lookup itself failed
// (timeout, upstream error); Err nil with an empty Quotes means the
// carrier had nothing to offer for this request (e.g. wrong segment).
type Result struct {
	CarrierCode string
	Quotes      []carrier.Quote
	Err         error
	Duration    time.Duration
}

// Engine holds the set of carriers to fan a request out to and the
// per-carrier timeout budget.
type Engine struct {
	adapters          []Quoter
	perCarrierTimeout time.Duration
}

// New builds an Engine. perCarrierTimeout bounds each carrier's Quote call
// independently; since all calls run concurrently, GetQuotes' overall
// wall-clock is bounded by this timeout regardless of carrier count.
func New(adapters []Quoter, perCarrierTimeout time.Duration) *Engine {
	return &Engine{adapters: adapters, perCarrierTimeout: perCarrierTimeout}
}

// Carriers returns the Info of every registered adapter, in registration
// order — the live fleet as assembled from configured credentials.
func (e *Engine) Carriers() []carrier.Info {
	infos := make([]carrier.Info, 0, len(e.adapters))
	for _, a := range e.adapters {
		infos = append(infos, a.Info())
	}
	return infos
}

// GetQuotes calls every adapter's Quote concurrently and returns one
// Result per adapter, in adapter-registration order, once all have either
// completed or hit their individual timeout. A plain errgroup.Group (not
// WithContext) is used deliberately: each goroutine always returns nil so
// errgroup's shared-cancellation-on-error behavior never triggers — one
// carrier's failure must never cancel another carrier's in-flight call.
func (e *Engine) GetQuotes(ctx context.Context, req carrier.QuoteRequest) []Result {
	results := make([]Result, len(e.adapters))

	var g errgroup.Group
	for i, adapter := range e.adapters {
		g.Go(func() error {
			results[i] = e.quoteOne(ctx, adapter, req)
			return nil
		})
	}
	_ = g.Wait()

	return results
}

func (e *Engine) quoteOne(ctx context.Context, adapter Quoter, req carrier.QuoteRequest) Result {
	start := time.Now()
	cctx, cancel := context.WithTimeout(ctx, e.perCarrierTimeout)
	defer cancel()

	quotes, err := adapter.Quote(cctx, req)
	return Result{
		CarrierCode: adapter.Info().Code,
		Quotes:      quotes,
		Err:         err,
		Duration:    time.Since(start),
	}
}
