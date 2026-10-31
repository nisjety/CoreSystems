package healthattest

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const (
	testCredential = "test-only-browser-broker-key"
	testToken      = "test-only-minted-token"
)

// fakeAuthCore serves Auth Core's internal-token mint for capability-core.
type fakeAuthCore struct {
	server *httptest.Server
	status int
	mints  atomic.Int32

	mu      sync.Mutex
	request map[string]any
	headers http.Header
}

func newFakeAuthCore(t *testing.T) *fakeAuthCore {
	t.Helper()
	fake := &fakeAuthCore{status: http.StatusOK}
	fake.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/api/capability-core/internal-token" {
			http.NotFound(w, r)
			return
		}
		fake.mints.Add(1)
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		fake.mu.Lock()
		fake.request, fake.headers = body, r.Header.Clone()
		fake.mu.Unlock()
		if fake.status != http.StatusOK {
			w.WriteHeader(fake.status)
			_, _ = w.Write([]byte(`{"message":"principal not authorized"}`))
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token": testToken, "expiresInSeconds": 300, "audience": "capability-core",
		})
	}))
	t.Cleanup(fake.server.Close)
	return fake
}

// fakeCapabilityCore serves the row read and the browser health lane.
type fakeCapabilityCore struct {
	server     *httptest.Server
	rowStatus  int
	rowVersion string
	// rowRedirect, when set, answers the row read with a redirect there.
	rowRedirect string
	postStatus  []int // consumed in order; the last one repeats

	mu     sync.Mutex
	posts  []map[string]any
	auths  []string
	reads  int
	extras []string
}

func newFakeCapabilityCore(t *testing.T) *fakeCapabilityCore {
	t.Helper()
	fake := &fakeCapabilityCore{rowStatus: http.StatusOK, rowVersion: "7", postStatus: []int{http.StatusOK}}
	fake.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fake.mu.Lock()
		defer fake.mu.Unlock()
		fake.auths = append(fake.auths, r.Header.Get("Authorization"))
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/capabilities/cap.browser.open":
			fake.reads++
			if fake.rowRedirect != "" {
				http.Redirect(w, r, fake.rowRedirect, http.StatusTemporaryRedirect)
				return
			}
			if fake.rowStatus != http.StatusOK {
				w.WriteHeader(fake.rowStatus)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"id": "cap.browser.open", "version": fake.rowVersion, "risk_level": "high"})
		case r.Method == http.MethodPost && r.URL.Path == "/api/v1/capabilities/browser/health":
			raw, _ := io.ReadAll(r.Body)
			var body map[string]any
			if err := json.Unmarshal(raw, &body); err != nil {
				t.Errorf("attestation body is not JSON: %v", err)
			}
			fake.posts = append(fake.posts, body)
			status := fake.postStatus[0]
			if len(fake.postStatus) > 1 {
				fake.postStatus = fake.postStatus[1:]
			}
			w.WriteHeader(status)
		default:
			fake.extras = append(fake.extras, r.Method+" "+r.URL.Path)
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(fake.server.Close)
	return fake
}

func (fake *fakeCapabilityCore) postCount() int {
	fake.mu.Lock()
	defer fake.mu.Unlock()
	return len(fake.posts)
}

func (fake *fakeCapabilityCore) lastPost(t *testing.T) map[string]any {
	t.Helper()
	fake.mu.Lock()
	defer fake.mu.Unlock()
	if len(fake.posts) == 0 {
		t.Fatal("nothing was posted")
	}
	return fake.posts[len(fake.posts)-1]
}

func newQuarry(t *testing.T, status int) *httptest.Server {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ready" {
			http.NotFound(w, r)
			return
		}
		w.WriteHeader(status)
	}))
	t.Cleanup(server.Close)
	return server
}

func newAttester(t *testing.T, auth *fakeAuthCore, capability *fakeCapabilityCore, quarryReadyURL string, ready func(context.Context) error) (*Attester, *bytes.Buffer) {
	t.Helper()
	logs := &bytes.Buffer{}
	attester, err := New(Config{
		CapabilityCoreURL: capability.server.URL,
		AuthCoreURL:       auth.server.URL,
		ServiceAPIKey:     testCredential,
		QuarryReadyURL:    quarryReadyURL,
		Interval:          time.Minute,
	}, ready, slog.New(slog.NewTextHandler(logs, nil)))
	if err != nil {
		t.Fatal(err)
	}
	return attester, logs
}

func assertNoSecretsLogged(t *testing.T, logs *bytes.Buffer, urls ...string) {
	t.Helper()
	text := logs.String()
	for _, secret := range append([]string{testCredential, testToken}, urls...) {
		if secret != "" && strings.Contains(text, secret) {
			t.Fatalf("log carries %q:\n%s", secret, text)
		}
	}
}

func TestHealthyBrokerAttestsAgenticAvailability(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	quarry := newQuarry(t, http.StatusOK)
	attester, logs := newAttester(t, auth, capability, quarry.URL+"/ready", func(context.Context) error { return nil })

	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	want := map[string]any{
		"id": "cap.browser.open", "version": "7", "state": "available", "reason_code": "browser_broker_ready",
		"reason": "", "execution_mode": "agentic", "cost_class": "variable",
	}
	got := capability.lastPost(t)
	if len(got) != len(want) {
		t.Fatalf("posted keys = %v, want exactly %v", got, want)
	}
	for key, value := range want {
		if got[key] != value {
			t.Fatalf("posted %s = %v, want %v (body %v)", key, got[key], value, got)
		}
	}
	for _, header := range capability.auths {
		if header != "Bearer "+testToken {
			t.Fatalf("capability-core saw Authorization %q", header)
		}
	}

	auth.mu.Lock()
	defer auth.mu.Unlock()
	if auth.headers.Get("x-service-id") != "browser-broker" || auth.headers.Get("x-service-api-key") != testCredential {
		t.Fatalf("mint headers = %v", auth.headers)
	}
	if auth.request["orgId"] != "global" {
		t.Fatalf("mint org = %v", auth.request["orgId"])
	}
	scopes, _ := auth.request["scopes"].([]any)
	asked := map[any]bool{}
	for _, scope := range scopes {
		asked[scope] = true
	}
	if len(scopes) != 2 || !asked["capability:browser:health:write"] || !asked["capability:read"] {
		t.Fatalf("mint scopes = %v", auth.request["scopes"])
	}
	if !strings.Contains(logs.String(), "cap.browser.open health attested") {
		t.Fatalf("no success line:\n%s", logs.String())
	}
	assertNoSecretsLogged(t, logs, capability.server.URL, quarry.URL)
}

func TestNilReadyAndNoQuarryProbeCountAsReady(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	attester, _ := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	if got := capability.lastPost(t); got["state"] != "available" || got["execution_mode"] != "agentic" {
		t.Fatalf("posted %v", got)
	}
}

func TestFailedGrantStorePingAttestsUnavailableExecution(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	quarry := newQuarry(t, http.StatusOK)
	attester, _ := newAttester(t, auth, capability, quarry.URL+"/ready", func(context.Context) error {
		return errors.New("database is down")
	})
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	got := capability.lastPost(t)
	if got["state"] != "unhealthy" || got["reason_code"] != "grant_store_unavailable" || got["execution_mode"] != "unavailable" {
		t.Fatalf("posted %v", got)
	}
}

func TestGRPCNotServingAttestsUnavailableExecutionBeforeAnyOtherProbe(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	quarry := newQuarry(t, http.StatusOK)
	pinged := false
	attester, _ := newAttester(t, auth, capability, quarry.URL+"/ready", func(context.Context) error {
		pinged = true
		return nil
	})
	serving := false
	attester.serving = func() bool { return serving }
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	got := capability.lastPost(t)
	if got["state"] != "unhealthy" || got["reason_code"] != "browser_broker_not_serving" || got["execution_mode"] != "unavailable" {
		t.Fatalf("posted %v", got)
	}
	if pinged {
		t.Fatal("the grant store was probed although the gRPC server is not serving")
	}
	serving = true
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	if got := capability.lastPost(t); got["state"] != "available" || got["reason_code"] != "browser_broker_ready" {
		t.Fatalf("posted %v once serving", got)
	}
}

func TestQuarryNotReadyAttestsUnavailableExecution(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	quarry := newQuarry(t, http.StatusServiceUnavailable)
	attester, _ := newAttester(t, auth, capability, quarry.URL+"/ready", func(context.Context) error { return nil })
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	got := capability.lastPost(t)
	if got["state"] != "unhealthy" || got["reason_code"] != "quarry_edge_not_ready" || got["execution_mode"] != "unavailable" ||
		got["cost_class"] != "variable" || got["version"] != "7" {
		t.Fatalf("posted %v", got)
	}
}

func TestUnreachableQuarryAttestsUnavailableExecution(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	quarry := newQuarry(t, http.StatusOK)
	readyURL := quarry.URL + "/ready"
	quarry.Close()
	attester, _ := newAttester(t, auth, capability, readyURL, nil)
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	if got := capability.lastPost(t); got["reason_code"] != "quarry_edge_not_ready" {
		t.Fatalf("posted %v", got)
	}
}

func TestFailedMintPostsNothing(t *testing.T) {
	auth := newFakeAuthCore(t)
	auth.status = http.StatusForbidden
	capability := newFakeCapabilityCore(t)
	attester, logs := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); !errors.Is(err, ErrTokenMint) {
		t.Fatalf("Attest() = %v, want ErrTokenMint", err)
	}
	capability.mu.Lock()
	defer capability.mu.Unlock()
	if len(capability.posts) != 0 || capability.reads != 0 {
		t.Fatalf("capability-core was called without a token: reads=%d posts=%d", capability.reads, len(capability.posts))
	}
	if strings.Contains(logs.String(), "principal not authorized") {
		t.Fatalf("Auth Core's response body reached the log:\n%s", logs.String())
	}
	assertNoSecretsLogged(t, logs, auth.server.URL)
}

func TestFailedRowReadPostsNothing(t *testing.T) {
	for _, status := range []int{http.StatusNotFound, http.StatusForbidden, http.StatusInternalServerError} {
		auth := newFakeAuthCore(t)
		capability := newFakeCapabilityCore(t)
		capability.rowStatus = status
		attester, _ := newAttester(t, auth, capability, "", nil)
		if err := attester.Attest(context.Background()); !errors.Is(err, ErrRowRead) {
			t.Fatalf("status %d: Attest() = %v, want ErrRowRead", status, err)
		}
		if capability.postCount() != 0 {
			t.Fatalf("status %d: an attestation was posted without a row version", status)
		}
	}
}

func TestRowWithoutVersionPostsNothing(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	capability.rowVersion = " "
	attester, _ := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); !errors.Is(err, ErrRowRead) {
		t.Fatalf("Attest() = %v, want ErrRowRead", err)
	}
	if capability.postCount() != 0 {
		t.Fatal("an attestation was posted without a row version")
	}
}

func TestCapabilityCoreRedirectIsNotFollowed(t *testing.T) {
	var followed atomic.Int32
	elsewhere := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		followed.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{"version": "9"})
	}))
	t.Cleanup(elsewhere.Close)

	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	capability.rowRedirect = elsewhere.URL + "/api/v1/capabilities/cap.browser.open"
	attester, _ := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); !errors.Is(err, ErrRowRead) {
		t.Fatalf("Attest() = %v, want ErrRowRead", err)
	}
	if followed.Load() != 0 {
		t.Fatal("the redirect was followed with the bearer token")
	}
	if capability.postCount() != 0 {
		t.Fatal("an attestation was posted after a redirected row read")
	}
}

func TestQuarryRedirectIsNotFollowedAndIsNotReady(t *testing.T) {
	var followed atomic.Int32
	elsewhere := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		followed.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	t.Cleanup(elsewhere.Close)
	quarry := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, elsewhere.URL+"/ready", http.StatusFound)
	}))
	t.Cleanup(quarry.Close)

	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	attester, _ := newAttester(t, auth, capability, quarry.URL+"/ready", nil)
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	if followed.Load() != 0 {
		t.Fatal("the quarry readiness redirect was followed")
	}
	if got := capability.lastPost(t); got["reason_code"] != "quarry_edge_not_ready" {
		t.Fatalf("posted %v", got)
	}
}

func TestUnauthorizedAttestationRemintsOnceAndRetries(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	capability.postStatus = []int{http.StatusUnauthorized, http.StatusOK}
	attester, _ := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() = %v", err)
	}
	if auth.mints.Load() != 2 || capability.postCount() != 2 {
		t.Fatalf("mints = %d, posts = %d; want one re-mint and one retry", auth.mints.Load(), capability.postCount())
	}

	// A second 401 is not retried again: the next tick will try afresh.
	capability.postStatus = []int{http.StatusUnauthorized}
	if err := attester.Attest(context.Background()); !errors.Is(err, ErrPost) {
		t.Fatalf("Attest() = %v, want ErrPost", err)
	}
	if capability.postCount() != 4 {
		t.Fatalf("posts = %d, want 4", capability.postCount())
	}
}

func TestRefusedAttestationIsReportedWithoutTheBody(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	capability.postStatus = []int{http.StatusForbidden}
	attester, logs := newAttester(t, auth, capability, "", nil)
	if err := attester.Attest(context.Background()); !errors.Is(err, ErrPost) {
		t.Fatalf("Attest() = %v, want ErrPost", err)
	}
	if !strings.Contains(logs.String(), "status=403") {
		t.Fatalf("refusal status missing from the log:\n%s", logs.String())
	}
	assertNoSecretsLogged(t, logs, capability.server.URL)
}

func TestRunAttestsImmediatelyAndStopsWithItsContext(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	attester, _ := newAttester(t, auth, capability, "", nil)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		attester.Run(ctx)
		close(done)
	}()
	deadline := time.Now().Add(5 * time.Second)
	for capability.postCount() == 0 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if capability.postCount() == 0 {
		t.Fatal("Run did not attest at start")
	}
	cancel()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("Run did not stop with its context")
	}
}

func env(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func TestConfigOffWhenNothingIsSet(t *testing.T) {
	_, enabled, err := ConfigFromEnv(env(map[string]string{EnvQuarryReadyURL: "http://quarry-edge:8082/ready"}))
	if enabled || err != nil {
		t.Fatalf("enabled=%v err=%v; want off without an error", enabled, err)
	}
}

func TestPartialConfigDisablesTheAttesterAndNamesOnlyTheVariables(t *testing.T) {
	values := map[string]string{
		EnvCapabilityCoreURL: "http://capability-core:8085",
		EnvServiceAPIKey:     testCredential,
	}
	_, enabled, err := ConfigFromEnv(env(values))
	if enabled || err == nil {
		t.Fatalf("enabled=%v err=%v; want disabled with an error", enabled, err)
	}
	if !strings.Contains(err.Error(), EnvAuthCoreURL) || strings.Contains(err.Error(), EnvServiceAPIKey) {
		t.Fatalf("error = %q; want only the missing variable named", err)
	}
	for _, value := range values {
		if strings.Contains(err.Error(), value) {
			t.Fatalf("error = %q carries a value", err)
		}
	}

	logs := &bytes.Buffer{}
	if Start(context.Background(), env(values), nil, nil, slog.New(slog.NewTextHandler(logs, nil))) {
		t.Fatal("Start started an attester on a partial configuration")
	}
	if !strings.Contains(logs.String(), "level=WARN") || strings.Contains(logs.String(), testCredential) {
		t.Fatalf("partial configuration log:\n%s", logs.String())
	}
}

func TestNonHTTPURLDisablesTheAttester(t *testing.T) {
	_, enabled, err := ConfigFromEnv(env(map[string]string{
		EnvCapabilityCoreURL: "capability-core:8085",
		EnvAuthCoreURL:       "http://auth-core:3011",
		EnvServiceAPIKey:     testCredential,
	}))
	if enabled || err == nil || !strings.Contains(err.Error(), EnvCapabilityCoreURL) {
		t.Fatalf("enabled=%v err=%v", enabled, err)
	}
}

func TestCompleteConfigStartsTheAttester(t *testing.T) {
	auth := newFakeAuthCore(t)
	capability := newFakeCapabilityCore(t)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := Start(ctx, env(map[string]string{
		EnvCapabilityCoreURL: capability.server.URL + "/",
		EnvAuthCoreURL:       auth.server.URL,
		EnvServiceAPIKey:     testCredential,
	}), nil, func() bool { return true }, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if !started {
		t.Fatal("Start did not start on a complete configuration")
	}
	deadline := time.Now().Add(5 * time.Second)
	for capability.postCount() == 0 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if capability.postCount() == 0 {
		t.Fatal("the started attester did not attest")
	}
}

func TestIntervalIsClampedToTheFreshnessWindow(t *testing.T) {
	for raw, want := range map[string]time.Duration{
		"":     DefaultInterval,
		"abc":  DefaultInterval,
		"1.5":  DefaultInterval,
		"0":    MinInterval,
		"-30":  MinInterval,
		"5":    MinInterval,
		"15":   15 * time.Second,
		"30":   30 * time.Second,
		" 90 ": 90 * time.Second,
		"150":  MaxInterval,
		"1000": MaxInterval,
	} {
		if got := IntervalFromEnv(raw); got != want {
			t.Fatalf("IntervalFromEnv(%q) = %v, want %v", raw, got, want)
		}
	}
	cfg, _, _ := ConfigFromEnv(env(map[string]string{EnvInterval: "3600"}))
	if cfg.Interval != MaxInterval {
		t.Fatalf("ConfigFromEnv interval = %v, want %v", cfg.Interval, MaxInterval)
	}
}
