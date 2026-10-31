package pulse

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestServiceReturnsVerifiedDailyGSCProjection(t *testing.T) {
	var operations []string
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Authorization"); got != "Bearer scoped-test-token" {
			t.Errorf("Authorization = %q", got)
		}
		if r.URL.Path != "/api/v1/connections/verified-google/actions" || r.Method != http.MethodPost {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		var body struct {
			Operation string         `json:"operation"`
			Params    map[string]any `json:"params"`
			Body      map[string]any `json:"body"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Errorf("decode: %v", err)
		}
		operations = append(operations, body.Operation)
		switch body.Operation {
		case "gsc.sites.list":
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
		case "gsc.search_analytics.query":
			if body.Params["siteUrl"] != "sc-domain:aquatiq.com" || body.Body["startDate"] != "2026-10-01" || body.Body["endDate"] != "2026-10-20" || body.Body["rowLimit"] != float64(20) {
				t.Errorf("query body = %#v, params=%#v", body.Body, body.Params)
			}
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.search_analytics.query","result":{"rows":[{"keys":["2026-10-01"],"clicks":4,"impressions":29,"ctr":0.1},{"keys":["2026-10-03"],"clicks":2,"impressions":19}]}}}}`))
		default:
			t.Errorf("unexpected operation %q", body.Operation)
		}
	}))
	defer provider.Close()

	service := newTestService(t, provider.URL, "sc-domain:aquatiq.com")
	got := service.Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusConnected || got.Totals == nil || got.Totals.Clicks != 6 || got.Totals.Impressions != 48 {
		t.Fatalf("projection = %#v", got)
	}
	if got.PropertyScope != "domain" {
		t.Fatalf("scope = %q, want domain", got.PropertyScope)
	}
	if len(got.Days) != 2 || got.Days[1].Date != "2026-10-03" || got.Period.EndDate != "2026-10-20" {
		t.Fatalf("days/period = %#v", got)
	}
	if strings.Join(operations, ",") != "gsc.sites.list,gsc.search_analytics.query" {
		t.Fatalf("operations = %#v", operations)
	}
}

func TestServiceKeepsEveryDailyRowInTheFullYear(t *testing.T) {
	var rowLimit int
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			Operation string `json:"operation"`
			Body      struct {
				RowLimit int `json:"rowLimit"`
			} `json:"body"`
		}
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Errorf("decode request: %v", err)
			return
		}
		if request.Operation == "gsc.sites.list" {
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
			return
		}
		rowLimit = request.Body.RowLimit
		start := time.Date(2026, time.January, 1, 0, 0, 0, 0, time.UTC)
		rows := make([]map[string]any, 0, 293)
		for index := 0; index < 293; index++ {
			rows = append(rows, map[string]any{"keys": []string{start.AddDate(0, 0, index).Format(dateLayout)}, "clicks": 1, "impressions": 2})
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "google", "operation": "gsc.search_analytics.query", "result": map[string]any{"rows": rows}}}})
	}))
	defer provider.Close()

	projection := newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "FY", "https://aquatiq.com/")
	if rowLimit != 293 || projection.Totals == nil || projection.Totals.Clicks != 293 || projection.Totals.Impressions != 586 || len(projection.Days) != 293 {
		t.Fatalf("rowLimit=%d projection=%#v", rowLimit, projection)
	}
}

func TestCanonicalSiteAcceptsOnlyPublicHTTPSOrigins(t *testing.T) {
	for _, tc := range []struct{ input, want string }{
		{"https://aquatiq.com", "https://aquatiq.com/"},
		{"https://BLOG.Example.COM/", "https://blog.example.com/"},
	} {
		got, err := CanonicalSite(tc.input)
		if err != nil || got != tc.want {
			t.Errorf("CanonicalSite(%q) = %q, %v; want %q", tc.input, got, err, tc.want)
		}
	}
	for _, input := range []string{
		"", "aquatiq.com", "http://aquatiq.com/", "https://aquatiq.com/path", "https://aquatiq.com/?q=1",
		"https://user@aquatiq.com/", "https://aquatiq.com:443/", "https://127.0.0.1/",
		"https://localhost/", "https://foo.localhost/", "https://intranet/", "https://foo..com/",
		"https://aquatiq.com/#fragment", "https://aquatiq.com\\@evil.example/",
	} {
		if got, err := CanonicalSite(input); err == nil {
			t.Errorf("CanonicalSite(%q) accepted as %q", input, got)
		}
	}
}

func TestServiceSelectsVerifiedPropertyForRequestedSite(t *testing.T) {
	for _, tc := range []struct {
		name, site, preferred, sites, wantProperty, wantStatus, wantScope string
	}{
		{"switched domain", "https://example.com/", "sc-domain:aquatiq.com", `[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"},{"siteUrl":"sc-domain:example.com","permissionLevel":"siteFullUser"}]`, "sc-domain:example.com", StatusConnected, "domain"},
		{"parent domain does not cover selected subdomain metrics", "https://blog.example.com/", "", `[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteOwner"}]`, "", StatusUnavailable, ""},
		{"exact subdomain property", "https://blog.example.com/", "", `[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteOwner"},{"siteUrl":"sc-domain:blog.example.com","permissionLevel":"siteOwner"}]`, "sc-domain:blog.example.com", StatusConnected, "domain"},
		{"exact prefix preferred", "https://example.com/", "", `[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteOwner"},{"siteUrl":"https://example.com/","permissionLevel":"siteOwner"}]`, "https://example.com/", StatusConnected, "url_prefix"},
		{"unverified sibling", "https://notexample.com/", "", `[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteOwner"}]`, "", StatusUnavailable, ""},
		{"no permission", "https://example.com/", "", `[{"siteUrl":"sc-domain:example.com","permissionLevel":"siteUnverifiedUser"}]`, "", StatusUnavailable, ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var queryProperty string
			provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				var body struct {
					Operation string         `json:"operation"`
					Params    map[string]any `json:"params"`
				}
				if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
					t.Error(err)
				}
				if body.Operation == "gsc.sites.list" {
					_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":` + tc.sites + `}}}}`))
					return
				}
				queryProperty, _ = body.Params["siteUrl"].(string)
				_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.search_analytics.query","result":{"rows":[]}}}}`))
			}))
			defer provider.Close()
			service := newTestService(t, provider.URL, tc.preferred)
			got := service.Get(context.Background(), "Q4", tc.site)
			if got.Site != tc.site || got.Status != tc.wantStatus || queryProperty != tc.wantProperty || got.PropertyScope != tc.wantScope {
				t.Fatalf("projection=%#v queried property=%q want=%q", got, queryProperty, tc.wantProperty)
			}
			if tc.wantStatus == StatusUnavailable && (got.Reason != ReasonPropertyUnverified || got.Totals != nil) {
				t.Fatalf("unverified projection=%#v", got)
			}
		})
	}
}

func TestServiceDoesNotQueryUnverifiedProperty(t *testing.T) {
	var calls int
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:other.example","permissionLevel":"siteOwner"}]}}}}`))
	}))
	defer provider.Close()

	got := newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusUnavailable || got.Reason != ReasonPropertyUnverified || got.Totals != nil || len(got.Days) != 0 || calls != 1 {
		t.Fatalf("projection=%#v calls=%d", got, calls)
	}
}

func TestServiceUnavailableWhenNotConfiguredOrProviderFails(t *testing.T) {
	now := func() time.Time { return time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC) }
	got := NewService(Config{}, nil, now).Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Reason != ReasonNotConfigured || got.Totals != nil || got.Period.StartDate != "2026-10-01" {
		t.Fatalf("unconfigured = %#v", got)
	}

	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte(`{"error":{"message":"private upstream detail"}}`))
	}))
	defer provider.Close()
	got = newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Reason != ReasonSourceUnavailable || got.Totals != nil || strings.Contains(string(mustJSON(t, got)), "private upstream detail") {
		t.Fatalf("failed projection = %#v", got)
	}
}

func TestServiceRejectsMalformedAnalyticsWithoutInventingValues(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.URL.Path, "/actions") {
			var body struct {
				Operation string `json:"operation"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			if body.Operation == "gsc.sites.list" {
				_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
				return
			}
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.search_analytics.query","result":{"rows":[{"keys":["2026-10-01"],"clicks":-4,"impressions":29}]}}}}`))
	}))
	defer provider.Close()
	got := newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusUnavailable || got.Totals != nil {
		t.Fatalf("malformed projection = %#v", got)
	}
}

func TestServiceTreatsEmptyVerifiedRowsAsMeasuredZero(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Operation string `json:"operation"`
		}
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body.Operation == "gsc.sites.list" {
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteRestrictedUser"}]}}}}`))
			return
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.search_analytics.query","result":{"rows":[]}}}}`))
	}))
	defer provider.Close()
	got := newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusConnected || got.Totals == nil || got.Totals.Clicks != 0 || got.Totals.Impressions != 0 || got.Days == nil {
		t.Fatalf("empty measured projection=%#v", got)
	}
}

func TestServiceDoesNotFollowProviderRedirectWithBearer(t *testing.T) {
	var redirected bool
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		redirected = true
	}))
	defer target.Close()
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusTemporaryRedirect)
	}))
	defer provider.Close()
	got := newTestService(t, provider.URL, "sc-domain:aquatiq.com").Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusUnavailable || got.Reason != ReasonSourceUnavailable || redirected {
		t.Fatalf("redirect result=%#v followed=%v", got, redirected)
	}
}

func TestValidateConfigAndTokenFileRejectUnsafeInputs(t *testing.T) {
	studioIntegration := Config{IntegrationCoreURL: "http://integration-api:3026", GoogleConnectionID: "fixed-connection", IntegrationBearerTokenFile: "/run/secrets/token"}
	if err := ValidateConfig(studioIntegration); err != nil {
		t.Fatalf("Studio inter-plane Integration Core URL rejected: %v", err)
	}
	for _, property := range []string{"http://aquatiq.com/", "https://other.example/path", "sc-domain:localhost"} {
		cfg := Config{IntegrationCoreURL: "https://integration.example", GoogleConnectionID: "fixed-connection", GoogleGSCProperty: property, IntegrationBearerTokenFile: "/run/secrets/token"}
		if err := ValidateConfig(cfg); err == nil {
			t.Errorf("property %q accepted", property)
		}
	}
	for _, token := range []string{"", "contains space", "contains\ttab", "line1\nline2", strings.Repeat("a", 8193)} {
		path := filepath.Join(t.TempDir(), "token")
		if err := os.WriteFile(path, []byte(token), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := readBearerToken(path); err == nil {
			t.Errorf("unsafe token file accepted")
		}
	}
}

func TestBrokerConfigurationDoesNotAuthorizeProviderReads(t *testing.T) {
	var brokerCalls atomic.Int32
	broker := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		brokerCalls.Add(1)
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer broker.Close()
	cfg := Config{IntegrationCoreURL: broker.URL}
	if err := ValidateConfig(cfg); err != nil {
		t.Fatalf("broker-only configuration rejected: %v", err)
	}
	service := NewService(cfg, nil, func() time.Time {
		return time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC)
	})
	got := service.Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusUnavailable || got.Reason != ReasonNotConfigured || got.Totals != nil || len(got.Days) != 0 {
		t.Fatalf("unauthorized projection = %#v", got)
	}
	if brokerCalls.Load() != 0 {
		t.Fatal("broker contacted before source authorization exists")
	}
}

func TestServiceRejectsMalformedSiteListAndMissingTokenFile(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":"not a sites object"}}}`))
	}))
	defer provider.Close()
	service := newTestService(t, provider.URL, "sc-domain:aquatiq.com")
	got := service.Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Reason != ReasonSourceUnavailable || got.Totals != nil {
		t.Fatalf("malformed sites response=%#v", got)
	}
	service.cfg.IntegrationBearerTokenFile = filepath.Join(t.TempDir(), "missing")
	got = service.Get(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Reason != ReasonSourceUnavailable || got.Totals != nil {
		t.Fatalf("missing token response=%#v", got)
	}
}

func TestQuarterBoundsAndFuture(t *testing.T) {
	now := time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC)
	for _, tc := range []struct{ quarter, start, end string }{
		{"Q1", "2026-01-01", "2026-03-31"},
		{"Q2", "2026-04-01", "2026-06-30"},
		{"Q3", "2026-07-01", "2026-09-30"},
		{"Q4", "2026-10-01", "2026-10-20"},
		{"FY", "2026-01-01", "2026-10-20"},
	} {
		period, err := QuarterPeriod(tc.quarter, now)
		if err != nil || period.StartDate != tc.start || period.EndDate != tc.end {
			t.Errorf("%s: period=%#v err=%v", tc.quarter, period, err)
		}
	}
	if _, err := QuarterPeriod("Q5", now); err == nil {
		t.Fatal("invalid quarter accepted")
	}
	if _, err := QuarterPeriod("Q4", time.Date(2026, 1, 5, 0, 0, 0, 0, time.UTC)); err == nil {
		t.Fatal("future quarter accepted")
	}
	if _, err := QuarterPeriod("FY", time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)); err == nil {
		t.Fatal("full year without a completed day accepted")
	}
	defaultPeriod, err := QuarterPeriod("", now)
	if err != nil || defaultPeriod.StartDate != "2026-10-01" {
		t.Fatalf("default current quarter=%#v err=%v", defaultPeriod, err)
	}
}

func newTestService(t *testing.T, upstreamURL, property string) *Service {
	t.Helper()
	path := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(path, []byte("scoped-test-token\n"), 0600); err != nil {
		t.Fatal(err)
	}
	return NewService(Config{IntegrationCoreURL: upstreamURL, GoogleConnectionID: "verified-google", GoogleGSCProperty: property, IntegrationBearerTokenFile: path}, nil, func() time.Time {
		return time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC)
	})
}

func mustJSON(t *testing.T, value any) []byte {
	t.Helper()
	bytes, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return bytes
}
