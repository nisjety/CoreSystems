package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
)

func newBlocklistsGoneServer(t *testing.T) http.Handler {
	t.Helper()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountBlocklistsGone(r)
	return r
}

// WS7.F gate: GET /v1/blocklists answers 410 with the typed envelope, for every
// method and for the item path too.
func TestBlocklistsReturnsGone(t *testing.T) {
	t.Parallel()
	h := newBlocklistsGoneServer(t)
	cases := []struct{ method, path string }{
		{http.MethodGet, "/v1/blocklists"},
		{http.MethodGet, "/v1/blocklists/"},
		{http.MethodPost, "/v1/blocklists"},
		{http.MethodGet, "/v1/blocklists/bl_1"},
		{http.MethodDelete, "/v1/blocklists/bl_1"},
	}
	for _, c := range cases {
		req := httptest.NewRequest(c.method, c.path, strings.NewReader("{}"))
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusGone {
			t.Fatalf("%s %s: status = %d, want 410; body=%s", c.method, c.path, w.Code, w.Body.String())
		}
		var env map[string]any
		if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
			t.Fatalf("%s %s: body is not JSON: %v", c.method, c.path, err)
		}
		errObj, _ := env["error"].(map[string]any)
		if errObj == nil {
			t.Fatalf("%s %s: no error envelope: %s", c.method, c.path, w.Body.String())
		}
		if errObj["code"] != "GONE" {
			t.Errorf("%s %s: error.code = %v, want GONE", c.method, c.path, errObj["code"])
		}
		if errObj["retryable"] != false {
			t.Errorf("%s %s: retryable = %v, want false", c.method, c.path, errObj["retryable"])
		}
		details, _ := errObj["details"].(map[string]any)
		if details["reference"] != "docs/POLICY.md" {
			t.Errorf("%s %s: details.reference = %v, want docs/POLICY.md", c.method, c.path, details["reference"])
		}
	}
}
