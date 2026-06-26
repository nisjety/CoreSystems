// Package pricing turns a reported model name + token counts into a USD cost.
//
// cost-core records what it is told, but most usage events arrive carrying only
// token counts (the gateway publishes tokens; cost was historically left at 0,
// which made the dollar ledger — and therefore the budget posture — always
// read $0). The resolver closes that gap: it loads a price catalogue
// (model_pricing table, seeded by migration 0002) and computes cost when a
// usage event has none.
//
// Matching is forgiving so a concrete deployment name still prices: an exact
// match wins, else the longest model key that is a prefix of the reported name
// (so `claude-sonnet-4-6` → `claude-sonnet`, `gpt-4o-mini-2024-07-18` →
// `gpt-4o-mini`), else the mandatory `default` row. The gateway mirrors this
// logic for the SSE display value off the same catalogue (GET /api/v1/pricing),
// so the streamed cost and the ledgered cost agree.
package pricing

import (
	"context"
	"sort"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// DefaultModelKey is the mandatory fallback key. It is always present so an
// unrecognised model is priced (at a mid tier) rather than counted as free.
const DefaultModelKey = "default"

// Rate is the price for one model key, in USD per 1,000,000 tokens.
type Rate struct {
	Model            string  `json:"model"`
	InputPerMillion  float64 `json:"input_per_million"`
	OutputPerMillion float64 `json:"output_per_million"`
	Currency         string  `json:"currency"`
}

// Resolver maps a model name to a Rate and computes USD cost. It is read-only
// after construction (safe for concurrent use).
type Resolver struct {
	rates map[string]Rate // keyed by normalized model key
	def   Rate            // the DefaultModelKey rate
}

// defaultSeed mirrors migrations/0002_model_pricing.up.sql so the in-memory /
// connect-failure path prices identically to the durable catalogue.
var defaultSeed = []Rate{
	{Model: DefaultModelKey, InputPerMillion: 3.00, OutputPerMillion: 15.00},
	{Model: "gpt-4o-mini", InputPerMillion: 0.15, OutputPerMillion: 0.60},
	{Model: "gpt-4o", InputPerMillion: 2.50, OutputPerMillion: 10.00},
	{Model: "gpt-4.1-mini", InputPerMillion: 0.40, OutputPerMillion: 1.60},
	{Model: "gpt-4.1", InputPerMillion: 2.00, OutputPerMillion: 8.00},
	{Model: "o4-mini", InputPerMillion: 1.10, OutputPerMillion: 4.40},
	{Model: "o3", InputPerMillion: 2.00, OutputPerMillion: 8.00},
	{Model: "model-router", InputPerMillion: 2.00, OutputPerMillion: 8.00},
	{Model: "claude-haiku", InputPerMillion: 0.80, OutputPerMillion: 4.00},
	{Model: "claude-3-5-haiku", InputPerMillion: 0.80, OutputPerMillion: 4.00},
	{Model: "claude-sonnet", InputPerMillion: 3.00, OutputPerMillion: 15.00},
	{Model: "claude-3-5-sonnet", InputPerMillion: 3.00, OutputPerMillion: 15.00},
	{Model: "claude-opus", InputPerMillion: 15.00, OutputPerMillion: 75.00},
	{Model: "claude-3-opus", InputPerMillion: 15.00, OutputPerMillion: 75.00},
}

// Default builds a resolver from the built-in seed. Used when no database is
// configured and as a fallback when loading the catalogue fails.
func Default() *Resolver {
	return fromRates(defaultSeed)
}

// LoadFromPool reads the model_pricing catalogue from Postgres. If the table is
// empty or missing a default row, the built-in seed backfills it so a default
// rate is always available.
func LoadFromPool(ctx context.Context, pool *pgxpool.Pool) (*Resolver, error) {
	rows, err := pool.Query(ctx, `
		SELECT model, input_per_million::double precision,
		       output_per_million::double precision, currency
		FROM model_pricing`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var loaded []Rate
	for rows.Next() {
		var r Rate
		if err := rows.Scan(&r.Model, &r.InputPerMillion, &r.OutputPerMillion, &r.Currency); err != nil {
			return nil, err
		}
		loaded = append(loaded, r)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	// Backfill from the seed for any key the DB lacks (notably `default`).
	seen := make(map[string]struct{}, len(loaded))
	for _, r := range loaded {
		seen[normalize(r.Model)] = struct{}{}
	}
	for _, r := range defaultSeed {
		if _, ok := seen[normalize(r.Model)]; !ok {
			loaded = append(loaded, r)
		}
	}
	return fromRates(loaded), nil
}

func fromRates(rates []Rate) *Resolver {
	r := &Resolver{rates: make(map[string]Rate, len(rates))}
	for _, rate := range rates {
		if rate.Currency == "" {
			rate.Currency = "USD"
		}
		key := normalize(rate.Model)
		rate.Model = key
		r.rates[key] = rate
		if key == DefaultModelKey {
			r.def = rate
		}
	}
	if r.def.Model == "" {
		r.def = Rate{Model: DefaultModelKey, InputPerMillion: 3.00, OutputPerMillion: 15.00, Currency: "USD"}
		r.rates[DefaultModelKey] = r.def
	}
	return r
}

// Cost returns the USD cost of an inference given its model and token counts.
// Negative token counts are clamped to zero.
func (r *Resolver) Cost(model string, inputTokens, outputTokens int64) float64 {
	rate := r.lookup(model)
	in := float64(max64(inputTokens, 0))
	out := float64(max64(outputTokens, 0))
	return in/1_000_000.0*rate.InputPerMillion + out/1_000_000.0*rate.OutputPerMillion
}

// lookup resolves a model name to a Rate: exact key, else longest prefix key,
// else the default.
func (r *Resolver) lookup(model string) Rate {
	key := normalize(model)
	if key == "" {
		return r.def
	}
	if rate, ok := r.rates[key]; ok {
		return rate
	}
	var best Rate
	bestLen := -1
	for candidate, rate := range r.rates {
		if candidate == DefaultModelKey {
			continue
		}
		if strings.HasPrefix(key, candidate) && len(candidate) > bestLen {
			best = rate
			bestLen = len(candidate)
		}
	}
	if bestLen >= 0 {
		return best
	}
	return r.def
}

// Rates returns the full catalogue, sorted by model key (default first), for
// the GET /api/v1/pricing endpoint.
func (r *Resolver) Rates() []Rate {
	out := make([]Rate, 0, len(r.rates))
	for _, rate := range r.rates {
		out = append(out, rate)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Model == DefaultModelKey {
			return true
		}
		if out[j].Model == DefaultModelKey {
			return false
		}
		return out[i].Model < out[j].Model
	})
	return out
}

func normalize(model string) string {
	return strings.ToLower(strings.TrimSpace(model))
}

func max64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}
