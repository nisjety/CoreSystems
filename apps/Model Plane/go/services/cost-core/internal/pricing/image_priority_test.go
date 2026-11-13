package pricing

import (
	"math"
	"os"
	"strings"
	"testing"
	"time"
)

// W4-IMG-2 (per-image prices) and FU-24 (priority-tier multiplier) on the resolver.

func imageResolver(rates ...ImageRate) *Resolver {
	r := Default()
	r.images = indexImageRates(rates)
	return r
}

func imageRate(model, size, quality string, usd float64) ImageRate {
	return ImageRate{Model: model, Size: size, Quality: quality, USDPerImage: usd, Source: "sheet-2026-11", EffectiveFrom: time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)}
}

func TestTheBuiltInCatalogueHasNoImagePrice(t *testing.T) {
	if usd, known := Default().ImageCost("gpt-image-1", "1024x1024", "high", 2); known || usd != 0 {
		t.Fatalf("an image was priced with no row: %v %t", usd, known)
	}
}

func TestImageCost(t *testing.T) {
	r := imageResolver(
		imageRate("gpt-image-1", "1024x1024", "high", 0.17),
		imageRate("gpt-image-1", "1024x1024", "", 0.04),
		imageRate("gpt-image-1", "1536x1024", "low", 0.016),
	)
	cases := []struct {
		name, model, size, quality string
		n                          int
		want                       float64
		known                      bool
	}{
		{"exact row", "gpt-image-1", "1024x1024", "high", 3, 0.51, true},
		{"case and space are normalised", " GPT-Image-1 ", "1024X1024", "HIGH", 1, 0.17, true},
		{"unknown quality falls back to the any-quality row", "gpt-image-1", "1024x1024", "medium", 2, 0.08, true},
		{"empty quality takes the any-quality row", "gpt-image-1", "1024x1024", "", 1, 0.04, true},
		{"no any-quality row for this size", "gpt-image-1", "1536x1024", "high", 1, 0, false},
		{"unknown size", "gpt-image-1", "512x512", "high", 1, 0, false},
		{"unknown model never takes the default", "dall-e-9", "1024x1024", "high", 1, 0, false},
		{"no prefix match", "gpt-image-1-mini", "1024x1024", "high", 1, 0, false},
		{"negative count prices nothing", "gpt-image-1", "1024x1024", "high", -4, 0, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			usd, known := r.ImageCost(tc.model, tc.size, tc.quality, tc.n)
			if known != tc.known || math.Abs(usd-tc.want) > 1e-9 {
				t.Fatalf("ImageCost = %v %t, want %v %t", usd, known, tc.want, tc.known)
			}
		})
	}
}

// Two stored keys that differ only in case would be one key after normalisation: which price
// applies would be a guess, so the key stays unpriced.
func TestAnAmbiguousImageKeyIsUnpriced(t *testing.T) {
	r := imageResolver(
		imageRate("gpt-image-1", "1024x1024", "high", 0.17),
		imageRate("GPT-IMAGE-1", "1024x1024", "high", 0.99),
		imageRate("gpt-image-1", "1024x1024", "", 0.04),
	)
	// The ambiguous exact key is dropped; the any-quality row then answers for it.
	usd, known := r.ImageCost("gpt-image-1", "1024x1024", "high", 1)
	if !known || usd != 0.04 {
		t.Fatalf("ambiguous key = %v %t; want the any-quality row 0.04", usd, known)
	}
}

func TestPriorityTierMultiplier(t *testing.T) {
	cases := map[string]float64{
		"":      2,
		"  ":    2,
		"abc":   2,
		"NaN":   2,
		"Inf":   2,
		"-Inf":  2,
		"1.5":   1.5,
		" 3 ":   3,
		"0.5":   1,
		"-2":    1,
		"9":     5,
		"5":     5,
		"1":     1,
		"2.25":  2.25,
		"1e309": 2,
	}
	for raw, want := range cases {
		if got := PriorityTierMultiplier(raw); got != want {
			t.Errorf("PriorityTierMultiplier(%q) = %v, want %v", raw, got, want)
		}
	}
	if PriorityTierCostMultiplierEnv != "PRIORITY_TIER_COST_MULTIPLIER" {
		t.Fatalf("env name %q differs from model-gateway's", PriorityTierCostMultiplierEnv)
	}
}

// 0012 creates the table and seeds nothing: a price is an owner input, never invented here, and the
// migration re-runs at every boot.
func TestImagePriceMigrationSeedsNoRowAndReapplies(t *testing.T) {
	raw, err := os.ReadFile("../../migrations/0012_image_pricing.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToUpper(string(raw))
	if strings.Contains(sql, "INSERT") {
		t.Fatal("0012 seeds an image price")
	}
	if !strings.Contains(sql, "CREATE TABLE IF NOT EXISTS IMAGE_PRICE_RATES") {
		t.Fatal("0012 is not re-run safe")
	}
}
