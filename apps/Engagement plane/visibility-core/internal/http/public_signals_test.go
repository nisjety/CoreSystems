package http

import (
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type publicOwner struct {
	registrySource
	jobs *publicsignals.Service
}

func (s publicOwner) PublicSignals() *publicsignals.Service { return s.jobs }
func TestPublicSignalsStrictHTTPAndWebsiteEnrollment(t *testing.T) {
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org_public")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	store, err := publicsignals.Open(filepath.Join(dir, "public.json"), "org_public")
	if err != nil {
		t.Fatal(err)
	}
	jobs := publicsignals.New(store, registry, nil, nil)
	site, _, _ := registry.Add(t.Context(), "https://triodelab.no/")
	handler := NewHandler("key", publicOwner{registrySource{registry}, jobs})
	base := "/api/v1/marketing/public-signals"
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d}`, site.ID, site.Site, site.Revision)
	for _, tc := range []struct {
		method, path, body, key string
		status                  int
	}{{"GET", base + query, "", "", 401}, {"GET", base + query, "", "key", 200}, {"GET", base + query + "&org=x", "", "key", 400}, {"GET", base + query, "{}", "key", 400}, {"POST", base + query, body, "key", 400}, {"POST", base, strings.Replace(body, `"websiteRevision":2`, `"websiteRevision":999`, 1), "key", 409}, {"PATCH", base, strings.TrimSuffix(body, "}") + `,"enabled":null}`, "key", 400}, {"POST", base, body, "key", 202}, {"PATCH", base, strings.TrimSuffix(body, "}") + `,"enabled":false}`, "key", 200}, {"DELETE", base, "", "key", 405}} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", tc.key)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatal(tc.method, tc.path, w.Code, tc.status, w.Body.String())
		}
		if tc.method == "GET" && tc.status == 200 && !strings.Contains(w.Body.String(), `"status":"idle"`) {
			t.Fatal("GETstartedjob")
		}
	}
	r := httptest.NewRequest("POST", "/api/v1/marketing/websites", strings.NewReader(`{"site":"https://other.no/"}`))
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("X-Internal-API-Key", "key")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	if w.Code != 201 {
		t.Fatal(w.Code, w.Body.String())
	}
	found := false
	for _, s := range registry.List() {
		if s.Site == "https://other.no/" {
			v, err := jobs.Get(s.ID, s.Site)
			if err != nil || v.Status != "queued" {
				t.Fatal(v, err)
			}
			found = true
		}
	}
	if !found {
		t.Fatal("siteabsent")
	}
}
