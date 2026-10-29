package pulse

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func ga4ReportFixture(daily bool) map[string]any {
	headers := []any{}
	for _, name := range []string{"totalUsers", "sessions", "engagedSessions", "screenPageViews", "keyEvents", "userEngagementDuration"} {
		headers = append(headers, map[string]string{"name": name})
	}
	values := func(users string) []any {
		return []any{map[string]string{"value": users}, map[string]string{"value": "12"}, map[string]string{"value": "8"}, map[string]string{"value": "23"}, map[string]string{"value": "2.5"}, map[string]string{"value": "45.25"}}
	}
	result := map[string]any{"metricHeaders": headers, "dimensionHeaders": []any{}, "metadata": map[string]any{"timeZone": "Europe/Oslo"}, "rows": []any{map[string]any{"metricValues": values("8")}}, "rowCount": 1}
	if daily {
		result["dimensionHeaders"] = []any{map[string]string{"name": "date"}}
		result["rows"] = []any{map[string]any{"dimensionValues": []any{map[string]string{"value": "20261001"}}, "metricValues": values("6")}, map[string]any{"dimensionValues": []any{map[string]string{"value": "20261002"}}, "metricValues": values("7")}}
		result["rowCount"] = 2
	}
	return result
}

func ga4NoDataReportFixture(daily bool) map[string]any {
	report := map[string]any{
		"kind": "analyticsData#runReport",
		"metadata": map[string]any{
			"currencyCode": "NOK",
			"timeZone":     "Europe/Oslo",
		},
	}
	if daily {
		headers := make([]any, 0, len(ga4MetricNames))
		for _, name := range ga4MetricNames {
			headers = append(headers, map[string]string{"name": name})
		}
		report["dimensionHeaders"] = []any{map[string]string{"name": "date"}}
		report["metricHeaders"] = headers
	}
	return report
}

func replaceGA4ReportFixture(target, source map[string]any) {
	for key := range target {
		delete(target, key)
	}
	for key, value := range source {
		target[key] = value
	}
}
func ga4FixtureService(t *testing.T, mutate func(map[string]any, map[string]any)) *Service {
	t.Helper()
	summary, daily := ga4ReportFixture(false), ga4ReportFixture(true)
	if mutate != nil {
		mutate(summary, daily)
	}
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/connections/private-ga4/actions" || r.Method != "POST" || r.Header.Get("Authorization") != "Bearer synthetic" {
			t.Error("wrong broker authority")
		}
		var command struct {
			Operation string         `json:"operation"`
			Params    map[string]any `json:"params"`
			Body      map[string]any `json:"body"`
		}
		_ = json.NewDecoder(r.Body).Decode(&command)
		if command.Operation != "ga4.report.run" || command.Params["propertyId"] != "123" || command.Params["streamId"] != "456" || command.Params["site"] != "https://example.com/" || command.Body["startDate"] != "2026-10-01" || command.Body["endDate"] != "2026-10-20" {
			t.Errorf("unsafe command: %+v", command)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "google", "operation": "ga4.report.run", "result": map[string]any{"summary": summary, "daily": daily, "token": "private-provider-token"}}}})
	}))
	t.Cleanup(provider.Close)
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte("synthetic"), 0600); err != nil {
		t.Fatal(err)
	}
	return NewService(Config{IntegrationCoreURL: provider.URL, IntegrationBearerTokenFile: path, GoogleGA4ConnectionID: "private-ga4", GoogleGA4PropertyID: "123", GoogleGA4StreamID: "456"}, provider.Client(), func() time.Time { return time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC) })
}
func TestGA4UsesSummaryTotalsAndStripsPrivateProviderFields(t *testing.T) {
	got := ga4FixtureService(t, nil).GetGA4(t.Context(), "Q4", "https://example.com/")
	if got.Status != "connected" || got.Totals == nil || got.Totals.Users != 8 || got.Totals.Sessions != 12 || got.Totals.KeyEvents != 2.5 || got.Totals.EngagementSeconds != 45.25 || len(got.Days) != 2 || got.Days[0].Date != "2026-10-01" || got.TimeZone != "Europe/Oslo" {
		t.Fatalf("projection: %+v", got)
	}
	if got.Totals.Users == got.Days[0].Users+got.Days[1].Users {
		t.Fatal("distinct users incorrectly summed over dates")
	}
	raw, _ := json.Marshal(got)
	if strings.Contains(string(raw), "private-") || strings.Contains(string(raw), "metricHeaders") {
		t.Fatal("raw provider data escaped")
	}
}

func TestGA4ReportingSitePreservesRegisteredSiteAndSendsExplicitScope(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var command struct {
			Params    map[string]any `json:"params"`
			Operation string         `json:"operation"`
		}
		if err := json.NewDecoder(r.Body).Decode(&command); err != nil {
			t.Fatal(err)
		}
		if command.Operation != "ga4.report.run" || command.Params["site"] != "https://example.com/" || command.Params["reportingSite"] != "https://www.example.com/" {
			t.Errorf("incorrect scope: %+v", command)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "google", "operation": "ga4.report.run", "result": map[string]any{"summary": ga4NoDataReportFixture(false), "daily": ga4NoDataReportFixture(true)}}}})
	}))
	defer provider.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte("synthetic"), 0600); err != nil {
		t.Fatal(err)
	}
	s := NewService(Config{IntegrationCoreURL: provider.URL, IntegrationBearerTokenFile: path, GoogleGA4ConnectionID: "private-ga4", GoogleGA4PropertyID: "123", GoogleGA4StreamID: "456", GoogleGA4ReportingSite: "https://www.example.com/"}, provider.Client(), func() time.Time { return time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC) })
	got := s.GetGA4(t.Context(), "Q4", "https://example.com/")
	if got.Status != StatusConnected || got.Site != "https://example.com/" || got.ReportingSite != "https://www.example.com/" || got.Totals != nil {
		t.Fatalf("incorrect projection: %+v", got)
	}
	s.cfg.GoogleGA4ReportingSite = "https://other.example/"
	if rejected := s.GetGA4(t.Context(), "Q4", "https://example.com/"); rejected.Status != StatusUnavailable || rejected.Totals != nil {
		t.Fatal("arbitrary report site reached provider")
	}
}
func TestGA4MalformedReportsNeverBecomeAnalytics(t *testing.T) {
	for _, mode := range []string{"timezone", "bad count", "fractional count", "unsafe integer", "negative metric", "nonfinite metric", "date outside range", "invalid date", "duplicate dates", "row truncation", "bad headers", "duplicate headers", "missing rows", "missing timezone", "local timezone", "bad key events", "overflow key events", "negative duration", "missing metric value", "summary dimension", "daily dimension", "empty reason"} {
		t.Run(mode, func(t *testing.T) {
			s := ga4FixtureService(t, func(summary, daily map[string]any) {
				switch mode {
				case "timezone":
					daily["metadata"] = map[string]any{"timeZone": "America/New_York"}
				case "missing timezone":
					summary["metadata"] = map[string]any{}
				case "local timezone":
					summary["metadata"] = map[string]any{"timeZone": "Local"}
				case "bad headers":
					summary["metricHeaders"] = []any{map[string]string{"name": "email"}}
				case "duplicate headers":
					summary["metricHeaders"].([]any)[0] = map[string]string{"name": "sessions"}
				case "missing rows":
					summary["rows"] = "wrong"
				case "row truncation":
					daily["rowCount"] = 3
				case "date outside range":
					daily["rows"].([]any)[0].(map[string]any)["dimensionValues"] = []any{map[string]string{"value": "20260930"}}
				case "invalid date":
					daily["rows"].([]any)[0].(map[string]any)["dimensionValues"] = []any{map[string]string{"value": "20261099"}}
				case "duplicate dates":
					daily["rows"].([]any)[1].(map[string]any)["dimensionValues"] = []any{map[string]string{"value": "20261001"}}
				case "bad key events":
					summary["rows"].([]any)[0].(map[string]any)["metricValues"].([]any)[4] = map[string]string{"value": "NaN"}
				case "overflow key events":
					summary["rows"].([]any)[0].(map[string]any)["metricValues"].([]any)[4] = map[string]string{"value": "1e999"}
				case "negative duration":
					summary["rows"].([]any)[0].(map[string]any)["metricValues"].([]any)[5] = map[string]string{"value": "-1"}
				case "missing metric value":
					summary["rows"].([]any)[0].(map[string]any)["metricValues"] = []any{}
				case "summary dimension":
					summary["rows"].([]any)[0].(map[string]any)["dimensionValues"] = []any{map[string]string{"value": "other.example"}}
				case "daily dimension":
					daily["rows"].([]any)[0].(map[string]any)["dimensionValues"] = []any{}
				case "empty reason":
					summary["metadata"].(map[string]any)["emptyReason"] = "restricted"
				default:
					value := map[string]string{"bad count": "abc", "fractional count": "1.5", "unsafe integer": "9007199254740992", "negative metric": "-1", "nonfinite metric": "NaN"}[mode]
					summary["rows"].([]any)[0].(map[string]any)["metricValues"].([]any)[0] = map[string]string{"value": value}
				}
			})
			got := s.GetGA4(t.Context(), "Q4", "https://example.com/")
			if got.Status != "unavailable" || got.Totals != nil || len(got.Days) != 0 || got.Reason != "source_unavailable" {
				t.Fatalf("bad source accepted: %+v", got)
			}
		})
	}
}
func TestGA4EmptyThresholdReportHasNoFabricatedTotals(t *testing.T) {
	got := ga4FixtureService(t, func(summary, daily map[string]any) {
		summary["rows"] = []any{}
		summary["rowCount"] = 0
		summary["metadata"].(map[string]any)["subjectToThresholding"] = true
		daily["metadata"].(map[string]any)["dataLossFromOtherRow"] = true
		daily["metadata"].(map[string]any)["samplingMetadatas"] = []any{map[string]string{"samplesReadCount": "10", "samplingSpaceSize": "100"}}
	}).GetGA4(t.Context(), "Q4", "https://example.com/")
	if got.Status != "connected" || got.Totals != nil || got.Quality == nil || !got.Quality.Thresholded || !got.Quality.Sampled || !got.Quality.DataLoss {
		t.Fatalf("empty privacy report: %+v", got)
	}
}

func TestGA4HeaderlessEmptySummaryReturnsConnectedNoData(t *testing.T) {
	got := ga4FixtureService(t, func(summary, daily map[string]any) {
		replaceGA4ReportFixture(summary, ga4NoDataReportFixture(false))
		replaceGA4ReportFixture(daily, ga4NoDataReportFixture(true))
	}).GetGA4(t.Context(), "Q4", "https://example.com/")
	if got.Status != "connected" || got.Totals != nil || len(got.Days) != 0 || got.TimeZone != "Europe/Oslo" || got.Quality != nil {
		t.Fatalf("headerless empty report: %+v", got)
	}
}

func TestGA4HeaderlessEmptySummaryPreservesQualityFlags(t *testing.T) {
	got := ga4FixtureService(t, func(summary, daily map[string]any) {
		replaceGA4ReportFixture(summary, ga4NoDataReportFixture(false))
		replaceGA4ReportFixture(daily, ga4NoDataReportFixture(true))
		summary["metadata"].(map[string]any)["subjectToThresholding"] = true
		daily["metadata"].(map[string]any)["dataLossFromOtherRow"] = true
		daily["metadata"].(map[string]any)["samplingMetadatas"] = []any{map[string]string{"samplesReadCount": "10", "samplingSpaceSize": "100"}}
	}).GetGA4(t.Context(), "Q4", "https://example.com/")
	if got.Status != "connected" || got.Totals != nil || len(got.Days) != 0 || got.Quality == nil || !got.Quality.Thresholded || !got.Quality.Sampled || !got.Quality.DataLoss {
		t.Fatalf("headerless empty quality report: %+v", got)
	}
}

func TestGA4HeaderlessEmptySummaryRejectsMalformedOrMismatchedReports(t *testing.T) {
	for _, mode := range []string{"summary metric headers", "missing daily metric headers", "bad daily metric headers", "duplicate daily metric headers", "bad daily date header", "nonempty daily", "daily row count nonzero", "mismatched timezone", "invalid timezone"} {
		t.Run(mode, func(t *testing.T) {
			got := ga4FixtureService(t, func(summary, daily map[string]any) {
				replaceGA4ReportFixture(summary, ga4NoDataReportFixture(false))
				replaceGA4ReportFixture(daily, ga4NoDataReportFixture(true))
				switch mode {
				case "summary metric headers":
					summary["metricHeaders"] = []any{map[string]string{"name": "totalUsers"}}
				case "missing daily metric headers":
					delete(daily, "metricHeaders")
				case "bad daily metric headers":
					daily["metricHeaders"] = []any{map[string]string{"name": "email"}}
				case "duplicate daily metric headers":
					daily["metricHeaders"].([]any)[1] = map[string]string{"name": "totalUsers"}
				case "bad daily date header":
					daily["dimensionHeaders"] = []any{map[string]string{"name": "day"}}
				case "nonempty daily":
					replaceGA4ReportFixture(daily, ga4ReportFixture(true))
				case "daily row count nonzero":
					daily["rowCount"] = 1
				case "mismatched timezone":
					daily["metadata"].(map[string]any)["timeZone"] = "America/New_York"
				case "invalid timezone":
					summary["metadata"].(map[string]any)["timeZone"] = "Local"
					daily["metadata"].(map[string]any)["timeZone"] = "Local"
				}
			}).GetGA4(t.Context(), "Q4", "https://example.com/")
			if got.Status != "unavailable" || got.Totals != nil || len(got.Days) != 0 || got.Reason != "source_unavailable" {
				t.Fatalf("bad headerless empty report accepted: %+v", got)
			}
		})
	}
}
func TestGA4InvalidConfigInputsCancellationAndSourceFailure(t *testing.T) {
	s := NewService(Config{}, nil, nil)
	if got := s.GetGA4(t.Context(), "Q1", "https://example.com/"); got.Reason != "not_configured" {
		t.Fatal(got)
	}
	if got := s.GetGA4(t.Context(), "Q9", "https://example.com/"); got.Reason != "source_unavailable" {
		t.Fatal(got)
	}
	if got := s.GetGA4(t.Context(), "Q1", "https://localhost/"); got.Totals != nil || got.Status != "unavailable" {
		t.Fatal("unsafe site accepted")
	}
	s = ga4FixtureService(t, nil)
	s.cfg.GoogleGA4PropertyID = "../bad"
	if got := s.GetGA4(t.Context(), "Q4", "https://example.com/"); got.Reason != "not_configured" {
		t.Fatal("unsafe provider ID accepted")
	}
	s = ga4FixtureService(t, nil)
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if got := s.GetGA4(ctx, "Q4", "https://example.com/"); got.Totals != nil || got.Reason != "source_unavailable" {
		t.Fatal("cancelled request returned data")
	}
}

func TestGA4TimezoneDataIsAvailableWithInvalidEnvironmentPaths(t *testing.T) {
	t.Setenv("ZONEINFO", filepath.Join(t.TempDir(), "missing-zone-data"))
	t.Setenv("GOROOT", filepath.Join(t.TempDir(), "missing-go-runtime"))
	for _, zone := range []string{"Europe/Oslo", "Antarctica/Troll", "Pacific/Chatham"} {
		if !validGA4Zone(zone) {
			t.Fatalf("IANA timezone unavailable: %s", zone)
		}
	}
	for _, zone := range []string{"Local", "not-a-timezone", "Europe/Oslo\n"} {
		if validGA4Zone(zone) {
			t.Fatalf("invalid provider timezone accepted: %q", zone)
		}
	}
}
