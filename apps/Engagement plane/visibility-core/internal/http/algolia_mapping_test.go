package http

import (
	"context"
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type mappingOwner struct {
	registrySource
	mapping *algoliamapping.Service
}

func (o mappingOwner) AlgoliaMapping() *algoliamapping.Service { return o.mapping }

type mappingClient struct{}

func (mappingClient) Access(_ context.Context, id string) (algoliamapping.Access, error) {
	return algoliamapping.Access{ConnectionID: id, Status: "active", Generation: "grant-1"}, nil
}
func (mappingClient) Allowed(context.Context, string, string, string) (bool, error) { return true, nil }
func (mappingClient) Report(context.Context, websites.Record, algoliamapping.Mapping) (sitesources.Report, error) {
	return sitesources.Report{}, nil
}
func TestAlgoliaMappingStrictHTTP(t *testing.T) {
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org_map")
	if err != nil {
		t.Fatal(err)
	}
	r := websites.NewService(repo, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}}
	}, nil)
	w, _, _ := r.Add(t.Context(), "https://triodelab.no/")
	m, err := algoliamapping.New(filepath.Join(dir, "maps.json"), "org_map", r, mappingClient{})
	if err != nil {
		t.Fatal(err)
	}
	h := NewHandler("key", mappingOwner{registrySource{r}, m})
	base := "/api/v1/marketing/site-sources/algolia-mapping"
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	identity := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d}`, w.ID, w.Site, w.Revision)
	body := strings.TrimSuffix(identity, "}") + `,"connectionId":"conn_1234","applicationId":"APP123","indexName":"triodelab"}`
	for _, tc := range []struct {
		method, path, body, key string
		status                  int
	}{
		{"GET", base + query, "", "", 401}, {"GET", base + query, "", "key", 200}, {"GET", base + query + "&org=evil", "", "key", 400},
		{"PUT", base, body, "", 401}, {"PUT", base + query, body, "key", 400}, {"PUT", base, strings.TrimSuffix(body, "}") + `,"token":"bad"}`, "key", 400},
		{"PUT", base, body, "key", 200}, {"DELETE", base, identity, "key", 200}, {"POST", base, body, "key", 405},
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
}
