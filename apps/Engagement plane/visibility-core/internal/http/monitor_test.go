package http

import (
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type monitorOwner struct {
	registrySource
	monitoring *siteaudit.Monitor
}

func (s monitorOwner) Monitoring() *siteaudit.Monitor { return s.monitoring }

func TestMonitoringHTTPRequiresAuthorityAndStrictOptimisticInput(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	audits, err := siteaudit.Open(filepath.Join(dir, "audits.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	monitor := siteaudit.NewMonitor(siteaudit.New(audits, registry, nil, nil))
	handler := NewHandler("test-key", monitorOwner{registrySource: registrySource{registry: registry}, monitoring: monitor})
	base := "/api/v1/marketing/monitoring"
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := `{"websiteId":"` + w.ID + `","site":"` + w.Site + `","websiteRevision":` + strconv.FormatInt(w.Revision, 10) + `,"scheduleRevision":0,"enabled":false,"cadence":"daily"}`
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{
		{"GET", base + query, "", "", "", 401},
		{"GET", base + query, "", "test-key", "", 200},
		{"GET", base + query + "&organizationId=other", "", "test-key", "", 400},
		{"GET", base + query + "&site=https%3A%2F%2Fevil.no%2F", "", "test-key", "", 400},
		{"GET", base + "?websiteId=" + w.ID + "&site=https%3A%2F%2Fevil.no%2F", "", "test-key", "", 409},
		{"PATCH", base, strings.Replace(body, `"cadence":"daily"`, `"cadence":"hourly"`, 1), "test-key", "application/json", 400},
		{"PATCH", base, strings.Replace(body, `"enabled":false`, `"enabled":true`, 1), "test-key", "application/json", 503},
		{"PATCH", base, body, "test-key", "text/plain", 415},
		{"PATCH", base + query, body, "test-key", "application/json", 400},
		{"PATCH", base, strings.TrimSuffix(body, "}") + `,"organizationId":"other"}`, "test-key", "application/json", 400},
		{"PATCH", base, strings.TrimSuffix(body, "}") + `,"enabled":true}`, "test-key", "application/json", 400},
		{"PATCH", base, strings.Replace(body, `"scheduleRevision":0`, `"scheduleRevision":-1`, 1), "test-key", "application/json", 400},
		{"PATCH", base, strings.Replace(body, `"scheduleRevision":0`, `"scheduleRevision":null`, 1), "test-key", "application/json", 400},
		{"PATCH", base, strings.Replace(body, `"scheduleRevision":0`, `"scheduleRevision":0.5`, 1), "test-key", "application/json", 400},
		{"PATCH", base, strings.Replace(body, `"scheduleRevision":0`, `"scheduleRevision":"0"`, 1), "test-key", "application/json", 400},
		{"POST", base, body, "test-key", "application/json", 405},
		{"PATCH", base, body, "test-key", "application/json", 200},
		{"PATCH", base, body, "test-key", "application/json", 409},
	} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("X-Internal-API-Key", tc.key)
		r.Header.Set("Content-Type", tc.media)
		result := httptest.NewRecorder()
		handler.ServeHTTP(result, r)
		if result.Code != tc.status {
			t.Fatalf("%s %s got%d want%d %s", tc.method, tc.path, result.Code, tc.status, result.Body.String())
		}
		if result.Code == 200 {
			var value struct {
				Data siteaudit.MonitorSnapshot `json:"data"`
			}
			if json.Unmarshal(result.Body.Bytes(), &value) != nil || value.Data.Source != "marketing_scheduler" || result.Header().Get("Cache-Control") != "no-store" || strings.Contains(result.Body.String(), "organizationId") {
				t.Fatal(result.Body.String())
			}
		}
	}
}
