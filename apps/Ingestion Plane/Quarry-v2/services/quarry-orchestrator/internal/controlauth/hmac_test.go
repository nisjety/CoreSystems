package controlauth

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"net/http"
	"strings"
	"sync"
	"testing"
)

// Two distinct test vectors assembled at run time; not real secrets.
func sharedSecret() string  { return strings.Repeat("e0", 16) }
func controlSecret() string { return strings.Repeat("c1", 16) }

// withEnv replaces the env reader and the warning sink for one test and resets the
// once-guard, returning the warnings logged.
func withEnv(t *testing.T, env map[string]string) *[]string {
	t.Helper()
	oldGetenv, oldWarnf := getenv, warnf
	var warnings []string
	getenv = func(k string) string { return env[k] }
	warnf = func(msg string) { warnings = append(warnings, msg) }
	warnOnce = sync.Once{}
	t.Cleanup(func() {
		getenv, warnf = oldGetenv, oldWarnf
		warnOnce = sync.Once{}
	})
	return &warnings
}

// signedWith reports whether req's signature verifies under secret (the canonical
// string of quarry-control's httpx/hmac.go).
func signedWith(t *testing.T, req *http.Request, body []byte, secret string) bool {
	t.Helper()
	bodyHash := sha256.Sum256(body)
	path := req.URL.EscapedPath()
	if req.URL.RawQuery != "" {
		path += "?" + req.URL.RawQuery
	}
	canonical := req.Method + "\n" + path + "\n" + hex.EncodeToString(bodyHash[:]) + "\n" +
		req.Header.Get(headerTS) + "\n" + req.Header.Get(headerNonce)
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	want := "sig_v1=" + base64.StdEncoding.EncodeToString(mac.Sum(nil))
	return req.Header.Get(headerSig) == want
}

func newReq(t *testing.T, method, url string) *http.Request {
	t.Helper()
	req, err := http.NewRequest(method, url, nil)
	if err != nil {
		t.Fatal(err)
	}
	return req
}

func TestIsControlInternalPath(t *testing.T) {
	yes := []string{
		"/v1/internal/jobs/claim",
		"/v1/internal/jobs/pending-control",
		"/v1/internal/jobs/job_1/control-ack",
		"/v1/internal/jobs/running",
		"/v1/internal/jobs/job_1/finalize",
		"/v1/internal/schedules/desired",
		"/v1/internal/schedules/sch_1/runs",
	}
	no := []string{
		"/v1/internal/schedules/preflight", // the edge's route
		"/v1/internal/schedules//runs",
		"/v1/internal/schedules/sch_1",
		"/v1/internal/queues/q1/enqueue", // the edge's frontier
		"/v1/internal/run_page",
		"/v1/internal/change/record",
		"/v1/internal/zdr-tombstones", // the edge signs it, with the shared secret
		"/v1/jobs/job_1",
		"/v1/runs/run_1/events",
	}
	for _, p := range yes {
		if !IsControlInternalPath(p) {
			t.Errorf("%s should be a control-internal path", p)
		}
	}
	for _, p := range no {
		if IsControlInternalPath(p) {
			t.Errorf("%s should not be a control-internal path", p)
		}
	}
}

func TestSign_ControlInternalRouteUsesTheControlSecret(t *testing.T) {
	warnings := withEnv(t, map[string]string{ControlInternalSecretEnv: controlSecret()})
	req := newReq(t, "POST", "http://control:8081/v1/internal/jobs/job_1/finalize")
	if err := Sign(req, []byte(`{"status":"cancelled"}`), sharedSecret()); err != nil {
		t.Fatal(err)
	}
	if !signedWith(t, req, []byte(`{"status":"cancelled"}`), controlSecret()) {
		t.Fatal("finalize is not signed with the control-internal secret")
	}
	if signedWith(t, req, []byte(`{"status":"cancelled"}`), sharedSecret()) {
		t.Fatal("finalize must not verify under the shared (edge) secret")
	}
	if len(*warnings) != 0 {
		t.Fatalf("distinct secrets logged a warning: %v", *warnings)
	}
}

func TestSign_OtherControlRoutesKeepTheSharedSecret(t *testing.T) {
	withEnv(t, map[string]string{ControlInternalSecretEnv: controlSecret()})
	for _, url := range []string{
		"http://control:8081/v1/jobs/job_1",
		"http://control:8081/v1/runs/run_1/events",
		"http://edge:8082/v1/internal/schedules/preflight",
		"http://edge:8082/v1/internal/run_page",
	} {
		req := newReq(t, "POST", url)
		if err := Sign(req, nil, sharedSecret()); err != nil {
			t.Fatal(err)
		}
		if !signedWith(t, req, nil, sharedSecret()) {
			t.Errorf("%s should sign with the shared secret", url)
		}
	}
}

func TestSign_UnsetControlSecretFallsBackWithOneWarning(t *testing.T) {
	warnings := withEnv(t, map[string]string{})
	for i := 0; i < 3; i++ {
		req := newReq(t, "GET", "http://control:8081/v1/internal/jobs/running?limit=5")
		if err := Sign(req, nil, sharedSecret()); err != nil {
			t.Fatal(err)
		}
		if !signedWith(t, req, nil, sharedSecret()) {
			t.Fatal("with the control-internal secret unset the call must sign with the shared secret (no outage)")
		}
	}
	if len(*warnings) != 1 {
		t.Fatalf("fallback logged %d warnings, want exactly 1: %v", len(*warnings), *warnings)
	}
	if !strings.Contains((*warnings)[0], ControlInternalSecretEnv) {
		t.Fatalf("the warning must name %s: %q", ControlInternalSecretEnv, (*warnings)[0])
	}
}

func TestSign_EqualSecretsWarnOnce(t *testing.T) {
	warnings := withEnv(t, map[string]string{ControlInternalSecretEnv: sharedSecret()})
	for i := 0; i < 2; i++ {
		req := newReq(t, "POST", "http://control:8081/v1/internal/schedules/sch_1/runs")
		if err := Sign(req, []byte("{}"), sharedSecret()); err != nil {
			t.Fatal(err)
		}
	}
	if len(*warnings) != 1 {
		t.Fatalf("equal secrets logged %d warnings, want 1", len(*warnings))
	}
}

func TestSign_NoSecretAtAllLeavesTheRequestUnsigned(t *testing.T) {
	withEnv(t, map[string]string{})
	req := newReq(t, "GET", "http://control:8081/v1/internal/jobs/running")
	if err := Sign(req, nil, ""); err != nil {
		t.Fatal(err)
	}
	if req.Header.Get(headerSig) != "" {
		t.Fatal("a request with no secret at all must stay unsigned (dev posture)")
	}
}

func TestSign_ControlSecretAloneSignsWithoutTheSharedSecret(t *testing.T) {
	withEnv(t, map[string]string{ControlInternalSecretEnv: controlSecret()})
	req := newReq(t, "GET", "http://control:8081/v1/internal/jobs/running")
	if err := Sign(req, nil, ""); err != nil {
		t.Fatal(err)
	}
	if !signedWith(t, req, nil, controlSecret()) {
		t.Fatal("the control-internal secret signs even when the shared secret is empty")
	}
}
