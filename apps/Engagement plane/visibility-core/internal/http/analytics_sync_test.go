package http

import (
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type syncOwner struct {
	registrySource
	jobs *analyticssync.Service
}

func (s syncOwner) AnalyticsSync() *analyticssync.Service { return s.jobs }
func syncHTTPFixture(t *testing.T) (*websites.Service, *analyticssync.Service, syncOwner) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org_sync")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	store, err := analyticssync.Open(filepath.Join(dir, "analytics.json"), "org_sync")
	if err != nil {
		t.Fatal(err)
	}
	jobs := analyticssync.New(store, registry, nil, nil, nil)
	return registry, jobs, syncOwner{registrySource{registry}, jobs}
}
func TestAnalyticsSyncHTTPStrictScopedLifecycle(t *testing.T) {
	registry, jobs, source := syncHTTPFixture(t)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	handler := NewHandler("key", source)
	base := "/api/v1/marketing/analytics-sync"
	q := fmt.Sprintf("Q%d", (int(time.Now().UTC().Month())-1)/3+1)
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F&quarter=" + q
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"quarter":%q}`, site.ID, site.Site, site.Revision, q)
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{{"GET", base + query, "", "", "", 401}, {"GET", base + query, "", "key", "", 200}, {"GET", base + query + "&org=other", "", "key", "", 400}, {"GET", base + query + "&quarter=" + q, "", "key", "", 400}, {"GET", base + query, "{}", "key", "", 400}, {"GET", strings.Replace(base+query, "triodelab.no", "other.no", 1), "", "key", "", 409}, {"POST", base, body, "key", "text/plain", 415}, {"POST", base + query, body, "key", "application/json", 400}, {"POST", base, strings.Replace(body, `"websiteRevision":2`, `"websiteRevision":999`, 1), "key", "application/json", 409}, {"POST", base, strings.TrimSuffix(body, "}") + `,"organizationId":"other"}`, "key", "application/json", 400}, {"POST", base, body, "key", "application/json", 202}, {"PATCH", base, strings.TrimSuffix(body, "}") + `,"enabled":null}`, "key", "application/json", 400}, {"PATCH", base, strings.TrimSuffix(body, "}") + `,"enabled":false}`, "key", "application/json", 200}, {"DELETE", base, "", "key", "", 405}} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("X-Internal-API-Key", tc.key)
		r.Header.Set("Content-Type", tc.media)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatalf("%s %s =>%d want%d %s", tc.method, tc.path, w.Code, tc.status, w.Body.String())
		}
		if tc.method == "GET" && tc.status == 200 && !strings.Contains(w.Body.String(), `"status":"idle"`) {
			t.Fatal("GET queued data")
		}
	}
	v, err := jobs.Get(site.ID, site.Site, q)
	if err != nil || v.Enabled || v.Status != "queued" {
		t.Fatal(v, err)
	}
}
func TestWebsiteMutationsEnrollAnalyticsButGetDoesNot(t *testing.T) {
	registry, jobs, source := syncHTTPFixture(t)
	handler := NewHandler("key", source)
	request := func(method, path, body string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("X-Internal-API-Key", "key")
		r.Header.Set("Content-Type", "application/json")
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	w := request("POST", "/api/v1/marketing/websites", `{"site":"https://triodelab.no/"}`)
	if w.Code != 201 {
		t.Fatal(w.Code, w.Body.String())
	}
	var envelope struct {
		Data websites.Website `json:"data"`
	}
	_ = json.Unmarshal(w.Body.Bytes(), &envelope)
	site := envelope.Data
	q := fmt.Sprintf("Q%d", (int(time.Now().UTC().Month())-1)/3+1)
	v, err := jobs.Get(site.ID, site.Site, q)
	if err != nil || v.Status != "queued" || !v.Enabled {
		t.Fatal("notautomaticallyenrolled", v, err)
	}
	for _, action := range []string{"verify", "mappings", "change"} {
		record, _ := registry.Resolve(site.ID, "")
		site = record.Website
		path := "/api/v1/marketing/websites/" + site.ID
		method := "POST"
		body := "{}"
		switch action {
		case "verify":
			path += "/verify"
		case "mappings":
			path += "/mappings"
			method = "PATCH"
			body = fmt.Sprintf(`{"clarityProjectId":"abc123","revision":%d}`, site.Revision)
		case "change":
			method = "PATCH"
			body = fmt.Sprintf(`{"site":"https://other.no/","revision":%d}`, site.Revision)
		}
		w = request(method, path, body)
		if w.Code != 200 {
			t.Fatal(action, w.Code, w.Body.String())
		}
		_ = json.Unmarshal(w.Body.Bytes(), &envelope)
		site = envelope.Data
		v, err = jobs.Get(site.ID, site.Site, q)
		if err != nil || v.Status != "queued" || v.WebsiteRevision != site.Revision {
			t.Fatal(action, v, err)
		}
	}
	_ = request("GET", "/api/v1/marketing/websites", "")
	v2, _ := jobs.Get(site.ID, site.Site, q)
	if v2.JobID != v.JobID {
		t.Fatal("GETchangedjob")
	}
}
