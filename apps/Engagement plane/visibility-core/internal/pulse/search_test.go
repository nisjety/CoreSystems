package pulse

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func searchBroker(t *testing.T, property, queryRows, pageRows string) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
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
		if r.Header.Get("Authorization") != "Bearer scoped-test-token" {
			t.Error("missing scoped bearer")
		}
		var result string
		if action.Operation == "gsc.sites.list" {
			result = fmt.Sprintf(`{"siteEntry":[{"siteUrl":%q,"permissionLevel":"siteOwner"}]}`, property)
		} else {
			if action.Operation != "gsc.search_analytics.query" || len(action.Body.Dimensions) != 1 || action.Params["siteUrl"] != property || action.Body.StartDate != "2026-10-01" || action.Body.EndDate != "2026-10-20" || action.Body.RowLimit != 100 || action.Body.DataState != "final" {
				t.Errorf("invalid fixed action: %#v", action)
				return
			}
			rows := queryRows
			if action.Body.Dimensions[0] == "page" {
				rows = pageRows
			} else if action.Body.Dimensions[0] != "query" {
				t.Error("unsafe dimension")
			}
			result = `{"rows":` + rows + `}`
		}
		_, _ = fmt.Fprintf(w, `{"success":true,"data":{"action":{"providerKey":"google","operation":%q,"result":%s}}}`, action.Operation, result)
	}))
}

func TestSearchProjectsRealRowsAndEvidenceOnlyOpportunities(t *testing.T) {
	provider := searchBroker(t, "sc-domain:aquatiq.com", `[
		{"keys":["snippet"],"clicks":2,"impressions":300,"ctr":0.0067,"position":2},
		{"keys":["title"],"clicks":3,"impressions":200,"ctr":0.015,"position":7},
		{"keys":["test"],"clicks":4,"impressions":100,"ctr":0.04,"position":8},
		{"keys":["content"],"clicks":3,"impressions":150,"ctr":0.02,"position":17},
		{"keys":["small"],"clicks":0,"impressions":99,"ctr":0,"position":5}
	]`, `[{"keys":["https://www.aquatiq.com/services?q=1"],"clicks":5,"impressions":200,"ctr":0.025,"position":7}]`)
	defer provider.Close()
	got := newTestService(t, provider.URL, "sc-domain:aquatiq.com").GetSearch(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusConnected || len(got.Queries) != 5 || len(got.Pages) != 1 || len(got.Opportunities) != 4 || got.PropertyScope != "domain" || got.RowLimit != 100 || got.UpdatedAt != "2026-10-21T00:00:00Z" || got.Reason != "" {
		t.Fatalf("projection=%#v", got)
	}
	for i, want := range []string{"snippet_review", "title_meta", "content_expansion", "title_test"} {
		if got.Opportunities[i].Type != want {
			t.Errorf("opportunity[%d]=%#v want %s", i, got.Opportunities[i], want)
		}
	}
	if strings.Contains(string(mustJSON(t, got)), "verified-google") || got.QueriesAtLimit || got.PagesAtLimit {
		t.Fatal("leaked configuration or false limit")
	}
}

func TestSearchNeverExposesPartialOrMalformedProviderRows(t *testing.T) {
	valid := `{"keys":["test"],"clicks":1,"impressions":200,"ctr":0.005,"position":5}`
	for name, row := range map[string]string{
		"missing metrics": `{"keys":["test"]}`, "null count": `{"keys":["test"],"clicks":null,"impressions":2,"ctr":0.5,"position":1}`,
		"negative":                 strings.Replace(valid, `"clicks":1`, `"clicks":-1`, 1),
		"fraction":                 strings.Replace(valid, `"clicks":1`, `"clicks":1.5`, 1),
		"unsafe integer":           strings.Replace(valid, `"impressions":200`, `"impressions":9007199254740992`, 1),
		"greater than impressions": strings.Replace(valid, `"clicks":1`, `"clicks":201`, 1),
		"invalid ctr":              strings.Replace(valid, `"ctr":0.005`, `"ctr":1.1`, 1),
		"infinite position":        strings.Replace(valid, `"position":5`, `"position":1e999`, 1),
		"zero position":            strings.Replace(valid, `"position":5`, `"position":0`, 1),
		"sub-first position":       strings.Replace(valid, `"position":5`, `"position":0.5`, 1),
		"negative position":        strings.Replace(valid, `"position":5`, `"position":-1`, 1),
		"empty key":                strings.Replace(valid, `"test"`, `" "`, 1),
		"control key":              strings.Replace(valid, `"test"`, `"bad\nvalue"`, 1),
		"long key":                 strings.Replace(valid, `"test"`, `"`+strings.Repeat("x", 513)+`"`, 1),
		"extra key":                strings.Replace(valid, `["test"]`, `["test","extra"]`, 1),
		"duplicate key":            valid + "," + valid,
	} {
		t.Run(name, func(t *testing.T) {
			provider := searchBroker(t, "sc-domain:aquatiq.com", "["+row+"]", "[]")
			defer provider.Close()
			got := newTestService(t, provider.URL, "").GetSearch(context.Background(), "Q4", "https://aquatiq.com/")
			if got.Status != StatusUnavailable || got.Reason != ReasonSourceUnavailable || len(got.Queries)+len(got.Pages)+len(got.Opportunities) != 0 || got.Queries == nil || got.Pages == nil || got.Opportunities == nil {
				t.Fatalf("malformed projection=%#v", got)
			}
		})
	}
}

func TestSearchValidatesPageScopeWithoutFetchingPageURLs(t *testing.T) {
	for _, tc := range []struct {
		name, property, key string
		connected           bool
	}{
		{"domain subdomain", "sc-domain:aquatiq.com", "http://www.aquatiq.com/path?a=1", true},
		{"prefix", "https://aquatiq.com/", "https://aquatiq.com/path", true},
		{"prefix other subdomain", "https://aquatiq.com/", "https://www.aquatiq.com/path", false},
		{"prefix other protocol", "https://aquatiq.com/", "http://aquatiq.com/path", false},
		{"foreign", "sc-domain:aquatiq.com", "https://evilaquatiq.com/path", false},
		{"credentials", "sc-domain:aquatiq.com", "https://user@aquatiq.com/path", false},
		{"fragment", "sc-domain:aquatiq.com", "https://aquatiq.com/path#secret", false},
		{"scheme", "sc-domain:aquatiq.com", "javascript:alert(1)", false},
		{"long", "sc-domain:aquatiq.com", "https://aquatiq.com/" + strings.Repeat("x", 2048), false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rows := fmt.Sprintf(`[{"keys":[%q],"clicks":1,"impressions":200,"ctr":0.005,"position":5}]`, tc.key)
			provider := searchBroker(t, tc.property, "[]", rows)
			defer provider.Close()
			got := newTestService(t, provider.URL, tc.property).GetSearch(context.Background(), "Q4", "https://aquatiq.com/")
			if (got.Status == StatusConnected) != tc.connected {
				t.Fatalf("projection=%#v", got)
			}
		})
	}
}

func TestSearchReportsPotentialRowLimitsAndEmptyMeasuredLists(t *testing.T) {
	rows := make([]string, 100)
	for i := range rows {
		rows[i] = fmt.Sprintf(`{"keys":["query %03d"],"clicks":0,"impressions":0,"ctr":0,"position":0}`, i)
	}
	provider := searchBroker(t, "sc-domain:aquatiq.com", "["+strings.Join(rows, ",")+"]", "[]")
	defer provider.Close()
	got := newTestService(t, provider.URL, "").GetSearch(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Status != StatusConnected || !got.QueriesAtLimit || got.PagesAtLimit || len(got.Queries) != 100 || got.Pages == nil || got.Opportunities == nil {
		t.Fatalf("projection=%#v", got)
	}
}

func TestSearchDoesNotQueryUnverifiedOrUnconfiguredSource(t *testing.T) {
	now := func() time.Time { return time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC) }
	for _, tc := range []struct{ quarter, site, reason string }{
		{"Q4", "https://aquatiq.com/", ReasonNotConfigured}, {"Q4", "https://localhost/", ReasonPropertyUnverified}, {"Q5", "https://aquatiq.com/", ReasonSourceUnavailable},
	} {
		got := NewService(Config{}, nil, now).GetSearch(context.Background(), tc.quarter, tc.site)
		if got.Status != StatusUnavailable || got.Reason != tc.reason || got.Queries == nil || got.Pages == nil || got.Opportunities == nil {
			t.Fatalf("projection=%#v", got)
		}
	}
	var calls atomic.Int32
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[]}}}}`))
	}))
	defer provider.Close()
	got := newTestService(t, provider.URL, "").GetSearch(context.Background(), "Q4", "https://aquatiq.com/")
	if got.Reason != ReasonPropertyUnverified || calls.Load() != 1 {
		t.Fatalf("projection=%#v calls=%d", got, calls.Load())
	}
}

func TestSearchReadsDimensionsInParallelAndHonorsCancellation(t *testing.T) {
	started := make(chan struct{}, 2)
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var action struct {
			Operation string `json:"operation"`
		}
		_ = json.NewDecoder(r.Body).Decode(&action)
		if action.Operation == "gsc.sites.list" {
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
			return
		}
		started <- struct{}{}
		<-r.Context().Done()
	}))
	defer provider.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	results := make(chan SearchProjection, 1)
	go func() { results <- newTestService(t, provider.URL, "").GetSearch(ctx, "Q4", "https://aquatiq.com/") }()
	for i := 0; i < 2; i++ {
		select {
		case <-started:
		case <-time.After(time.Second):
			t.Fatal("dimension reads are not parallel")
		}
	}
	cancel()
	select {
	case got := <-results:
		if got.Reason != ReasonSourceUnavailable {
			t.Fatalf("projection=%#v", got)
		}
	case <-time.After(time.Second):
		t.Fatal("cancellation ignored")
	}
}

func TestSearchOpportunityThresholdsAndFractionalPositions(t *testing.T) {
	for _, tc := range []struct {
		name          string
		position, ctr float64
		want          string
	}{
		{"top three at threshold", 3, 0.03, ""},
		{"top two above threshold", 2, 0.04, ""},
		{"fractional top three", 2.5, 0.02, "snippet_review"},
		{"fractional second range", 3.01, 0.02, "title_meta"},
		{"title boundary", 10, 0.03, "title_test"},
		{"fractional content range", 10.01, 0.02, "content_expansion"},
		{"position outside limit", 20.01, 0.02, ""},
		{"CTR outside limit", 12, 0.05, ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got := searchOpportunities([]SearchRow{{Key: "observed", Impressions: 100, CTR: tc.ctr, Position: tc.position}})
			if tc.want == "" {
				if len(got) != 0 {
					t.Fatalf("unexpected opportunity=%#v", got)
				}
			} else if len(got) != 1 || got[0].Type != tc.want {
				t.Fatalf("opportunities=%#v want %q", got, tc.want)
			}
		})
	}
}

func TestSearchCountsAcceptWholeDoubleRepresentationsOnly(t *testing.T) {
	for _, tc := range []struct {
		value string
		want  int64
		valid bool
	}{
		{"3", 3, true}, {"3.0", 3, true}, {"3e0", 3, true}, {"30e-1", 3, true}, {"0.00", 0, true},
		{"9007199254740991", 9007199254740991, true}, {"9007199254740991.0", 9007199254740991, true},
		{"3.5", 0, false}, {"3e-1", 0, false}, {"-1.0", 0, false}, {"9007199254740992", 0, false},
		{"9007199254740990.5", 0, false}, {"1e999999999", 0, false}, {"1e-999999999", 0, false}, {"", 0, false},
	} {
		t.Run(tc.value, func(t *testing.T) {
			got, err := wholeSafeSearchCount(json.Number(tc.value))
			if (err == nil) != tc.valid || (tc.valid && got != tc.want) {
				t.Fatalf("count(%q)=%d,%v", tc.value, got, err)
			}
		})
	}
}
