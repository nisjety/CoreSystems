package api

import (
	"fmt"
	"strings"
	"testing"
)

// The radar route answers in the data envelope with its window, what it read and the results.
func TestReTendersReturnsTheEnvelope(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/retenders", `{"profile":{"cpv":["72000000"]},"horizon_months":6}`, nil)
	if status != 200 {
		t.Fatalf("status %d: %s", status, raw)
	}
	data, _ := decodeMap(t, raw)["data"].(map[string]any)
	if data == nil {
		t.Fatalf("no data envelope: %s", raw)
	}
	for _, key := range []string{"as_of", "window", "scanned", "awards", "winners", "results"} {
		if _, ok := data[key]; !ok {
			t.Errorf("data has no %q", key)
		}
	}
	if data["winners"] != "not_configured" {
		t.Errorf("winners = %v, want not_configured without a register", data["winners"])
	}
}

func TestReTendersAnswersAnInvalidRequestWithTheFields(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/retenders", `{"horizon_months":99}`, nil)
	if code, _, _ := errorOf(t, raw); status != 400 || code != "validation_failed" {
		t.Errorf("status %d code %q, want 400 validation_failed: %s", status, code, raw)
	}
	big := `{"profile":{"keywords":["` + strings.Repeat("x", 70<<10) + `"]}}`
	if status, raw, _ := post(t, app, "/api/v1/demo/retenders", big, nil); status != 413 {
		t.Errorf("an oversized body: status %d %s", status, raw)
	}
}

// The slider is a field of the request. A known step is answered with its name, the total before the cap,
// and what each wider step would add with the dials it widens; the widest step has nothing to widen. An
// unknown step is refused as a validation problem on that field.
func TestReTendersAnswersTheSliderStep(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/retenders", `{"profile":{"cpv":["72000000"]},"strictness":"strict"}`, nil)
	data, _ := decodeMap(t, raw)["data"].(map[string]any)
	if status != 200 || data == nil || data["strictness"] != "strict" {
		t.Fatalf("status %d data %v: %s", status, data, raw)
	}
	if _, ok := data["matching"].(float64); !ok {
		t.Errorf("matching = %v, want the total before the cap", data["matching"])
	}
	widening, ok := data["widening"].([]any)
	if !ok || len(widening) != 2 {
		t.Fatalf("widening = %v, want the balanced and the wide step", data["widening"])
	}
	for _, w := range widening {
		step, _ := w.(map[string]any)
		for _, key := range []string{"to", "adds", "dials"} {
			if _, ok := step[key]; !ok {
				t.Errorf("a widening step has no %q: %v", key, step)
			}
		}
	}
	status, raw, _ = post(t, app, "/api/v1/demo/retenders", `{"profile":{"cpv":["72000000"]},"strictness":"wide"}`, nil)
	data, _ = decodeMap(t, raw)["data"].(map[string]any)
	if status != 200 || data == nil || data["strictness"] != "wide" {
		t.Fatalf("status %d data %v: %s", status, data, raw)
	}
	if widening, ok := data["widening"].([]any); !ok || len(widening) != 0 {
		t.Errorf("widening = %v, want an empty list at the widest step", data["widening"])
	}
	status, raw, _ = post(t, app, "/api/v1/demo/retenders", `{"strictness":"loose"}`, nil)
	if code, _, details := errorOf(t, raw); status != 400 || code != "validation_failed" || !strings.Contains(fmt.Sprint(details), "strictness") {
		t.Errorf("an unknown step: status %d code %q details %v", status, code, details)
	}
}
