package http

import (
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type discoveryOwner struct {
	registrySource
	discovery *discovery.Service
}

func (s discoveryOwner) Discovery() *discovery.Service { return s.discovery }
func TestDiscoveryHTTPAuthorizesAndBindsExactRegisteredContext(t *testing.T) {
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
	owner := discoveryOwner{registrySource: registrySource{registry: registry}, discovery: discovery.New(registry, nil, nil)}
	handler := NewHandler("test-key", owner)
	call := func(method, path, body, key, media string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("X-Internal-API-Key", key)
		r.Header.Set("Content-Type", media)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	base := "/api/v1/marketing/discovery"
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := `{"websiteId":"` + site.ID + `","site":"https://triodelab.no/","websiteRevision":` + "2" + `}`
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{
		{"GET", base + query, "", "", "", 401},
		{"GET", base + query, "", "test-key", "", 200},
		{"GET", base + query + "&site=https%3A%2F%2Fother.no%2F", "", "test-key", "", 400},
		{"GET", base + query + "&token=secret", "", "test-key", "", 400},
		{"GET", base + "?websiteId=" + site.ID + "&site=https%3A%2F%2Fother.no%2F", "", "test-key", "", 409},
		{"POST", base, body, "test-key", "application/json", 200},
		{"POST", base, body, "test-key", "text/plain", 415},
		{"POST", base + query, body, "test-key", "application/json", 400},
		{"POST", base, strings.Replace(body, `"websiteRevision":2`, `"websiteRevision":999`, 1), "test-key", "application/json", 409},
		{"POST", base, strings.TrimSuffix(body, "}") + `,"url":"https://evil.test/"}`, "test-key", "application/json", 400},
		{"POST", base, strings.TrimSuffix(body, "}") + `,"websiteRevision":2}`, "test-key", "application/json", 400},
		{"POST", base, strings.TrimSuffix(body, "}") + `,"orgId":"other"}`, "test-key", "application/json", 400},
		{"DELETE", base, "", "test-key", "", 405},
	} {
		w := call(tc.method, tc.path, tc.body, tc.key, tc.media)
		if w.Code != tc.status {
			t.Fatalf("%s %s: %d expected%d %s", tc.method, tc.path, w.Code, tc.status, w.Body.String())
		}
	}
	w := call("GET", base+query, "", "test-key", "")
	if !strings.Contains(w.Body.String(), `"reason":"not_configured"`) || strings.Contains(w.Body.String(), `"discoveredAt"`) {
		t.Fatal("unconfigured fabricated discovery")
	}
}
