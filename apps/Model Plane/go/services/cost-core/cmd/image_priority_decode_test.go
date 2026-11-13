package main

import (
	"strings"
	"testing"
	"time"
)

// FU-24 and W4-IMG-2 on the usage decoder.

func usageWith(payload string) []byte {
	return []byte(`{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","correlation_id":"req-1","idempotency_key":"req-1-USAGE_ENVELOPE","org_id":"org-a","user_id":"user-a","payload":{"request_id":"req-1","org_id":"org-a","user_id":"user-a","model":"gpt-image-1"` + payload + `}}`)
}

func TestServiceTierUsedPriorityMarksTheEntry(t *testing.T) {
	for payload, want := range map[string]bool{
		``:                                false,
		`,"service_tier_used":"priority"`: true,
		`,"service_tier_used":"Priority"`: false,
		`,"service_tier_used":"default"`:  false,
		`,"service_tier_used":""`:         false,
	} {
		entry, err := decodeUsageMessage("mp.v1.usage.org-a", usageWith(`,"input_tokens":10`+payload), time.Now())
		if err != nil {
			t.Fatalf("%q: %v", payload, err)
		}
		if entry.PriorityTier != want {
			t.Errorf("%q: PriorityTier = %t, want %t", payload, entry.PriorityTier, want)
		}
	}
}

func TestImageUsageIsIgnoredWhileImagePricingIsOff(t *testing.T) {
	data := usageWith(`,"kind":"image_generation","size":"1024x1024","quality":"high","n":2`)
	if _, isImage, err := imageUsageOf(data, false); isImage || err != nil {
		t.Fatalf("image pricing off: isImage=%t err=%v; want the event recorded as before", isImage, err)
	}
	// Off, even a malformed image event is not refused by the image path.
	if _, isImage, err := imageUsageOf(usageWith(`,"kind":"image_generation","n":0`), false); isImage || err != nil {
		t.Fatalf("image pricing off refused an event: %v", err)
	}
	// And the decoder itself accepts it as an ordinary usage event.
	if _, err := decodeUsageMessage("mp.v1.usage.org-a", data, time.Now()); err != nil {
		t.Fatalf("an image event no longer decodes as usage: %v", err)
	}
}

func TestImageUsageIsReadWhileImagePricingIsOn(t *testing.T) {
	image, isImage, err := imageUsageOf(usageWith(`,"kind":" Image_Generation ","size":"1024x1024","quality":"high","n":3`), true)
	if err != nil || !isImage || image.Size != "1024x1024" || image.Quality != "high" || image.Count != 3 {
		t.Fatalf("image=%+v isImage=%t err=%v", image, isImage, err)
	}
	for name, payload := range map[string]string{
		"no kind":     ``,
		"other kind":  `,"kind":"chat"`,
		"kind number": `,"kind":7`,
	} {
		if _, isImage, err := imageUsageOf(usageWith(payload), true); isImage || err != nil {
			t.Errorf("%s: isImage=%t err=%v; want an ordinary usage event", name, isImage, err)
		}
	}
	for name, payload := range map[string]string{
		"zero images":       `,"kind":"image_generation","n":0`,
		"too many images":   `,"kind":"image_generation","n":101`,
		"negative images":   `,"kind":"image_generation","n":-1`,
		"count as text":     `,"kind":"image_generation","n":"2"`,
		"oversized size":    `,"kind":"image_generation","n":1,"size":"` + strings.Repeat("x", 300) + `"`,
		"oversized quality": `,"kind":"image_generation","n":1,"quality":"` + strings.Repeat("q", 300) + `"`,
	} {
		if _, _, err := imageUsageOf(usageWith(payload), true); err == nil {
			t.Errorf("%s: a malformed image event was accepted", name)
		}
	}
}
