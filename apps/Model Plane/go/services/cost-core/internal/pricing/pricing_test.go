package pricing

import (
	"math"
	"testing"
)

func approx(t *testing.T, got, want float64) {
	t.Helper()
	if math.Abs(got-want) > 1e-9 {
		t.Fatalf("got %.10f, want %.10f", got, want)
	}
}

func TestCostExactMatch(t *testing.T) {
	r := Default()
	// gpt-4o-mini: 0.15 in / 0.60 out per 1M.
	// 1000 in, 500 out → 1000/1e6*0.15 + 500/1e6*0.60 = 0.00015 + 0.0003 = 0.00045.
	approx(t, r.Cost("gpt-4o-mini", 1000, 500), 0.00045)
}

func TestCostPrefixMatch(t *testing.T) {
	r := Default()
	// claude-sonnet-4-6 has no exact row → longest prefix `claude-sonnet`
	// (3.00 / 15.00). 1_000_000 in + 1_000_000 out = 3 + 15 = 18.
	approx(t, r.Cost("claude-sonnet-4-6", 1_000_000, 1_000_000), 18.0)
	// A dated OpenAI deployment name resolves to gpt-4o-mini, not gpt-4o.
	approx(t, r.Cost("gpt-4o-mini-2024-07-18", 1_000_000, 0), 0.15)
}

func TestCostFallsBackToDefault(t *testing.T) {
	r := Default()
	// Unknown model → default (3.00 / 15.00), never zero.
	approx(t, r.Cost("some-future-model", 1_000_000, 0), 3.0)
}

func TestCostCaseInsensitiveAndClamped(t *testing.T) {
	r := Default()
	approx(t, r.Cost("GPT-4o-Mini", 1_000_000, 0), 0.15)
	// Negative tokens are clamped to zero.
	approx(t, r.Cost("gpt-4o-mini", -5, -5), 0.0)
}

func TestCostWithCacheZeroCacheTokensMatchesCost(t *testing.T) {
	r := Default()
	// Purely additive: every existing caller passes 0 cache tokens and must
	// see byte-for-byte the same figure Cost always returned.
	approx(t, r.CostWithCache("claude-sonnet", 1_000, 500, 0, 0), r.Cost("claude-sonnet", 1_000, 500))
}

func TestCostWithCacheAppliesTheTenXCacheReadDiscount(t *testing.T) {
	r := Default()
	// claude-sonnet: 3.00 / 15.00 per 1M. 8_520 input_tokens folds in an
	// 8_000-token cache read (Anthropic's total_input_tokens fold-in): 520
	// fresh tokens at the full rate, 8_000 cached tokens at a 10x discount.
	got := r.CostWithCache("claude-sonnet", 8_520, 42, 8_000, 400)
	want := 520.0/1_000_000.0*3.0 + // fresh input, full rate
		8_000.0/1_000_000.0*3.0*0.1 + // cached input, 10x discount
		42.0/1_000_000.0*15.0 // output, unaffected
	approx(t, got, want)
}

func TestRatesIncludesDefaultFirst(t *testing.T) {
	r := Default()
	rates := r.Rates()
	if len(rates) == 0 {
		t.Fatal("expected a non-empty catalogue")
	}
	if rates[0].Model != DefaultModelKey {
		t.Fatalf("expected default first, got %q", rates[0].Model)
	}
}

// Mirrors model-gateway pricing.rs `cache_rates_list_only_the_documented_openai_models`
// so the ledgered cost and the streamed display cost apply the same rates.
func TestCacheRatesMirrorModelGateway(t *testing.T) {
	flat := cacheRates{read: CacheReadDiscount, write: 1.0}
	half := cacheRates{read: 0.5, write: 1.0}
	gpt5 := cacheRates{read: 0.1, write: 1.0}
	gpt56 := cacheRates{read: 0.1, write: 1.25}
	for _, test := range []struct {
		model string
		want  cacheRates
	}{
		{"gpt-4o-mini", half},
		{"gpt-4o-mini-2024-07-18", half},
		{" GPT-4o-Mini ", half},
		{"gpt-5", gpt5},
		{"gpt-5-mini", gpt5},
		{"gpt-5-nano", gpt5},
		{"gpt-5.2", gpt5},
		{"gpt-5.5", gpt5},
		{"gpt-5.6", gpt56},
		{"gpt-5.6-terra", gpt56},
		{"gpt-5.10", gpt56},
		// Not confirmed, so unchanged from the flat rate.
		{"gpt-4o", flat},
		{"gpt-4.1-mini", flat},
		{"o4-mini", flat},
		{"model-router", flat},
		{"claude-sonnet-4-6", flat},
		{"", flat},
	} {
		if got := cacheRatesFor(test.model); got != test.want {
			t.Errorf("cacheRatesFor(%q) = %+v, want %+v", test.model, got, test.want)
		}
	}
}

func TestCostWithCacheZeroCacheTokensMatchesCostForEveryCacheTier(t *testing.T) {
	r := Default()
	for _, model := range []string{"claude-sonnet-4-6", "gpt-4o-mini", "gpt-5.6-terra"} {
		approx(t, r.CostWithCache(model, 1_000, 500, 0, 0), r.Cost(model, 1_000, 500))
	}
}

// The numbers of pricing.rs `gpt_4o_mini_cache_reads_are_billed_at_half_the_input_rate`.
func TestCostWithCacheBillsGpt4oMiniCacheReadsAtHalfTheInputRate(t *testing.T) {
	r := Default()
	got := r.CostWithCache("gpt-4o-mini-2024-07-18", 1_566, 2, 1_408, 0)
	want := 158.0/1_000_000.0*0.15 + // fresh input
		1_408.0/1_000_000.0*0.15*0.5 + // cached input, half rate
		2.0/1_000_000.0*0.60
	approx(t, got, want)
}

// The numbers of pricing.rs `gpt_5_6_cache_writes_are_surcharged_to_one_and_a_quarter`.
// gpt-5.6 has no catalogue row, so it prices against `default` (3.00/15.00).
func TestCostWithCacheSurchargesGpt56CacheWrites(t *testing.T) {
	r := Default()
	// 2,000 prompt tokens: 1,024 read from the cache, 512 written to it, 464
	// plain input.
	got := r.CostWithCache("gpt-5.6", 2_000, 50, 1_024, 512)
	want := 464.0/1_000_000.0*3.0 +
		1_024.0/1_000_000.0*3.0*0.1 +
		512.0/1_000_000.0*3.0*1.25 +
		50.0/1_000_000.0*15.0
	approx(t, got, want)
}

func TestCostWithCacheWriteLegIsPlainInputForModelsThatDoNotBillWrites(t *testing.T) {
	r := Default()
	for _, model := range []string{"claude-sonnet-4-6", "gpt-5.2", "gpt-4o-mini"} {
		approx(t, r.CostWithCache(model, 2_000, 50, 0, 512), r.CostWithCache(model, 2_000, 50, 0, 0))
	}
}
