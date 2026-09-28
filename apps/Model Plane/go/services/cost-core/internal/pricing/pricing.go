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
	"strconv"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// DefaultModelKey is the mandatory fallback key. It is always present so an
// unrecognised model is priced (at a mid tier) rather than counted as free.
const DefaultModelKey = "default"

// CacheReadDiscount is the fraction of the base input-token rate a
// prompt-cache read is billed at -- the documented 10x discount Anthropic
// and the GPT-5 family apply to a cache hit. It is the read rate for every
// model cacheRatesFor does not list, exactly as in the gateway's Rust
// pricing cache (services/model-gateway/src/pricing.rs), so the streamed
// display cost and this ledgered cost agree.
const CacheReadDiscount = 0.1

// cacheRates are the cache-leg prices for one model, as multiples of its
// base input rate. Both legs are already counted inside inputTokens at the
// full rate; these say what each leg should have cost instead.
type cacheRates struct {
	read  float64
	write float64
}

// cacheRatesFor mirrors model-gateway's pricing.rs `cache_rates`: the
// cache-leg prices for a reported model name. Keep the two in step.
//
// Only rates OpenAI documents are listed: gpt-4o-mini's cached input is half
// its input rate, the GPT-5 family's is a tenth, and GPT-5.6 and later also
// bill a cache write at 1.25x (earlier models do not charge for writes).
// Every other model keeps the flat CacheReadDiscount for reads and the plain
// input rate for writes. That includes gpt-4o, gpt-4.1, the o-series,
// model-router and Claude.
//
// Keyed on the model name the provider served, not on the catalogue row it
// priced against: a GPT-5 model with no row of its own resolves to
// `default` and still gets its cache rates.
func cacheRatesFor(model string) cacheRates {
	model = strings.ToLower(strings.TrimSpace(model))
	// Also matches dated snapshots such as `gpt-4o-mini-2024-07-18`.
	if strings.HasPrefix(model, "gpt-4o-mini") {
		return cacheRates{read: 0.5, write: 1.0}
	}
	if rest, ok := strings.CutPrefix(model, "gpt-5"); ok {
		minor := ""
		if afterDot, ok := strings.CutPrefix(rest, "."); ok {
			digits := 0
			for digits < len(afterDot) && afterDot[digits] >= '0' && afterDot[digits] <= '9' {
				digits++
			}
			minor = afterDot[:digits]
		}
		write := 1.0
		if n, err := strconv.ParseUint(minor, 10, 32); err == nil && n >= 6 {
			write = 1.25
		}
		return cacheRates{read: 0.1, write: write}
	}
	return cacheRates{read: CacheReadDiscount, write: 1.0}
}

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
//
// There are no gpt-5.x rows because the repo has no sourced GPT-5 list price:
// model-gateway's pricing cache reads this catalogue and keeps no table of
// its own. GPT-5 API models (gpt-5-mini, gpt-5-nano, gpt-5.2, ...) therefore
// price at `default` until a sourced row is added here and in 0002 together.
// Subscription models (gpt-5.6-terra and the like) are unaffected: they carry
// cost_basis "subscription" and are never priced.
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
	return r.CostWithCache(model, inputTokens, outputTokens, 0, 0)
}

// CostWithCache is [Resolver.Cost] plus prompt-cache telemetry: a cache-token
// telemetry prerequisite for the native-compaction migration, so cache-hit
// rate and its cost savings are visible on the durable ledger.
//
// cacheReadInputTokens and cacheCreationInputTokens are cache legs already
// folded into inputTokens by the serving adapter (Anthropic's
// `total_input_tokens` fold-in and OpenAI's `prompt_tokens`, mirrored on the
// Rust side) -- passing 0 for both reproduces Cost's result exactly. Each leg
// is re-priced at the model's cacheRatesFor rates, as model-gateway's
// `cost_from_rate` does: inputTokens is charged in full first, then the
// cache-read leg's over-charge is credited back down to the read rate, and
// the cache-write leg is surcharged up to the write rate (a no-op at the
// default 1.0, which adds exactly 0).
func (r *Resolver) CostWithCache(model string, inputTokens, outputTokens, cacheReadInputTokens, cacheCreationInputTokens int64) float64 {
	rate := r.lookup(model)
	cache := cacheRatesFor(model)
	in := float64(max64(inputTokens, 0))
	out := float64(max64(outputTokens, 0))
	cacheRead := float64(max64(cacheReadInputTokens, 0))
	cacheWrite := float64(max64(cacheCreationInputTokens, 0))
	return in/1_000_000.0*rate.InputPerMillion +
		out/1_000_000.0*rate.OutputPerMillion -
		cacheRead/1_000_000.0*rate.InputPerMillion*(1.0-cache.read) +
		cacheWrite/1_000_000.0*rate.InputPerMillion*(cache.write-1.0)
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
