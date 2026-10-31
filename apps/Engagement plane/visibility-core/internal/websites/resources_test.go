package websites

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestResourceSelectionIsFreshScopedAndPreservedDuringDiscovery(t *testing.T) {
	s := newTestService(t, func(context.Context, string) Evidence {
		return Evidence{SearchConsole: Access{Status: "not_found"}, GA4: Access{Status: "needs_selection"}}
	})
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	id := "res_" + strings.Repeat("a", 32)
	choices := []ResourceChoice{{ID: id, Provider: "ga4", Label: "GA4 123 / 456", Access: Access{Status: "connected", ConnectionID: "conn-google", PropertyID: "123", StreamID: "456", MeasurementID: "G-ABCDE12345"}}}
	calls := 0
	s.SetResourceActions(func(_ context.Context, site string) (ResourceReport, error) {
		calls++
		if site != w.Site {
			t.Error("wrong site")
		}
		return ResourceReport{Status: "ready", Choices: choices}, nil
	})
	report, err := s.GoogleResources(t.Context(), w.ID)
	if err != nil || report.WebsiteID != w.ID {
		t.Fatal(report, err)
	}
	raw, _ := json.Marshal(report)
	if strings.Contains(string(raw), "conn-google") || strings.Contains(string(raw), "Access") {
		t.Fatal("private connection exposed")
	}
	w, err = s.SelectGoogleResource(t.Context(), w.ID, id, w.Revision)
	if err != nil || calls != 2 || w.Integrations.GA4.PropertyID != "123" {
		t.Fatal(w, err, calls)
	}
	w, err = s.Verify(t.Context(), w.ID)
	if err != nil || w.Integrations.GA4.Status != "connected" {
		t.Fatal(w, err)
	}
	choices = []ResourceChoice{}
	w, err = s.Verify(t.Context(), w.ID)
	if err != nil || w.Integrations.GA4.Status == "connected" || w.Integrations.GA4.PropertyID != "" {
		t.Fatal("revoked choice survived", w, err)
	}
	if _, err = s.SelectGoogleResource(t.Context(), w.ID, id, w.Revision); err == nil {
		t.Fatal("stale choice reused")
	}
}
