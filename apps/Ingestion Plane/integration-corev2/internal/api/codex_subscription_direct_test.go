package api

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
)

const directConnectionID = "conn-direct"

// directSSE encodes one upstream server-sent event.
func directSSE(value any) string {
	encoded, _ := json.Marshal(value)
	return "data: " + string(encoded) + "\n\n"
}

// goldenUpstreamFrames is a Responses stream answering "Hello" in two deltas.
func goldenUpstreamFrames() []string {
	return []string{
		directSSE(map[string]any{"type": "response.created", "response": map[string]any{"id": "resp_1", "model": "gpt-5.6-terra"}}),
		directSSE(map[string]any{"type": "response.output_text.delta", "delta": "Hel"}),
		directSSE(map[string]any{"type": "response.output_text.delta", "delta": "lo"}),
		directSSE(map[string]any{"type": "response.completed", "response": map[string]any{
			"id": "resp_1", "model": "gpt-5.6-terra",
			"usage": map[string]any{"input_tokens": 1200, "input_tokens_details": map[string]any{"cached_tokens": 1024}, "output_tokens": 42, "output_tokens_details": map[string]any{"reasoning_tokens": 30}},
		}}),
	}
}

func serveFrames(frames []string) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		for _, frame := range frames {
			_, _ = io.WriteString(w, frame)
			if flusher, ok := w.(http.Flusher); ok {
				flusher.Flush()
			}
		}
	}
}

type steppedClock struct {
	mu      sync.Mutex
	offsets []time.Duration
	calls   int
}

func (c *steppedClock) now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	offset := c.offsets[min(c.calls, len(c.offsets)-1)]
	c.calls++
	return time.Unix(1_800_000_000, 0).Add(offset)
}

type directStackOptions struct {
	tokenHandler      http.HandlerFunc
	now               func() time.Time
	invocationTimeout time.Duration
	// logs, when set, receives the broker's content-free log lines.
	logs io.Writer
}

// directStack is the full broker stack: the fiber app, the Manager, and the
// real DirectTransport against a fake ChatGPT upstream.
func directStack(t *testing.T, upstream http.HandlerFunc, options directStackOptions) (*fiber.App, *store.MemoryRepository) {
	t.Helper()
	cfg, repo, service := testOAuthStack(t)
	fakeUpstream := httptest.NewTLSServer(upstream)
	t.Cleanup(fakeUpstream.Close)
	direct := codexsubscription.DirectConfig{
		BaseURL:    fakeUpstream.URL + "/backend-api/codex",
		HTTPClient: fakeUpstream.Client(),
		Now:        options.now,
		MaxRetries: codexsubscription.DefaultDirectMaxRetries,
	}
	if options.tokenHandler != nil {
		tokens := httptest.NewServer(options.tokenHandler)
		t.Cleanup(tokens.Close)
		direct.TokenURL = tokens.URL + "/oauth/token"
	}
	runnerOptions := codexsubscription.RunnerOptions{Transport: codexsubscription.TransportDirect, Direct: direct}
	if options.logs != nil {
		runnerOptions.Logger = zerolog.New(options.logs)
	}
	runner, err := codexsubscription.NewRunner(runnerOptions)
	if err != nil {
		t.Fatalf("NewRunner: %v", err)
	}
	home := t.TempDir()
	manager, err := codexsubscription.NewManager(codexsubscription.Config{Enabled: true, Home: home, InvocationTimeout: options.invocationTimeout}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	writeDirectAuth(t, filepath.Join(home, directConnectionID))
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:             directConnectionID,
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	}); err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	return NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager}), repo
}

// writeDirectAuth writes a Codex auth.json whose access token is valid for
// an hour. The token values are test fixtures, not credentials.
func writeDirectAuth(t *testing.T, dir string) {
	t.Helper()
	payload, _ := json.Marshal(map[string]any{"exp": time.Now().Add(time.Hour).Unix()})
	header := base64.RawURLEncoding.EncodeToString([]byte("{\"alg\":\"HS256\",\"typ\":\"JWT\"}"))
	access := header + "." + base64.RawURLEncoding.EncodeToString(payload) + ".fixture"
	idToken := header + "." + base64.RawURLEncoding.EncodeToString([]byte("{}")) + ".fixture"
	// The shape internal/codexsubscription reads (see its testhelpers_test.go).
	auth, err := json.Marshal(map[string]any{
		"auth_mode":      "chatgpt",
		"OPENAI_API_KEY": nil,
		"tokens": map[string]any{
			"id_token":      idToken,
			"access_token":  access,
			"refresh_token": "fixture-refresh",
			"account_id":    "acct-fixture",
		},
		"last_refresh": time.Now().UTC().Format(time.RFC3339),
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "auth.json"), []byte(auth), 0o600); err != nil {
		t.Fatal(err)
	}
}

const directInferBody = `{"organizationId":"org-1","userId":"user-1","connectionId":"conn-direct","requestId":"req_golden","model":"gpt-5.6-terra","messages":[{"role":"user","content":"hello"}]}`

func postInfer(t *testing.T, app *fiber.App, path, body string, timeout int) *http.Response {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
	req.Header.Set("X-Request-ID", "req_golden")
	resp, err := app.Test(req, timeout)
	if err != nil {
		t.Fatalf("app.Test %s: %v", path, err)
	}
	t.Cleanup(func() { _ = resp.Body.Close() })
	return resp
}

func readBody(t *testing.T, resp *http.Response) string {
	t.Helper()
	payload, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatal(err)
	}
	return string(payload)
}

func compareGolden(t *testing.T, name, got string) {
	t.Helper()
	path := filepath.Join("testdata", name)
	if os.Getenv("UPDATE_GOLDEN") == "1" {
		if err := os.MkdirAll("testdata", 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(got), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read golden %s: %v", path, err)
	}
	if got != strings.ReplaceAll(string(want), "\r\n", "\n") {
		t.Fatalf("%s differs.\ngot:\n%s\nwant:\n%s", name, got, want)
	}
}

func TestGoldenStreamEventsDirect(t *testing.T) {
	clock := &steppedClock{offsets: []time.Duration{0, 10 * time.Millisecond, 30 * time.Millisecond, 50 * time.Millisecond}}
	app, _ := directStack(t, serveFrames(goldenUpstreamFrames()), directStackOptions{now: clock.now})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer/stream", directInferBody, -1)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	compareGolden(t, "codex_stream_direct.golden", readBody(t, resp))
}

func TestGoldenUnaryDirect(t *testing.T) {
	clock := &steppedClock{offsets: []time.Duration{0, 10 * time.Millisecond, 30 * time.Millisecond, 50 * time.Millisecond}}
	app, _ := directStack(t, serveFrames(goldenUpstreamFrames()), directStackOptions{now: clock.now})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	compareGolden(t, "codex_unary_direct.golden.json", readBody(t, resp)+"\n")
}

type apiErrorBody struct {
	Success bool `json:"success"`
	Error   struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

func decodeAPIError(t *testing.T, body string) apiErrorBody {
	t.Helper()
	var decoded apiErrorBody
	if err := json.Unmarshal([]byte(body), &decoded); err != nil {
		t.Fatalf("decode %q: %v", body, err)
	}
	return decoded
}

// auditEvents drains the memory repository's audit rows.
func auditEvents(t *testing.T, repo *store.MemoryRepository) []store.AuditEvent {
	t.Helper()
	var events []store.AuditEvent
	for {
		event, ok, err := repo.ClaimAuditEvent(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if !ok {
			return events
		}
		events = append(events, event)
	}
}

func fakeRunnerStack(t *testing.T, runner codexsubscription.Runner, cfgOverride func(*codexsubscription.Config)) (*fiber.App, *store.MemoryRepository) {
	t.Helper()
	cfg, repo, service := testOAuthStack(t)
	managerConfig := codexsubscription.Config{Enabled: true, Home: t.TempDir()}
	if cfgOverride != nil {
		cfgOverride(&managerConfig)
	}
	manager, err := codexsubscription.NewManager(managerConfig, runner)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: directConnectionID, ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{codexsubscription.Capability},
	}); err != nil {
		t.Fatal(err)
	}
	return NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager}), repo
}

func TestUnaryUsageLimit429(t *testing.T) {
	resetsAt := time.Now().Add(3725 * time.Second).UTC().Truncate(time.Second)
	runner := &apiCodexRunner{invokeErr: &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, RetryAfter: 3725 * time.Second, ResetsAt: resetsAt, WindowMinutes: 300, LimitReachedType: "rate_limit_reached"}}
	app, repo := fakeRunnerStack(t, runner, nil)
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	retryAfter, err := strconv.Atoi(resp.Header.Get("Retry-After"))
	if resp.StatusCode != http.StatusTooManyRequests || err != nil || retryAfter < 3724 || retryAfter > 3725 {
		t.Fatalf("status = %d Retry-After = %q, want 429 and the seconds until the reset", resp.StatusCode, resp.Header.Get("Retry-After"))
	}
	decoded := decodeFailureBody(t, body)
	if decoded.Success || decoded.Error.Code != "subscription_usage_limit" || decoded.Error.Message != "The connected ChatGPT plan has reached its Codex usage limit." {
		t.Fatalf("body = %s", body)
	}
	if decoded.Error.ResetsAt != resetsAt.Format(time.RFC3339) || decoded.Error.WindowMinutes != 300 || decoded.Error.LimitReachedType != "rate_limit_reached" {
		t.Fatalf("body = %s, want resetsAt %s, windowMinutes 300 and the limit type", body, resetsAt.Format(time.RFC3339))
	}
	if ms := decoded.Error.RetryAfterMs; ms <= 3723_000 || ms > 3725_000 {
		t.Fatalf("retryAfterMs = %d, want the time until the reset", ms)
	}
	if strings.Contains(body, "plan_type") || strings.Contains(body, "planType") {
		t.Fatalf("body leaks the plan type: %s", body)
	}
	for _, event := range auditEvents(t, repo) {
		if event.EventType == "subscription_inference.completed" {
			t.Fatal("a rate-limited call was audited as completed")
		}
	}
}

func TestUnaryUsageLimitFromUpstreamBody(t *testing.T) {
	resetsAt := time.Now().Add(120 * time.Second).Unix()
	app, _ := directStack(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusTooManyRequests)
		_, _ = io.WriteString(w, `{"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"plus","resets_at":`+strconv.FormatInt(resetsAt, 10)+`}}`)
	}, directStackOptions{})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	if resp.StatusCode != http.StatusTooManyRequests {
		t.Fatalf("status = %d body = %s", resp.StatusCode, body)
	}
	retryAfter, err := strconv.Atoi(resp.Header.Get("Retry-After"))
	if err != nil || retryAfter < 119 || retryAfter > 121 {
		t.Fatalf("Retry-After = %q, want 120±1", resp.Header.Get("Retry-After"))
	}
	if strings.Contains(body, "plus") || strings.Contains(body, "usage limit has been reached") {
		t.Fatalf("body leaks upstream details: %s", body)
	}
	decoded := decodeFailureBody(t, body)
	if decoded.Error.Code != "subscription_usage_limit" || decoded.Error.ResetsAt != time.Unix(resetsAt, 0).UTC().Format(time.RFC3339) {
		t.Fatalf("body = %s, want subscription_usage_limit resetting at the upstream's resets_at", body)
	}
}

type rateLimitedStreamRunner struct{ apiCodexRunner }

func (r *rateLimitedStreamRunner) InvokeStream(context.Context, string, codexsubscription.InvokeRequest, func(string) error) (codexsubscription.InvokeResponse, error) {
	return codexsubscription.InvokeResponse{}, &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitThrottle, RetryAfter: 2 * time.Second}
}

// A rate limit raised after the 200 (an upstream usage limit mid-turn) can no
// longer be a 429, but it is still labelled as one, with nothing upstream.
func TestStreamRateLimitAfterReadyIsRateLimitedEvent(t *testing.T) {
	app, _ := fakeRunnerStack(t, &rateLimitedStreamRunner{}, nil)
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer/stream", directInferBody, -1)
	stream := readBody(t, resp)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	ready := strings.Index(stream, `"type":"ready"`)
	failed := strings.Index(stream, `{"type":"error","code":"rate_limited"`)
	if ready < 0 || failed <= ready || strings.Contains(stream, "subscription.done") {
		t.Fatalf("stream = %s, want ready then a rate_limited error", stream)
	}
	for _, leaked := range []string{"throttle", "plan", "reset"} {
		if strings.Contains(stream, leaked) {
			t.Fatalf("stream leaks %q: %s", leaked, stream)
		}
	}
}

// An upstream outage that outlasts the retries is the transient
// subscription_upstream_unavailable, not the generic broker failure.
func TestUnary5xxAfterRetries503(t *testing.T) {
	var mu sync.Mutex
	attempts := 0
	app, _ := directStack(t, func(w http.ResponseWriter, _ *http.Request) {
		mu.Lock()
		attempts++
		mu.Unlock()
		w.WriteHeader(http.StatusServiceUnavailable)
		_, _ = io.WriteString(w, `{"error":{"message":"busy"}}`)
	}, directStackOptions{})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	if resp.StatusCode != http.StatusServiceUnavailable || decodeAPIError(t, body).Error.Code != "subscription_upstream_unavailable" || resp.Header.Get("Retry-After") != "" {
		t.Fatalf("status = %d Retry-After = %q body = %s", resp.StatusCode, resp.Header.Get("Retry-After"), body)
	}
	mu.Lock()
	defer mu.Unlock()
	if attempts != 1+codexsubscription.DefaultDirectMaxRetries {
		t.Fatalf("attempts = %d, want %d", attempts, 1+codexsubscription.DefaultDirectMaxRetries)
	}
}

// The broker's own invocation deadline is an upstream that did not answer.
func TestTimeout503(t *testing.T) {
	app, _ := directStack(t, func(w http.ResponseWriter, r *http.Request) {
		// Reading the body lets the server notice the client hanging up.
		_, _ = io.Copy(io.Discard, r.Body)
		select {
		case <-r.Context().Done():
		case <-time.After(3 * time.Second):
		}
	}, directStackOptions{invocationTimeout: 150 * time.Millisecond})
	started := time.Now()
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	if resp.StatusCode != http.StatusServiceUnavailable || decodeAPIError(t, body).Error.Code != "subscription_upstream_unavailable" {
		t.Fatalf("status = %d body = %s", resp.StatusCode, body)
	}
	if elapsed := time.Since(started); elapsed > time.Second {
		t.Fatalf("timeout took %v, want under 1s", elapsed)
	}
}

type holdingRunner struct {
	apiCodexRunner
	started chan struct{}
	release chan struct{}
}

func (r *holdingRunner) Invoke(_ context.Context, _ string, request codexsubscription.InvokeRequest) (codexsubscription.InvokeResponse, error) {
	r.started <- struct{}{}
	<-r.release
	return codexsubscription.InvokeResponse{RequestID: request.RequestID, Content: "ok", ModelUsed: request.Model}, nil
}

func TestQueueTimeout429(t *testing.T) {
	runner := &holdingRunner{started: make(chan struct{}, 1), release: make(chan struct{})}
	app, _ := fakeRunnerStack(t, runner, func(cfg *codexsubscription.Config) {
		cfg.MaxConcurrencyPerConnection = 1
		cfg.QueueTimeout = 50 * time.Millisecond
	})
	held := make(chan int, 1)
	go func() {
		req := httptest.NewRequest(http.MethodPost, "/internal/model-subscriptions/openai-codex/infer", strings.NewReader(directInferBody))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
		resp, err := app.Test(req, -1)
		if err != nil {
			held <- 0
			return
		}
		_ = resp.Body.Close()
		held <- resp.StatusCode
	}()
	<-runner.started
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	close(runner.release)
	if resp.StatusCode != http.StatusTooManyRequests || resp.Header.Get("Retry-After") != "1" || decodeAPIError(t, body).Error.Code != "rate_limited" {
		t.Fatalf("queued call: status = %d Retry-After = %q body = %s", resp.StatusCode, resp.Header.Get("Retry-After"), body)
	}
	if status := <-held; status != http.StatusOK {
		t.Fatalf("held call status = %d", status)
	}
}

type holdingStreamRunner struct{ holdingRunner }

func (r *holdingStreamRunner) InvokeStream(ctx context.Context, home string, request codexsubscription.InvokeRequest, _ func(string) error) (codexsubscription.InvokeResponse, error) {
	return r.Invoke(ctx, home, request)
}

// The streamed route, which chat answers use, must report a full queue the
// way the unary route does: a 429 with Retry-After before any 200 or ready
// event, never a mid-stream broker failure.
func TestStreamQueueTimeout429(t *testing.T) {
	runner := &holdingStreamRunner{holdingRunner{started: make(chan struct{}, 1), release: make(chan struct{})}}
	app, repo := fakeRunnerStack(t, runner, func(cfg *codexsubscription.Config) {
		cfg.MaxConcurrencyPerConnection = 1
		cfg.QueueTimeout = 50 * time.Millisecond
	})
	const path = "/internal/model-subscriptions/openai-codex/infer/stream"
	held := make(chan string, 1)
	go func() {
		req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(directInferBody))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
		resp, err := app.Test(req, -1)
		if err != nil {
			held <- ""
			return
		}
		payload, _ := io.ReadAll(resp.Body)
		_ = resp.Body.Close()
		held <- string(payload)
	}()
	<-runner.started
	resp := postInfer(t, app, path, directInferBody, -1)
	body := readBody(t, resp)
	close(runner.release)
	if resp.StatusCode != http.StatusTooManyRequests || resp.Header.Get("Retry-After") != "1" || decodeAPIError(t, body).Error.Code != "rate_limited" {
		t.Fatalf("queued stream: status = %d Retry-After = %q body = %s", resp.StatusCode, resp.Header.Get("Retry-After"), body)
	}
	if stream := <-held; !strings.Contains(stream, "subscription.done") {
		t.Fatalf("held stream = %q, want it to complete", stream)
	}

	// The finished stream released its slot.
	resp = postInfer(t, app, path, directInferBody, -1)
	if stream := readBody(t, resp); resp.StatusCode != http.StatusOK || !strings.Contains(stream, "subscription.done") {
		t.Fatalf("next stream: status = %d body = %s", resp.StatusCode, stream)
	}
	completed := 0
	for _, event := range auditEvents(t, repo) {
		if event.EventType == "subscription_inference.completed" {
			completed++
		}
	}
	if completed != 2 {
		t.Fatalf("completed audits = %d, want 2 (the queued call never ran)", completed)
	}
}

// A 401 the one successful forced refresh did not cure is a transient 503:
// the token endpoint accepted the sign-in, so the connection stays active,
// exactly as on the app-server transport.
func TestUnauthorizedTwiceAfterSuccessfulRefreshStaysActive(t *testing.T) {
	var mu sync.Mutex
	tokenCalls := 0
	app, repo := directStack(t, func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = io.WriteString(w, `{}`)
	}, directStackOptions{tokenHandler: func(w http.ResponseWriter, _ *http.Request) {
		mu.Lock()
		tokenCalls++
		mu.Unlock()
		payload, _ := json.Marshal(map[string]any{"exp": time.Now().Add(time.Hour).Unix()})
		_, _ = io.WriteString(w, `{"access_token":"`+base64.RawURLEncoding.EncodeToString([]byte("{\"alg\":\"HS256\",\"typ\":\"JWT\"}"))+`.`+base64.RawURLEncoding.EncodeToString(payload)+`.rotated"}`)
	}})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	if resp.StatusCode != http.StatusServiceUnavailable || decodeAPIError(t, body).Error.Code != "subscription_upstream_unavailable" {
		t.Fatalf("status = %d body = %s", resp.StatusCode, body)
	}
	mu.Lock()
	calls := tokenCalls
	mu.Unlock()
	if calls != 1 {
		t.Fatalf("token calls = %d, want exactly one refresh", calls)
	}
	connection, err := repo.GetConnection(t.Context(), directConnectionID)
	if err != nil {
		t.Fatal(err)
	}
	if connection.Status != "active" {
		t.Fatalf("connection = %s/%s, want it still active", connection.Status, connection.LastSyncStatus)
	}
}

// A 401 whose forced refresh the token endpoint refuses marks the connection
// needs_refresh.
func TestUnauthorizedWithRefusedRefreshMarksNeedsRefresh(t *testing.T) {
	app, repo := directStack(t, func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = io.WriteString(w, `{}`)
	}, directStackOptions{tokenHandler: func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = io.WriteString(w, `{"error":"invalid_grant"}`)
	}})
	resp := postInfer(t, app, "/internal/model-subscriptions/openai-codex/infer", directInferBody, -1)
	body := readBody(t, resp)
	if resp.StatusCode != http.StatusConflict || decodeAPIError(t, body).Error.Code != "subscription_reauth_required" {
		t.Fatalf("status = %d body = %s", resp.StatusCode, body)
	}
	connection, err := repo.GetConnection(t.Context(), directConnectionID)
	if err != nil {
		t.Fatal(err)
	}
	if connection.Status != "needs_refresh" || connection.LastSyncStatus != "reauthentication_required" {
		t.Fatalf("connection = %s/%s, want needs_refresh", connection.Status, connection.LastSyncStatus)
	}
}

func TestAuditMetadataUnchanged(t *testing.T) {
	app, repo := directStack(t, serveFrames(goldenUpstreamFrames()), directStackOptions{})
	for _, path := range []string{"/internal/model-subscriptions/openai-codex/infer", "/internal/model-subscriptions/openai-codex/infer/stream"} {
		resp := postInfer(t, app, path, directInferBody, -1)
		_ = readBody(t, resp)
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("%s status = %d", path, resp.StatusCode)
		}
	}
	counts := map[string]int{}
	for _, event := range auditEvents(t, repo) {
		if !strings.HasPrefix(event.EventType, "subscription_inference.") {
			continue
		}
		counts[event.EventType]++
		keys := make([]string, 0, len(event.Metadata))
		for key := range event.Metadata {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		joined := strings.Join(keys, ",")
		if joined != "model,requestId" && joined != "model,requestId,stream" {
			t.Fatalf("%s metadata keys = %s", event.EventType, joined)
		}
		if event.Metadata["model"] != "gpt-5.6-terra" || event.Metadata["requestId"] != "req_golden" {
			t.Fatalf("%s metadata = %v", event.EventType, event.Metadata)
		}
	}
	if counts["subscription_inference.requested"] != 2 || counts["subscription_inference.completed"] != 2 {
		t.Fatalf("audit counts = %v", counts)
	}
}

func TestHealthReportsCodexTransport(t *testing.T) {
	app, _ := directStack(t, serveFrames(goldenUpstreamFrames()), directStackOptions{})
	resp, err := app.Test(httptest.NewRequest(http.MethodGet, "/health/detailed", nil))
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()
	var health struct {
		Transport string `json:"codexSubscriptionTransport"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&health); err != nil {
		t.Fatal(err)
	}
	if health.Transport != "direct" {
		t.Fatalf("codexSubscriptionTransport = %q, want direct", health.Transport)
	}
}
