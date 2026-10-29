package http

import (
	"context"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type collectionOwner struct {
	mappingOwner
	sources *sitesources.Service
}

func (s collectionOwner) SiteSources() *sitesources.Service { return s.sources }
func TestCollectionEndpointAndMappingEnqueue(t *testing.T) {
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org_map")
	if err != nil {
		t.Fatal(err)
	}
	r := websites.NewService(repo, nil, nil)
	w, _, err := r.Add(context.Background(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	m, err := algoliamapping.New(filepath.Join(dir, "maps.json"), "org_map", r, mappingClient{})
	if err != nil {
		t.Fatal(err)
	}
	s, err := sitesources.NewWithOAuth(filepath.Join(dir, "sources.json"), "org_map", r, nil, m, nil)
	if err != nil {
		t.Fatal(err)
	}
	h := NewHandler("key", collectionOwner{mappingOwner{registrySource{r}, m}, s})
	base := "/api/v1/marketing/site-sources/collection"
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F&provider=algolia"
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"provider":"algolia"}`, w.ID, w.Site, w.Revision)
	for _, tc := range []struct {
		method, path, body, key string
		status                  int
	}{
		{"GET", base + query, "", "", 401}, {"GET", base + query, "", "key", 200}, {"GET", base + query + "&orgId=evil", "", "key", 400},
		{"POST", base, body, "key", 400}, {"POST", base + query, body, "key", 400}, {"PUT", base + query, "", "key", 405},
	} {
		req := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", tc.key)
		res := httptest.NewRecorder()
		h.ServeHTTP(res, req)
		if res.Code != tc.status {
			t.Fatal(tc, res.Code, res.Body.String())
		}
	}
	mapping := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"connectionId":"conn_1234","applicationId":"APP123","indexName":"public_index"}`, w.ID, w.Site, w.Revision)
	req := httptest.NewRequest("PUT", "/api/v1/marketing/site-sources/algolia-mapping", strings.NewReader(mapping))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "key")
	res := httptest.NewRecorder()
	h.ServeHTTP(res, req)
	if res.Code != 200 {
		t.Fatal(res.Code, res.Body.String())
	}
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "queued" {
		t.Fatal("mapping did not queue first sync", state, err)
	}
	req = httptest.NewRequest("POST", base, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "key")
	res = httptest.NewRecorder()
	h.ServeHTTP(res, req)
	if res.Code != 202 {
		t.Fatal(res.Code, res.Body.String())
	}
}
