package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestCodexSubscriptionInferIsScopedAndNeverLeasesAToken(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	runner := &apiCodexRunner{}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{
		Enabled: true,
		Home:    t.TempDir(),
	}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-subscription",
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	})
	if err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{
		Config:             cfg,
		Repo:               repo,
		OAuth:              service,
		CodexSubscriptions: manager,
	})

	body := `{"organizationId":"org-1","userId":"user-1","connectionId":"conn-subscription","requestId":"req-1","model":"gpt-5.6-terra","outputSchema":{"type":"object"},"messages":[{"role":"user","content":"hello"}]}`
	req := httptest.NewRequest(http.MethodPost, "/internal/model-subscriptions/openai-codex/infer", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	var decoded struct {
		Data struct {
			Response codexsubscription.InvokeResponse `json:"response"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if decoded.Data.Response.Content != "subscription answer" || runner.invocations != 1 {
		t.Fatalf("response=%+v invocations=%d", decoded.Data.Response, runner.invocations)
	}

	if string(runner.lastRequest.OutputSchema) != `{"type":"object"}` || runner.lastRequest.Model != "gpt-5.6-terra" || runner.lastRequest.ConnectionID != "conn-subscription" {
		t.Fatal("structured output lost the verified subscription route")
	}
	// A service caller cannot substitute another user for the same opaque id.
	mismatch := httptest.NewRequest(http.MethodPost, "/internal/model-subscriptions/openai-codex/infer", strings.NewReader(strings.Replace(body, "user-1", "user-2", 1)))
	mismatch.Header.Set("Content-Type", "application/json")
	mismatch.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
	mismatchResp, err := app.Test(mismatch)
	if err != nil {
		t.Fatalf("app.Test mismatch: %v", err)
	}
	defer mismatchResp.Body.Close()
	if mismatchResp.StatusCode != http.StatusForbidden || runner.invocations != 1 {
		t.Fatalf("mismatch status=%d invocations=%d, want 403 and no second invocation", mismatchResp.StatusCode, runner.invocations)
	}
}

func TestCodexSubscriptionUsageReturnsProviderWindowsWithoutAllowanceInference(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	reset := time.Date(2026, 10, 1, 15, 0, 0, 0, time.UTC)
	windowMinutes := 300
	runner := &apiCodexRunner{rateLimits: codexsubscription.RateLimitReport{
		ObservedAt: time.Date(2026, 10, 1, 10, 0, 0, 0, time.UTC),
		Primary:    &codexsubscription.RateLimitWindow{UsedPercent: 42, WindowMinutes: &windowMinutes, ResetsAt: &reset},
	}}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-allowance", ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: "org-1", UserID: "user-1", Status: "connected", Capabilities: []string{codexsubscription.Capability},
	})
	if err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager})
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-subscriptions/openai-codex/connections/conn-allowance/usage", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(body), `"scope":"connected_user"`) || !strings.Contains(string(body), `"usedPercent":42`) || strings.Contains(string(body), "planType") || strings.Contains(string(body), "tokenAllowance") {
		t.Fatalf("allowance response is missing provider values or exposes inferred data: %s", body)
	}
	if runner.rateLimitReads != 1 || runner.invocations != 0 {
		t.Fatalf("rate-limit reads=%d inference calls=%d", runner.rateLimitReads, runner.invocations)
	}
	if !runner.rateLimitHasDeadline {
		t.Fatal("provider allowance read must have a request deadline")
	}
}

func TestCodexSubscriptionInferStreamsDeltasAndCompletion(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	runner := &apiCodexRunner{}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{
		Enabled: true,
		Home:    t.TempDir(),
	}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-stream",
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	})
	if err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{
		Config:             cfg,
		Repo:               repo,
		OAuth:              service,
		CodexSubscriptions: manager,
	})

	body := `{"organizationId":"org-1","userId":"user-1","connectionId":"conn-stream","requestId":"req-stream","model":"gpt-codex","messages":[{"role":"user","content":"hello"}]}`
	req := httptest.NewRequest(http.MethodPost, "/internal/model-subscriptions/openai-codex/infer/stream", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	if got := resp.Header.Get("Content-Type"); !strings.Contains(got, "text/event-stream") {
		t.Fatalf("content-type = %q, want text/event-stream", got)
	}
	payload, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("read stream: %v", err)
	}
	stream := string(payload)
	first := strings.Index(stream, `"type":"delta","delta":"subscription "`)
	second := strings.Index(stream, `"type":"delta","delta":"answer"`)
	done := strings.Index(stream, `"type":"done","requestId":"req-stream"`)
	if first < 0 || second <= first || done <= second {
		t.Fatalf("stream events out of order: %s", stream)
	}
}

func TestCodexSubscriptionInferMarksMissingAuthenticationForReconnect(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	runner := &apiCodexRunner{invokeErr: codexsubscription.ErrReauthenticationRequired}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{
		Enabled: true,
		Home:    t.TempDir(),
	}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-stale-subscription",
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	})
	if err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{
		Config:             cfg,
		Repo:               repo,
		OAuth:              service,
		CodexSubscriptions: manager,
	})

	body := `{"organizationId":"org-1","userId":"user-1","connectionId":"conn-stale-subscription","requestId":"req-1","model":"gpt-codex","messages":[{"role":"user","content":"hello"}]}`
	req := httptest.NewRequest(http.MethodPost, "/internal/model-subscriptions/openai-codex/infer", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusConflict {
		t.Fatalf("status = %d, want 409", resp.StatusCode)
	}
	var decoded struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if decoded.Error.Code != "subscription_reauth_required" {
		t.Fatalf("error code = %q, want subscription_reauth_required", decoded.Error.Code)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-stale-subscription")
	if err != nil {
		t.Fatalf("GetConnection: %v", err)
	}
	if connection.Status != "needs_refresh" || connection.LastSyncStatus != "reauthentication_required" {
		t.Fatalf("connection status = %q/%q, want needs_refresh/reauthentication_required", connection.Status, connection.LastSyncStatus)
	}
}

func TestCodexSubscriptionReportsUsageAndPhasesOnBothRoutes(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	runner := &apiCodexRunner{
		usage:  &codexsubscription.TokenUsage{InputTokens: 1800, CachedInputTokens: 1024, CacheWriteInputTokens: 256, OutputTokens: 120, ReasoningOutputTokens: 40},
		phases: map[string]int64{codexsubscription.PhaseSpawn: 41, codexsubscription.PhaseTurnTotal: 9000},
	}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-usage",
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	}); err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager})
	body := `{"organizationId":"org-1","userId":"user-1","connectionId":"conn-usage","requestId":"req-usage","model":"gpt-5.6-terra","messages":[{"role":"user","content":"hello"}]}`
	send := func(path string) string {
		t.Helper()
		req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", "codex-subscription-test-key")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test %s: %v", path, err)
		}
		defer resp.Body.Close()
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("%s status = %d, want 200", path, resp.StatusCode)
		}
		payload, err := io.ReadAll(resp.Body)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		return string(payload)
	}
	const usage = `"usage":{"inputTokens":1800,"cachedInputTokens":1024,"cacheWriteInputTokens":256,"outputTokens":120,"reasoningOutputTokens":40}`
	const phases = `"phasesMs":{"spawn":41,"turn_total":9000}`

	unary := send("/internal/model-subscriptions/openai-codex/infer")
	if !strings.Contains(unary, usage) || !strings.Contains(unary, phases) {
		t.Fatalf("unary envelope lacks usage or phases: %s", unary)
	}

	stream := send("/internal/model-subscriptions/openai-codex/infer/stream")
	done := strings.Index(stream, `"type":"done"`)
	if done < 0 {
		t.Fatalf("stream has no done event: %s", stream)
	}
	doneEvent := stream[done:]
	if !strings.Contains(doneEvent, usage) || !strings.Contains(doneEvent, phases) {
		t.Fatalf("done event lacks usage or phases: %s", doneEvent)
	}
	if head := stream[:done]; strings.Contains(head, `"usage"`) || strings.Contains(head, `"phasesMs"`) {
		t.Fatalf("usage or phases leaked onto a non-terminal event: %s", head)
	}
}

func TestCodexSubscriptionDoneEventOmitsUnreportedUsage(t *testing.T) {
	encoded, err := json.Marshal(codexSubscriptionStreamEvent{Type: "done", RequestID: "req-1", ModelUsed: "gpt-5.6-terra"})
	if err != nil {
		t.Fatal(err)
	}
	// An older Model Plane build parses exactly this shape.
	if string(encoded) != `{"type":"done","requestId":"req-1","modelUsed":"gpt-5.6-terra"}` {
		t.Fatalf("done event = %s", encoded)
	}
}

type apiCodexRunner struct {
	lastRequest          codexsubscription.InvokeRequest
	invocations          int
	invokeErr            error
	usage                *codexsubscription.TokenUsage
	phases               map[string]int64
	rateLimits           codexsubscription.RateLimitReport
	rateLimitReads       int
	rateLimitHasDeadline bool
}

func (r *apiCodexRunner) BeginDeviceLogin(context.Context, string) (codexsubscription.LoginProcess, codexsubscription.DeviceCode, error) {
	return nil, codexsubscription.DeviceCode{}, nil
}

func (r *apiCodexRunner) Invoke(_ context.Context, _ string, request codexsubscription.InvokeRequest) (codexsubscription.InvokeResponse, error) {
	r.invocations++
	r.lastRequest = request
	if r.invokeErr != nil {
		return codexsubscription.InvokeResponse{}, r.invokeErr
	}
	return codexsubscription.InvokeResponse{RequestID: request.RequestID, Content: "subscription answer", ModelUsed: request.Model, Usage: r.usage, PhasesMs: r.phases}, nil
}

func (r *apiCodexRunner) InvokeStream(_ context.Context, _ string, request codexsubscription.InvokeRequest, onDelta func(string) error) (codexsubscription.InvokeResponse, error) {
	r.invocations++
	if r.invokeErr != nil {
		return codexsubscription.InvokeResponse{}, r.invokeErr
	}
	if err := onDelta("subscription "); err != nil {
		return codexsubscription.InvokeResponse{}, err
	}
	if err := onDelta("answer"); err != nil {
		return codexsubscription.InvokeResponse{}, err
	}
	return codexsubscription.InvokeResponse{RequestID: request.RequestID, Content: "subscription answer", ModelUsed: request.Model, Usage: r.usage, PhasesMs: r.phases}, nil
}

func (r *apiCodexRunner) Logout(context.Context, string) error { return nil }

func (r *apiCodexRunner) ReadRateLimits(ctx context.Context, _ string) (codexsubscription.RateLimitReport, error) {
	r.rateLimitReads++
	_, r.rateLimitHasDeadline = ctx.Deadline()
	return r.rateLimits, nil
}
