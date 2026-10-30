package http

import (
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type preparationOwner struct {
	registrySource
	jobs *preparation.Service
}

func (s preparationOwner) Preparation() *preparation.Service { return s.jobs }
func TestPreparationHTTPGetIsPureAndQueueIsExplicitAndScoped(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	store, err := preparation.Open(filepath.Join(dir, "preparation.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	jobs := preparation.New(store, registry, nil, nil)
	handler := NewHandler("test-key", preparationOwner{registrySource{registry: registry}, jobs})
	base := "/api/v1/marketing/preparation"
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := `{"websiteId":"` + site.ID + `","site":"https://triodelab.no/","websiteRevision":2}`
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{
		{"GET", base + query, "", "", "", 401}, {"GET", base + query, "", "test-key", "", 200},
		{"GET", base + query + "&token=secret", "", "test-key", "", 400},
		{"GET", strings.Replace(base+query, "triodelab.no", "other.no", 1), "", "test-key", "", 409},
		{"POST", base, body, "test-key", "text/plain", 415},
		{"POST", base + query, body, "test-key", "application/json", 400},
		{"POST", base, strings.Replace(body, `:2}`, `:999}`, 1), "test-key", "application/json", 409},
		{"POST", base, strings.TrimSuffix(body, "}") + `,"orgId":"other"}`, "test-key", "application/json", 400},
		{"POST", base, body, "test-key", "application/json", 202}, {"POST", base, body, "test-key", "application/json", 202},
		{"PATCH", base, body, "test-key", "application/json", 405},
	} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("X-Internal-API-Key", tc.key)
		r.Header.Set("Content-Type", tc.media)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatalf("%s %s: %d expected%d %s", tc.method, tc.path, w.Code, tc.status, w.Body.String())
		}
		if tc.method == "GET" && tc.status == 200 && !strings.Contains(w.Body.String(), `"status":"idle"`) {
			t.Fatal("status read initiated a job")
		}
	}
	value, err := jobs.Get(site.ID, site.Site)
	if err != nil || value.Status != "queued" {
		t.Fatal("queue not persisted", err)
	}
	// A successful URL registration automatically queues public preparation;
	// the request itself performs no page captures and has no worker attached.
	r := httptest.NewRequest("POST", "/api/v1/marketing/websites", strings.NewReader(`{"site":"https://other.no/"}`))
	r.Header.Set("X-Internal-API-Key", "test-key")
	r.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	var envelope struct {
		Data websites.Website `json:"data"`
	}
	_ = json.Unmarshal(w.Body.Bytes(), &envelope)
	if w.Code != 201 {
		t.Fatal(w.Code, w.Body.String())
	}
	value, err = jobs.Get(envelope.Data.ID, envelope.Data.Site)
	if err != nil || value.Status != "queued" {
		t.Fatal("new URL not prepared asynchronously", err)
	}
}
