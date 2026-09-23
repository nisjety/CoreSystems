package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

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
	if decoded.Error.Code != "subscription_reauthentication_required" {
		t.Fatalf("error code = %q, want subscription_reauthentication_required", decoded.Error.Code)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-stale-subscription")
	if err != nil {
		t.Fatalf("GetConnection: %v", err)
	}
	if connection.Status != "needs_refresh" || connection.LastSyncStatus != "reauthentication_required" {
		t.Fatalf("connection status = %q/%q, want needs_refresh/reauthentication_required", connection.Status, connection.LastSyncStatus)
	}
}

type apiCodexRunner struct {
	lastRequest codexsubscription.InvokeRequest
	invocations int
	invokeErr   error
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
	return codexsubscription.InvokeResponse{RequestID: request.RequestID, Content: "subscription answer", ModelUsed: request.Model}, nil
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
	return codexsubscription.InvokeResponse{RequestID: request.RequestID, Content: "subscription answer", ModelUsed: request.Model}, nil
}

func (r *apiCodexRunner) Logout(context.Context, string) error { return nil }
