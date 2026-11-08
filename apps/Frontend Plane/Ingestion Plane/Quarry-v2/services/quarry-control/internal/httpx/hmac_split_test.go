package httpx

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func okHandler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(http.StatusNoContent) })
}

func splitStatus(t *testing.T, h http.Handler, secret, method, pathQ string, nonce string) int {
	t.Helper()
	req := httptest.NewRequest(method, "http://localhost"+pathQ, nil)
	for k, vs := range signRequest(t, secret, method, pathQ, nil, time.Now().Unix(), nonce) {
		req.Header.Set(k, vs[0])
	}
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec.Code
}

func TestIsOrchestratorOnlyPath(t *testing.T) {
	yes := []string{
		"/v1/internal/jobs/claim",
		"/v1/internal/jobs/job_1/finalize",
		"/v1/internal/schedules/desired",
		"/v1/internal/schedules/sch_1/runs",
	}
	no := []string{
		"/v1/internal/zdr-tombstones",
		"/v1/jobs/job_1",
		"/v1/schedules",
		"/v1/internal/run_page",
		"/v1/internalx/jobs/claim",
		"/health",
	}
	for _, p := range yes {
		if !IsOrchestratorOnlyPath(p) {
			t.Errorf("%s should be orchestrator-only", p)
		}
	}
	for _, p := range no {
		if IsOrchestratorOnlyPath(p) {
			t.Errorf("%s should not be orchestrator-only", p)
		}
	}
}

func TestSplitHMAC_RoutesEachGroupToItsOwnSecret(t *testing.T) {
	edge, orch := testSecret(), otherSecret()
	h := SplitHMAC(NewHMACVerifier(orch, true), NewHMACVerifier(edge, true))(okHandler())

	cases := []struct {
		name, secret, path string
		want               int
	}{
		{"edge secret on an edge route", edge, "/v1/jobs/ongoing?org_id=org_a", http.StatusNoContent},
		{"control secret on an edge route", orch, "/v1/jobs/ongoing?org_id=org_a", http.StatusUnauthorized},
		{"control secret on an orchestrator route", orch, "/v1/internal/jobs/running?limit=5", http.StatusNoContent},
		{"edge secret on an orchestrator route", edge, "/v1/internal/jobs/running?limit=5", http.StatusUnauthorized},
		{"edge secret on schedules/desired", edge, "/v1/internal/schedules/desired", http.StatusUnauthorized},
		{"edge secret on the tombstone route", edge, "/v1/internal/zdr-tombstones", http.StatusNoContent},
		{"control secret on the tombstone route", orch, "/v1/internal/zdr-tombstones", http.StatusUnauthorized},
	}
	for i, c := range cases {
		nonce := "7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e" + string(rune('a'+i)) + "0"
		if got := splitStatus(t, h, c.secret, "GET", c.path, nonce); got != c.want {
			t.Errorf("%s: status %d, want %d", c.name, got, c.want)
		}
	}
}

func TestSplitHMAC_UnsignedRefusedWhenRequired(t *testing.T) {
	h := SplitHMAC(NewHMACVerifier(otherSecret(), true), NewHMACVerifier(testSecret(), true))(okHandler())
	for _, p := range []string{"/v1/internal/jobs/running", "/v1/jobs/ongoing"} {
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, httptest.NewRequest("GET", "http://localhost"+p, nil))
		if rec.Code != http.StatusUnauthorized {
			t.Errorf("unsigned %s: status %d, want 401", p, rec.Code)
		}
	}
}

func TestSplitHMAC_SameVerifierIsTheOldSharedSecretBehaviour(t *testing.T) {
	v := NewHMACVerifier(testSecret(), true)
	h := SplitHMAC(v, v)(okHandler())
	for i, p := range []string{"/v1/internal/jobs/running", "/v1/jobs/ongoing"} {
		nonce := "3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c" + string(rune('a'+i)) + "0"
		if got := splitStatus(t, h, testSecret(), "GET", p, nonce); got != http.StatusNoContent {
			t.Errorf("shared secret on %s: status %d, want 204", p, got)
		}
	}
}

func TestControlInternalSecretPosture(t *testing.T) {
	cases := []struct {
		name, control, edge, wantSecret, wantWarning string
	}{
		{"unset falls back with a warning", "", "edge-secret", "edge-secret", "unset"},
		{"equal is flagged", "same-secret", "same-secret", "same-secret", "same"},
		{"distinct is quiet", "control-secret", "edge-secret", "control-secret", ""},
	}
	for _, c := range cases {
		secret, warning := ControlInternalSecretPosture(c.control, c.edge)
		if secret != c.wantSecret || warning != c.wantWarning {
			t.Errorf("%s: got (%q, %q), want (%q, %q)", c.name, secret, warning, c.wantSecret, c.wantWarning)
		}
	}
}
