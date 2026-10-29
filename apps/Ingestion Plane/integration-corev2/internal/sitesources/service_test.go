package sitesources

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const website = "web_12345678901234567890123456789012"
const group = "12345678-1234-4234-8234-123456789012"

var current = time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)

type roundTrip func(*http.Request) (*http.Response, error)

func (f roundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func setupManifest(t *testing.T, mutate func(map[string]any)) string {
	t.Helper()
	dir := t.TempDir()
	cookieKey := filepath.Join(dir, "cookie-key")
	algoliaKey := filepath.Join(dir, "algolia-key")
	for file, key := range map[string]string{cookieKey: strings.Repeat("a", 32), algoliaKey: strings.Repeat("b", 32)} {
		if err := os.WriteFile(file, []byte(key), 0600); err != nil {
			t.Fatal(err)
		}
	}
	item := map[string]any{"organizationId": "org-1", "websiteId": website, "websiteRevision": 7, "site": "https://triodelab.no/", "cookiebot": map[string]any{"domainGroupId": group, "domain": "www.triodelab.no", "culture": "en", "apiKeyFile": cookieKey}, "algolia": map[string]any{"applicationId": "ABCDEFGHIJ", "indexName": "triodelab_prod", "region": "de", "analyticsKeyFile": algoliaKey}}
	if mutate != nil {
		mutate(item)
	}
	raw, _ := json.Marshal(map[string]any{"version": 1, "installations": []any{item}})
	path := filepath.Join(dir, "bindings.json")
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	return path
}
func input(provider string) Input {
	return Input{WebsiteID: website, WebsiteRevision: 7, Site: "https://triodelab.no/", Provider: provider}
}

const consentJSON = `{"domain":"www.triodelab.no","urlpath":"/","utcstartdate":"2026-10-16T00:00:00.0000000Z","utcenddate":"2026-10-22T23:59:59.9990000Z","consentstat":{"consentday":{"Date":"2026-10-22T00:00:00","OptIn":"12","OptOut":"3","TypeOptInPref":"4","TypeOptInStat":"5","TypeOptInMark":"2","Countries":{"Country":[{"Code":"no","Count":"15"}]}}}}`
const inventoryJSON = `{"domain":"www.triodelab.no","urlpath":"/","utcscandate":"2026-10-20T09:45:00.5030000Z","culture":"en","cookies":[{"Category":"1","ThirdParty":"0","Secure":"1","HTTPOnly":"0","Value":"PRIVATE_COOKIE_VALUE","Name":"private-name","InitiatorSourceDomainIP":"private-IP"},{"Category":"3","ThirdParty":"1","Secure":"1","HTTPOnly":"1"}]}`
const algoliaJSON = `{"count":10,"noResultCount":2,"rate":0.2,"dates":[{"date":"2026-10-22","count":10,"noResultCount":2,"rate":0.2}]}`

func response(body string) *http.Response {
	return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(body)), Header: http.Header{"Content-Type": []string{"application/json"}}}
}
func TestMissingConfigurationNeverCallsProviders(t *testing.T) {
	s, err := NewService("", nil, func() time.Time { return current })
	if err != nil {
		t.Fatal(err)
	}
	for _, provider := range []string{"cookiebot", "algolia"} {
		r, err := s.Collect(context.Background(), "org-1", input(provider))
		if err != nil || r.Status != "not_configured" || r.Cookiebot != nil || r.Algolia != nil || r.Window.StartDate != "2026-10-16" || r.Window.EndDate != "2026-10-22" {
			t.Fatalf("%+v %v", r, err)
		}
	}
}
func TestFixedEndpointsAndAggregateOnlyProjection(t *testing.T) {
	var called []string
	client := &http.Client{Transport: roundTrip(func(r *http.Request) (*http.Response, error) {
		called = append(called, r.URL.Host)
		if r.Method != "GET" || r.Header.Get("Cookie") != "" || r.Header.Get("X-Org-ID") != "" {
			t.Fatal("request widened")
		}
		switch r.URL.Host {
		case "consent.cookiebot.com":
			if r.Header.Get("X-Algolia-API-Key") != "" || !strings.HasPrefix(r.URL.Path, "/api/v1/"+strings.Repeat("a", 32)+"/json/domaingroup/"+group+"/") {
				t.Fatal("credential crossed host")
			}
			if strings.HasSuffix(r.URL.Path, "/consent/stats") {
				if r.URL.Query().Get("startdate") != "20261016" || r.URL.Query().Get("enddate") != "20261022" {
					t.Fatal("period")
				}
				return response(consentJSON), nil
			}
			if r.URL.Path != "/api/v1/"+strings.Repeat("a", 32)+"/json/domaingroup/"+group+"/en/domain/www.triodelab.no/cookies" {
				t.Fatal("inventory path")
			}
			return response(inventoryJSON), nil
		case "analytics.de.algolia.com":
			if r.URL.Path != "/2/searches/noResultRate" || r.Header.Get("X-Algolia-Application-Id") != "ABCDEFGHIJ" || r.Header.Get("X-Algolia-API-Key") != strings.Repeat("b", 32) || r.URL.Query().Get("index") != "triodelab_prod" || r.URL.Query().Get("startDate") != "2026-10-16" {
				t.Fatal("analytics request")
			}
			return response(algoliaJSON), nil
		default:
			t.Fatal("unapproved host")
			return nil, nil
		}
	})}
	s, err := NewService(setupManifest(t, nil), client, func() time.Time { return current })
	if err != nil {
		t.Fatal(err)
	}
	cookie, err := s.Collect(context.Background(), "org-1", input("cookiebot"))
	if err != nil || cookie.Status != "connected" || cookie.Cookiebot.Consents.ReportedDays != 1 || cookie.Cookiebot.Consents.Totals.OptIn != 12 || *cookie.Cookiebot.Inventory.Total != 2 || cookie.Cookiebot.Inventory.Categories.Statistics != 1 {
		t.Fatalf("%+v %v", cookie, err)
	}
	algolia, err := s.Collect(context.Background(), "org-1", input("algolia"))
	if err != nil || algolia.Status != "connected" || *algolia.Algolia.Analytics.Searches != 10 || **algolia.Algolia.Analytics.NoResultRate != 0.2 {
		t.Fatalf("%+v %v", algolia, err)
	}
	for _, r := range []Report{cookie, algolia} {
		raw, _ := json.Marshal(r)
		for _, forbidden := range []string{"PRIVATE", "private-", strings.Repeat("a", 32), strings.Repeat("b", 32), "Countries", "cookies"} {
			if strings.Contains(string(raw), forbidden) {
				t.Fatal("sensitive upstream data crossed boundary")
			}
		}
	}
	if len(called) != 3 {
		t.Fatal("unexpected calls")
	}
}
func TestIdentityAndRevisionAreBoundBeforeAnyRequest(t *testing.T) {
	s, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) { t.Fatal("cross-scope fetch"); return nil, nil })}, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		org string
		in  Input
	}{{"org-other", input("cookiebot")}, {"org-1", Input{WebsiteID: website, Site: "https://triodelab.no/", WebsiteRevision: 8, Provider: "algolia"}}} {
		r, err := s.Collect(context.Background(), tc.org, tc.in)
		if err != nil || r.Status != "not_configured" {
			t.Fatal("scope binding")
		}
	}
}
