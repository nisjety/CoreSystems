package gdpr

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/rs/zerolog"
)

func envOf(m map[string]string) func(string) string {
	return func(k string) string { return m[k] }
}

// The hop is off unless QUARRY_EDGE_URL is set (WS17.E3).
func TestEdgePurgerFromEnv_UnsetIsOff(t *testing.T) {
	t.Parallel()
	if p := EdgePurgerFromEnv(envOf(map[string]string{}), zerolog.Nop()); p != nil {
		t.Fatalf("EdgePurgerFromEnv with QUARRY_EDGE_URL unset = %T, want nil (hop off)", p)
	}
}

// A set URL without a token must not silently disable the erasure: every call fails.
func TestEdgePurgerFromEnv_MissingTokenFailsEveryCall(t *testing.T) {
	t.Parallel()
	p := EdgePurgerFromEnv(envOf(map[string]string{"QUARRY_EDGE_URL": "http://quarry-edge:8080"}), zerolog.Nop())
	if p == nil {
		t.Fatal("EdgePurgerFromEnv returned nil with a URL set; the erasure would be skipped silently")
	}
	if _, err := p.PurgeEdgeOrg("org_a"); err == nil {
		t.Fatal("misconfigured edge purger returned nil error; the event would be acked")
	}
}

func TestEdgePurgerFromEnv_RejectsNonHTTPURL(t *testing.T) {
	t.Parallel()
	p := EdgePurgerFromEnv(envOf(map[string]string{
		"QUARRY_EDGE_URL":            "file:///etc/passwd",
		"QUARRY_EDGE_INTERNAL_TOKEN": "tok",
	}), zerolog.Nop())
	if _, err := p.PurgeEdgeOrg("org_a"); err == nil {
		t.Fatal("non-http(s) QUARRY_EDGE_URL accepted")
	}
}

// The request carries the shared token as a Bearer credential and the counts
// come back from the envelope's data field.
func TestHTTPEdgePurger_SendsBearerAndParsesCounts(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodDelete {
			t.Errorf("method=%s want DELETE", r.Method)
		}
		if r.URL.EscapedPath() != "/v1/internal/orgs/org_a/profiles" {
			t.Errorf("path=%s", r.URL.EscapedPath())
		}
		if got := r.Header.Get("Authorization"); got != "Bearer tok" {
			t.Errorf("authorization header=%q want Bearer tok", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"families":[{"family":"profiles","rows_deleted":2,"tombstones_deleted":1,"cache_keys_deleted":3,"keys_evicted":true}]}}`))
	}))
	defer srv.Close()

	p := NewHTTPEdgePurger(srv.URL+"/", "tok", nil)
	res, err := p.PurgeEdgeOrg("org_a")
	if err != nil {
		t.Fatalf("PurgeEdgeOrg: %v", err)
	}
	if len(res.Families) != 1 || res.Families[0].Family != "profiles" {
		t.Fatalf("families=%+v want one profiles entry", res.Families)
	}
	if res.Total() != 6 {
		t.Errorf("Total()=%d want 6", res.Total())
	}
}

// An org id is escaped as one path segment, so it cannot reach another route.
func TestHTTPEdgePurger_EscapesOrgID(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.EscapedPath() != "/v1/internal/orgs/a%2Fb/profiles" {
			t.Errorf("path=%s want the org id escaped", r.URL.EscapedPath())
		}
		_, _ = w.Write([]byte(`{"data":{"families":[]}}`))
	}))
	defer srv.Close()

	if _, err := NewHTTPEdgePurger(srv.URL, "tok", nil).PurgeEdgeOrg("a/b"); err != nil {
		t.Fatalf("PurgeEdgeOrg: %v", err)
	}
}

// A refusal (for example a wrong token) is an error, so the event is retried.
func TestHTTPEdgePurger_RefusalIsAnError(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer srv.Close()

	if _, err := NewHTTPEdgePurger(srv.URL, "wrong", nil).PurgeEdgeOrg("org_a"); err == nil {
		t.Fatal("401 from the edge returned nil error")
	}
}
