package main

import (
	"os"
	"regexp"
	"testing"
)

// Plan 3.4 for COST_CORE_IMAGE_PRICING_ENABLED (W4-IMG-2): main reads it inline, so the parser is pinned on the source: every
// read is exactly `os.Getenv("COST_CORE_IMAGE_PRICING_ENABLED") == "true"`. The off path (image usage ignored, the envelope
// handled as before) is pinned by TestImageUsageIsIgnoredWhileImagePricingIsOff in image_priority_decode_test.go.
func TestImagePricingSwitchIsReadAsTheLiteralTrue(t *testing.T) {
	raw, err := os.ReadFile("main.go")
	if err != nil {
		t.Fatal(err)
	}
	reads := len(regexp.MustCompile(`os\.Getenv\("COST_CORE_IMAGE_PRICING_ENABLED"\)`).FindAllIndex(raw, -1))
	strict := len(regexp.MustCompile(`os\.Getenv\("COST_CORE_IMAGE_PRICING_ENABLED"\) == "true"`).FindAllIndex(raw, -1))
	if reads == 0 {
		t.Fatal("main.go no longer reads COST_CORE_IMAGE_PRICING_ENABLED; update this guard")
	}
	if reads != strict {
		t.Fatalf("%d of %d reads of COST_CORE_IMAGE_PRICING_ENABLED are not the strict literal", reads-strict, reads)
	}
}
