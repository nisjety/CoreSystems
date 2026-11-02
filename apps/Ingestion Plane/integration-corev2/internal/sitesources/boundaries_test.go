package sitesources

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func payloadService(t *testing.T, consent, inventory, algolia string) *Service {
	t.Helper()
	s, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(r *http.Request) (*http.Response, error) {
		if r.URL.Host == "analytics.de.algolia.com" {
			return response(algolia), nil
		}
		if strings.HasSuffix(r.URL.Path, "/cookies") {
			return response(inventory), nil
		}
		return response(consent), nil
	})}, func() time.Time { return current })
	if err != nil {
		t.Fatal(err)
	}
	return s
}
func TestExplicitEmptyResultsAreNotConnectionFailures(t *testing.T) {
	var consent map[string]any
	_ = json.Unmarshal([]byte(consentJSON), &consent)
	consent["consentstat"] = map[string]any{"consentday": []any{}}
	emptyConsent, _ := json.Marshal(consent)
	var inventory map[string]any
	_ = json.Unmarshal([]byte(inventoryJSON), &inventory)
	inventory["cookies"] = []any{}
	emptyInventory, _ := json.Marshal(inventory)
	s := payloadService(t, string(emptyConsent), string(emptyInventory), `{"count":0,"noResultCount":0,"rate":null,"dates":[]}`)
	cookie, err := s.Collect(context.Background(), "org-1", input("cookiebot"))
	if err != nil || cookie.Status != "connected" || cookie.Cookiebot.Consents.Status != "no_data" || cookie.Cookiebot.Consents.Totals != nil || cookie.Cookiebot.Consents.ReportedDays != 0 || cookie.Cookiebot.Inventory.Status != "connected" || *cookie.Cookiebot.Inventory.Total != 0 || cookie.Cookiebot.Inventory.Categories == nil {
		t.Fatalf("%+v %v", cookie, err)
	}
	algolia, err := s.Collect(context.Background(), "org-1", input("algolia"))
	if err != nil || algolia.Status != "connected" || *algolia.Algolia.Analytics.Searches != 0 || algolia.Algolia.Analytics.NoResultRate == nil || *algolia.Algolia.Analytics.NoResultRate != nil {
		t.Fatal("observed zero must be explicit null rate")
	}
	raw, _ := json.Marshal(algolia)
	if !strings.Contains(string(raw), `"noResultRate":null`) {
		t.Fatal("null rate omitted")
	}
}
func TestMalformedConsentNeverBecomesZeroOrResolved(t *testing.T) {
	for _, raw := range []string{
		`{}`, strings.Replace(consentJSON, "www.triodelab.no", "other.no", 1), strings.Replace(consentJSON, "2026-10-16T00:00:00", "2026-10-17T00:00:00", 1), strings.Replace(consentJSON, `"urlpath":"/"`, `"urlpath":"/private"`, 1), strings.Replace(consentJSON, `"OptIn":"12"`, `"OptIn":null`, 1), strings.Replace(consentJSON, `"OptIn":"12"`, `"OptIn":"-1"`, 1), strings.Replace(consentJSON, `"OptIn":"12"`, `"OptIn":"9007199254740992"`, 1), strings.Replace(consentJSON, `"Date":"2026-10-22T00:00:00"`, `"Date":"2026-10-23T00:00:00"`, 1), strings.Replace(consentJSON, `"OptIn":"12"`, `"OptIn":"12","OptIn":"0"`, 1),
	} {
		s := payloadService(t, raw, inventoryJSON, algoliaJSON)
		r, err := s.Collect(context.Background(), "org-1", input("cookiebot"))
		if err != nil || r.Status != "partial" || r.Cookiebot.Consents.Status != "unavailable" || r.Cookiebot.Consents.Totals != nil {
			t.Fatal("malformed consent accepted")
		}
	}
}
func TestMalformedInventoryNeverBecomesCompliance(t *testing.T) {
	for _, raw := range []string{`{}`, strings.Replace(inventoryJSON, `"Category":"1"`, `"Category":"6"`, 1), strings.Replace(inventoryJSON, `"ThirdParty":"0"`, `"ThirdParty":"2"`, 1), strings.Replace(inventoryJSON, `"Secure":"1"`, `"Secure":true`, 1), strings.Replace(inventoryJSON, "2026-10-20", "2026-10-24", 1), strings.Replace(inventoryJSON, "www.triodelab.no", "evil.no", 1), strings.Replace(inventoryJSON, `"urlpath":"/"`, `"urlpath":"/private"`, 1), strings.Replace(inventoryJSON, `"culture":"en"`, `"culture":"UPPER"`, 1)} {
		s := payloadService(t, consentJSON, raw, algoliaJSON)
		r, err := s.Collect(context.Background(), "org-1", input("cookiebot"))
		if err != nil || r.Status != "partial" || r.Cookiebot.Inventory.Status != "unavailable" || r.Cookiebot.Inventory.Total != nil || r.Cookiebot.Inventory.Categories != nil {
			t.Fatal("malformed inventory accepted")
		}
	}
}
func TestAlgoliaCountsRatesCoverageAndSafeIntegers(t *testing.T) {
	for _, raw := range []string{`{}`, strings.Replace(algoliaJSON, `"count":10`, `"count":11`, 1), strings.Replace(algoliaJSON, `"count":10`, `"count":9007199254740992`, 1), strings.Replace(algoliaJSON, `"noResultCount":2`, `"noResultCount":11`, 1), strings.Replace(algoliaJSON, `"rate":0.2`, `"rate":null`, 1), strings.Replace(algoliaJSON, `"rate":0.2`, `"rate":0.8`, 1), strings.Replace(algoliaJSON, "2026-10-22", "2026-10-23", 1), `{"count":1,"noResultCount":0,"rate":0,"dates":[]}`, `{"count":0,"noResultCount":0,"rate":null,"dates":null}`} {
		r, err := payloadService(t, consentJSON, inventoryJSON, raw).Collect(context.Background(), "org-1", input("algolia"))
		if err != nil || r.Status != "unavailable" || r.Algolia.Analytics.Status != "unavailable" || r.Algolia.Analytics.Searches != nil || r.Algolia.Analytics.NoResultRate != nil {
			t.Fatal("malformed analytics accepted")
		}
	}
}
func TestProtectedBindingsAndFilesFailClosed(t *testing.T) {
	for _, tc := range []struct {
		name   string
		mutate func(map[string]any)
	}{
		{"domain", func(m map[string]any) { m["cookiebot"].(map[string]any)["domain"] = "evil.no" }},
		{"uuid", func(m map[string]any) { m["cookiebot"].(map[string]any)["domainGroupId"] = "wrong" }},
		{"culture", func(m map[string]any) { m["cookiebot"].(map[string]any)["culture"] = "../en" }},
		{"region", func(m map[string]any) { m["algolia"].(map[string]any)["region"] = "evil.com" }},
		{"index", func(m map[string]any) { m["algolia"].(map[string]any)["indexName"] = "index?key=x" }},
		{"app", func(m map[string]any) { m["algolia"].(map[string]any)["applicationId"] = "https://evil" }},
		{"extra", func(m map[string]any) { m["organizationIdOverride"] = "other" }},
		{"case", func(m map[string]any) { m["WebsiteID"] = m["websiteId"]; delete(m, "websiteId") }},
		{"relative-key", func(m map[string]any) { m["algolia"].(map[string]any)["analyticsKeyFile"] = "relative" }},
		{"null-provider", func(m map[string]any) { m["algolia"] = nil }},
		{"bad-key-mode", func(m map[string]any) {
			if err := os.Chmod(m["cookiebot"].(map[string]any)["apiKeyFile"].(string), 0644); err != nil {
				t.Fatal(err)
			}
		}},
		{"bad-key", func(m map[string]any) {
			if err := os.WriteFile(m["cookiebot"].(map[string]any)["apiKeyFile"].(string), []byte("key/secret?evil"), 0600); err != nil {
				t.Fatal(err)
			}
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := NewService(setupManifest(t, tc.mutate), nil, nil); err == nil {
				t.Fatal("invalid bindings accepted")
			}
		})
	}
	path := setupManifest(t, nil)
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := NewService(path, nil, nil); err == nil {
		t.Fatal("public manifest")
	}
	if err := os.Chmod(path, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := NewService(link, nil, nil); err == nil {
		t.Fatal("symlink manifest")
	}
}
func TestRevocationStopsSecondCookiebotCallAndDiscardsReceipt(t *testing.T) {
	path := setupManifest(t, nil)
	calls := 0
	s, err := NewService(path, &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		calls++
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
		return response(consentJSON), nil
	})}, func() time.Time { return current })
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.Collect(context.Background(), "org-1", input("cookiebot")); err != ErrUnavailable || calls != 1 {
		t.Fatalf("revocation crossed: calls%d err%v", calls, err)
	}
}
func TestFailureStatusesAreSanitizedAndNoRedirects(t *testing.T) {
	for _, tc := range []struct {
		code   int
		status string
	}{{401, "access_denied"}, {403, "access_denied"}, {402, "plan_required"}, {429, "rate_limited"}, {500, "unavailable"}, {302, "unavailable"}} {
		s, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
			r := response("secret raw error")
			r.StatusCode = tc.code
			r.Header.Set("Location", "https://evil.no")
			return r, nil
		})}, nil)
		if err != nil {
			t.Fatal(err)
		}
		if s.client.CheckRedirect(nil, nil) != http.ErrUseLastResponse {
			t.Fatal("redirect allowed")
		}
		r, err := s.Collect(context.Background(), "org-1", input("algolia"))
		if err != nil || r.Status != "unavailable" || r.Algolia.Analytics.Status != tc.status {
			t.Fatal("wrong provider failure")
		}
		raw, _ := json.Marshal(r)
		if strings.Contains(string(raw), "secret") {
			t.Fatal("raw provider error escaped")
		}
	}
	s, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) { return nil, errors.New("https://secret-key-in-url") })}, nil)
	if err != nil {
		t.Fatal(err)
	}
	r, err := s.Collect(context.Background(), "org-1", input("algolia"))
	if err != nil || r.Status != "unavailable" {
		t.Fatal("transport failure")
	}
	r, err = payloadService(t, consentJSON, inventoryJSON, strings.Repeat("x", (2<<20)+1)).Collect(context.Background(), "org-1", input("algolia"))
	if err != nil || r.Status != "unavailable" {
		t.Fatal("oversized response")
	}
}
func TestGlobalAdmissionAndStrictInput(t *testing.T) {
	started := make(chan struct{}, 2)
	release := make(chan struct{})
	var wg sync.WaitGroup
	s, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		started <- struct{}{}
		<-release
		return response(algoliaJSON), nil
	})}, func() time.Time { return current })
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = s.Collect(context.Background(), "org-1", input("algolia")) }()
	}
	<-started
	<-started
	if _, err := s.Collect(context.Background(), "org-1", input("algolia")); err != ErrBusy {
		t.Fatal("admission")
	}
	close(release)
	wg.Wait()
	valid := `{"websiteId":"` + website + `","site":"https://triodelab.no/","websiteRevision":7,"provider":"algolia"}`
	for _, raw := range []string{`null`, valid + `{}`, strings.Replace(valid, `"provider":"algolia"`, `"provider":"other"`, 1), strings.Replace(valid, `"websiteRevision":7`, `"websiteRevision":7,"websiteRevision":8`, 1), strings.Replace(valid, `"websiteId"`, `"WebsiteId"`, 1)} {
		if _, err := ParseInput([]byte(raw)); err == nil {
			t.Fatal("strict input")
		}
	}
	if _, err := ParseInput([]byte(valid)); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Collect(context.Background(), "", input("algolia")); err != ErrInvalid {
		t.Fatal("missing organization")
	}
}

func TestUnconfiguredReceiptUsesOneClockAndEmptyManifestIsValid(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bindings.json")
	if err := os.WriteFile(path, []byte(`{"version":1,"installations":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	calls := 0
	first := time.Date(2026, 10, 23, 23, 59, 59, 0, time.UTC)
	s, err := NewService(path, nil, func() time.Time { calls++; return first.Add(time.Duration(calls-1) * time.Second) })
	if err != nil {
		t.Fatal(err)
	}
	r, err := s.Collect(context.Background(), "org-1", input("algolia"))
	if err != nil || calls != 1 || r.ObservedAt != "2026-10-23T23:59:59Z" || r.Window != period(first) || r.Status != "not_configured" {
		t.Fatalf("inconsistent receipt %+v calls%d err%v", r, calls, err)
	}
}
