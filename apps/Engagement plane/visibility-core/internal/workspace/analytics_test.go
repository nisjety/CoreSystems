package workspace

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestWorkspaceGA4UsesPrivateMappingAndNeverAnotherWebsiteGrant(t *testing.T) {
	registry := workspaceRegistry(t, func(_ context.Context, site string) websites.Evidence {
		ga := websites.Access{Status: "requires_consent"}
		if site == "https://example.com/" {
			ga = websites.Access{Status: "connected", ConnectionID: "private-ga4-connection", PropertyID: "123", StreamID: "456", MeasurementID: "G-ABCDE123"}
		}
		return websites.Evidence{SearchConsole: websites.Access{Status: "needs_connection"}, GA4: ga}
	})
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := registry.Add(t.Context(), "https://other.example/"); err != nil {
		t.Fatal(err)
	}
	var calls int
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.URL.Path != "/api/v1/connections/private-ga4-connection/actions" {
			t.Error("wrong connection used")
		}
		w.WriteHeader(503)
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	service := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path, GoogleConnectionID: "wrong-legacy-connection"}, registry)
	if got := service.GetGA4(t.Context(), "Q1", website.Site); got.Reason != "source_unavailable" || calls != 1 {
		t.Fatal("private GA4mapping notused")
	}
	if got := service.GetGA4(t.Context(), "Q1", "https://other.example/"); got.Reason != "requires_consent" || calls != 1 {
		t.Fatal("otherwebsite grant leaked")
	}
	if got := service.GetGA4(t.Context(), "Q1", "https://missing.example/"); got.Reason != "not_found" || calls != 1 {
		t.Fatal("unregistered site reached broker")
	}
	if err := os.WriteFile(path, []byte("header.e30.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	if got := service.GetGA4(t.Context(), "Q1", website.Site); got.Reason != "not_configured" || calls != 1 {
		t.Fatal("wrong-orgtoken reached broker")
	}
}

func TestWorkspaceGA4UsesExplicitWWWOnlyForOwnedRegisteredWebsite(t *testing.T) {
	registry := workspaceRegistry(t, func(_ context.Context, site string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "gsc-one", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}, GA4: websites.Access{Status: "connected", ConnectionID: "private-ga4-one", PropertyID: "560102923", StreamID: "16177443323", MeasurementID: "G-ZEWPHF6X3K"}}
	})
	website, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	website, err = registry.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", website.Revision)
	if err != nil {
		t.Fatal(err)
	}
	requests := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		if r.URL.Path != "/api/v1/connections/private-ga4-one/actions" {
			t.Errorf("wrong connection path %s", r.URL.Path)
		}
		var command struct {
			Operation string         `json:"operation"`
			Params    map[string]any `json:"params"`
		}
		if err := json.NewDecoder(r.Body).Decode(&command); err != nil {
			t.Fatal(err)
		}
		if command.Operation != "ga4.report.run" || command.Params["site"] != website.Site || command.Params["reportingSite"] != "https://www.triodelab.no/" || command.Params["streamId"] != "16177443323" {
			t.Errorf("unsafe mapping: %+v", command)
		}
		w.WriteHeader(503)
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	service := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, registry)
	if result := service.GetGA4(t.Context(), "Q1", website.Site); result.Status != pulse.StatusUnavailable || result.Totals != nil || requests != 1 {
		t.Fatalf("unexpected result %+v requests=%d", result, requests)
	}
	if result := service.GetGA4(t.Context(), "Q1", "https://other.example/"); result.Status != pulse.StatusUnavailable || requests != 1 {
		t.Fatal("unregistered website reused GA4 mapping")
	}
}
func TestWorkspaceClarityChecksExpectedProjectButAllowsApprovedOperatorBinding(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	project := "abc123"
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"site": website.Site, "source": "microsoft_clarity", "status": "connected", "scope": "clarity_project", "projectId": project, "window": map[string]any{"numOfDays": 3, "timeZone": "UTC"}, "metrics": map[string]any{"sessions": 12, "botSessions": nil, "users": nil, "pagesPerSession": nil}}})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	service := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, registry)
	if got := service.GetClarity(t.Context(), website.Site); got.Status != "connected" {
		t.Fatalf("approved operatorbinding blocked: %+v", got)
	}
	if website, err = registry.MapClarity(website.ID, project, website.Revision); err != nil {
		t.Fatal(err)
	}
	if got := service.GetClarity(t.Context(), website.Site); got.Status != "connected" {
		t.Fatal("matchingpendingmapping blocked")
	}
	project = "other123"
	if got := service.GetClarity(t.Context(), website.Site); got.Status != "unavailable" || got.Metrics != nil {
		t.Fatal("differentClarityproject exposed")
	}
}
