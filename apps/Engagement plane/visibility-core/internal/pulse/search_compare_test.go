package pulse

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestComparisonPeriodsRequireEqualOrderedBoundedCompletedRanges(t *testing.T) {
	now := time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC)
	validCurrent, validPrevious, err := ComparisonPeriods("2026-09-24", "2026-10-21", "2026-08-27", "2026-09-23", now)
	if err != nil || validCurrent.StartDate != "2026-09-24" || validPrevious.EndDate != "2026-09-23" {
		t.Fatalf("valid periods=%#v %#v err=%v", validCurrent, validPrevious, err)
	}
	for name, values := range map[string][4]string{
		"invalid date":      {"2026-02-30", "2026-03-01", "2026-02-01", "2026-02-28"},
		"future":            {"2026-10-22", "2026-10-22", "2026-09-20", "2026-09-21"},
		"overlap":           {"2026-09-20", "2026-10-10", "2026-09-01", "2026-09-21"},
		"different lengths": {"2026-09-24", "2026-10-21", "2026-08-27", "2026-09-22"},
		"too long":          {"2026-06-01", "2026-10-21", "2026-01-01", "2026-05-23"},
		"missing":           {"", "2026-10-21", "2026-08-27", "2026-09-23"},
	} {
		t.Run(name, func(t *testing.T) {
			if _, _, err := ComparisonPeriods(values[0], values[1], values[2], values[3], now); err == nil {
				t.Fatal("expected invalid comparison periods")
			}
		})
	}
}

func TestSearchComparisonUsesVerifiedPropertyAndMarksUnmatchedRowsUnknown(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var action struct {
			Operation string            `json:"operation"`
			Params    map[string]string `json:"params"`
			Body      struct {
				StartDate  string   `json:"startDate"`
				EndDate    string   `json:"endDate"`
				Dimensions []string `json:"dimensions"`
				RowLimit   int      `json:"rowLimit"`
				DataState  string   `json:"dataState"`
			} `json:"body"`
		}
		if err := json.NewDecoder(r.Body).Decode(&action); err != nil {
			t.Error(err)
			return
		}
		if action.Operation == "gsc.sites.list" {
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteOwner"}]}}}}`))
			return
		}
		if action.Operation != "gsc.search_analytics.query" || action.Params["siteUrl"] != "sc-domain:example.com" || len(action.Body.Dimensions) != 1 || action.Body.Dimensions[0] != "page" || action.Body.RowLimit != 500 || action.Body.DataState != "final" {
			t.Errorf("unexpected action %#v", action)
			return
		}
		rows := `[{"keys":["https://example.com/retained"],"clicks":3,"impressions":100,"ctr":0.03,"position":5}]`
		if action.Body.StartDate == "2026-08-26" {
			rows = `[{"keys":["https://example.com/retained"],"clicks":9,"impressions":300,"ctr":0.03,"position":3},{"keys":["https://example.com/unmatched"],"clicks":4,"impressions":80,"ctr":0.05,"position":8}]`
		} else if action.Body.StartDate != "2026-09-23" || action.Body.EndDate != "2026-10-20" {
			t.Errorf("unexpected range %#v", action.Body)
		}
		_, _ = fmt.Fprintf(w, `{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.search_analytics.query","result":{"rows":%s}}}}`, rows)
	}))
	defer provider.Close()
	service := newTestService(t, provider.URL, "sc-domain:example.com")
	got := service.GetSearchComparison(context.Background(), "https://example.com/", "page", Period{"2026-09-23", "2026-10-20"}, Period{"2026-08-26", "2026-09-22"}, 500)
	if got.Status != StatusConnected || len(got.Rows) != 2 || got.RowLimit != 500 || got.RowsAtLimit {
		t.Fatalf("comparison=%#v", got)
	}
	retained := got.Rows[0]
	if retained.Key != "https://example.com/retained" || retained.ClicksChange == nil || *retained.ClicksChange != -6 || retained.PositionChange == nil || *retained.PositionChange != 2 || retained.State != "compared" {
		t.Fatalf("retained row=%#v", retained)
	}
	if got.Rows[1].State != "not_comparable" || got.Rows[1].ClicksChange != nil {
		t.Fatalf("unmatched row inferred a zero: %#v", got.Rows[1])
	}
}
