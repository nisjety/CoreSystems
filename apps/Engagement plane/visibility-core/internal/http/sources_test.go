package http

import (
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func TestSourceCatalogueIsReadOnlyScopedAndHonestAboutAvailability(t *testing.T) {
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
	handler := NewHandler("test-key", registrySource{registry: registry})
	base := "/api/v1/marketing/sources?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	for _, tc := range []struct {
		method, path, key string
		status            int
	}{
		{"GET", base, "", 401}, {"GET", base, "test-key", 200},
		{"GET", base + "&orgId=other", "test-key", 400},
		{"GET", base + "&site=https%3A%2F%2Fother.no%2F", "test-key", 400},
		{"GET", strings.Replace(base, "triodelab.no", "other.no", 1), "test-key", 409},
		{"POST", base, "test-key", 405},
	} {
		r := httptest.NewRequest(tc.method, tc.path, nil)
		r.Header.Set("X-Internal-API-Key", tc.key)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatalf("%s %s: %d wanted %d", tc.method, tc.path, w.Code, tc.status)
		}
	}
	r := httptest.NewRequest("GET", base, nil)
	r.Header.Set("X-Internal-API-Key", "test-key")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	var envelope struct {
		Data struct {
			WebsiteID string `json:"websiteId"`
			Items     []struct {
				Key         string `json:"key"`
				Implemented bool   `json:"implemented"`
				Status      string `json:"status"`
				Refresh     string `json:"refresh"`
			} `json:"items"`
		} `json:"data"`
	}
	if json.Unmarshal(w.Body.Bytes(), &envelope) != nil || envelope.Data.WebsiteID != site.ID || len(envelope.Data.Items) != 14 {
		t.Fatal("invalid source catalogue")
	}
	implemented := 0
	for _, item := range envelope.Data.Items {
		if item.Implemented {
			implemented++
		} else if item.Status != "not_implemented" || item.Refresh != "not_available" {
			t.Fatal("planned source claims collection")
		}
	}
	if implemented != 11 || strings.Contains(w.Body.String(), "ConnectionID") || strings.Contains(w.Body.String(), "authorizationUrl") {
		t.Fatal("catalogue fabricated support or leaked authority")
	}
}
