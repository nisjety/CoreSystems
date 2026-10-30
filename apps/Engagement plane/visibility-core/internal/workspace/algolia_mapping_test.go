package workspace

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestAlgoliaMappingClientFixedSignedEndpointsAndStrictReceipts(t *testing.T) {
	r := workspaceRegistry(t, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}}
	})
	w, _, _ := r.Add(t.Context(), "https://triodelab.no/")
	record, _ := r.Resolve(w.ID, w.Site)
	mode := "good"
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(wr http.ResponseWriter, req *http.Request) {
		calls++
		if req.Method != "POST" || req.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("wrong signed boundary")
		}
		if mode == "redirect" {
			http.Redirect(wr, req, "https://evil.example", 302)
			return
		}
		if mode == "status" {
			wr.WriteHeader(503)
			return
		}
		if mode == "unknown" {
			_, _ = wr.Write([]byte(`{"success":true,"data":{},"token":"bad"}`))
			return
		}
		var body map[string]any
		_ = json.NewDecoder(req.Body).Decode(&body)
		switch req.URL.Path {
		case "/api/v1/insights/algolia/access":
			if len(body) != 1 || body["connectionId"] != "conn_1234" {
				t.Error(body)
			}
			_ = json.NewEncoder(wr).Encode(map[string]any{"success": true, "data": map[string]string{"connectionId": "conn_1234", "status": "active", "generation": "grant-1"}})
		case "/api/v1/insights/algolia/resources":
			if len(body) != 3 || body["applicationId"] != "APP123" || body["indexName"] != "triodelab" {
				t.Error(body)
			}
			_ = json.NewEncoder(wr).Encode(map[string]any{"success": true, "data": map[string]string{"connectionId": "conn_1234", "applicationId": "APP123", "indexName": "triodelab", "status": "allowed"}})
		case "/api/v1/insights/algolia/report":
			if len(body) != 6 || body["websiteId"] != w.ID || body["site"] != w.Site || body["websiteRevision"] != float64(w.Revision) {
				t.Error(body)
			}
			now := time.Now().UTC()
			day := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
			report := sitesources.Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Provider: "algolia", Status: "unavailable", ObservedAt: now.Format(time.RFC3339Nano), Window: sitesources.Window{StartDate: day.AddDate(0, 0, -7).Format("2006-01-02"), EndDate: day.AddDate(0, 0, -1).Format("2006-01-02")}, Algolia: &sitesources.Algolia{IndexName: "triodelab", Region: "provider", Analytics: sitesources.Analytics{Status: "unavailable", ReportedDays: 0}}}
			_ = json.NewEncoder(wr).Encode(map[string]any{"success": true, "data": report})
		default:
			t.Error("unexpected endpoint", req.URL.Path)
		}
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	_ = os.WriteFile(path, []byte(syntheticBearer), 0600)
	c := NewAlgoliaMappingClient(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, r)
	if a, err := c.Access(t.Context(), "conn_1234"); err != nil || a.Generation != "grant-1" {
		t.Fatal(a, err)
	}
	if yes, err := c.Allowed(t.Context(), "conn_1234", "APP123", "triodelab"); err != nil || !yes {
		t.Fatal(yes, err)
	}
	m := algoliamapping.Mapping{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Status: "mapped", ConnectionID: "conn_1234", ApplicationID: "APP123", IndexName: "triodelab"}
	if result, err := c.Report(t.Context(), record, m); err != nil || result.Algolia.Region != "provider" {
		t.Fatal(result, err)
	}
	for _, bad := range []string{"redirect", "status", "unknown"} {
		mode = bad
		if _, err := c.Access(t.Context(), "conn_1234"); err == nil {
			t.Fatal(bad)
		}
	}
	before := calls
	_, _ = r.MapClarity(w.ID, "abc123", w.Revision)
	if _, err := c.Report(t.Context(), record, m); err == nil || calls != before {
		t.Fatal("stale report reached Integration")
	}
	_ = os.WriteFile(path, []byte("header.e30.signature"), 0600)
	if _, err := c.Access(t.Context(), "conn_1234"); err == nil || calls != before {
		t.Fatal("wrong-org credential reached Integration")
	}
}
