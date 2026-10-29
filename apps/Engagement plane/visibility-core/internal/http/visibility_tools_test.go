package http

import (
	"context"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/competitors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/visibilityanalysis"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type visibilityToolsOwner struct {
	registrySource
	comparisons *competitors.Service
	reads       int
}

func (s *visibilityToolsOwner) Competitors() *competitors.Service { return s.comparisons }
func (s *visibilityToolsOwner) VisibilityAnalysis(_ context.Context, id, site, quarter string) (visibilityanalysis.Snapshot, error) {
	w, err := s.registry.Resolve(id, site)
	if err != nil {
		return visibilityanalysis.Snapshot{}, err
	}
	s.reads++
	return visibilityanalysis.Snapshot{WebsiteID: w.Website.ID, Site: w.Website.Site, WebsiteRevision: w.Website.Revision, Source: "visibility_analysis"}, nil
}
func TestVisibilityToolsStrictHTTPBoundaries(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	r := websites.NewService(repo, nil, nil)
	w, _, err := r.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	store, err := competitors.Open(filepath.Join(dir, "comparisons.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	owner := &visibilityToolsOwner{registrySource: registrySource{registry: r}, comparisons: competitors.New(store, r, nil, time.Now)}
	h := NewHandler("test-key", owner)
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"target":"https://competitor.no/","operation":"refresh"}`, w.ID, w.Site, w.Revision)
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{
		{"GET", "/api/v1/visibility/analysis" + query + "&quarter=Q4", "", "", "", 401},
		{"GET", "/api/v1/visibility/analysis" + query + "&quarter=Q4", "", "test-key", "", 200},
		{"GET", "/api/v1/visibility/analysis" + query + "&quarter=all", "", "test-key", "", 400},
		{"GET", "/api/v1/visibility/analysis" + query + "&quarter=Q4&orgId=other", "", "test-key", "", 400},
		{"POST", "/api/v1/visibility/analysis", "{}", "test-key", "application/json", 405},
		{"GET", "/api/v1/visibility/competitors" + query, "", "test-key", "", 200},
		{"GET", "/api/v1/visibility/competitors" + strings.Replace(query, "triodelab.no", "other.no", 1), "", "test-key", "", 409},
		{"POST", "/api/v1/visibility/competitors", body, "", "application/json", 401},
		{"POST", "/api/v1/visibility/competitors", body, "test-key", "text/plain", 415},
		{"POST", "/api/v1/visibility/competitors" + query, body, "test-key", "application/json", 400},
		{"POST", "/api/v1/visibility/competitors", strings.TrimSuffix(body, "}") + `,"token":"private"}`, "test-key", "application/json", 400},
		{"POST", "/api/v1/visibility/competitors", strings.Replace(body, `"refresh"`, `"publish"`, 1), "test-key", "application/json", 400},
		{"POST", "/api/v1/visibility/competitors", strings.Replace(body, "https://competitor.no/", "https://localhost/", 1), "test-key", "application/json", 400},
		{"POST", "/api/v1/visibility/competitors", body, "test-key", "application/json", 200},
	} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("X-Internal-API-Key", tc.key)
		r.Header.Set("Content-Type", tc.media)
		out := httptest.NewRecorder()
		h.ServeHTTP(out, r)
		if out.Code != tc.status {
			t.Fatalf("%s %s: %d want%d %s", tc.method, tc.path, out.Code, tc.status, out.Body.String())
		}
	}
	if owner.reads != 1 {
		t.Fatalf("unexpected analysis execution: %d", owner.reads)
	}
	value, err := owner.comparisons.Get(w.ID, w.Site)
	if err != nil || len(value.Entries) != 1 || value.Entries[0].Status != "queued" {
		t.Fatal(value, err)
	}
}
