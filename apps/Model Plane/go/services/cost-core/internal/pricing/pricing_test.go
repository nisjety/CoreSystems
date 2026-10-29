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
