package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS12-S5: the message-lifecycle routes. Pure tier: scope table, delegation and
// principal guards, body shape, fail-closed answers. The resolver and policy
// legs that need a database are in spaces/message_lifecycle_live_test.go.

const (
	reviseDecisionPath = "/api/v1/internal/spaces/thread-revise-decision"
	messagePolicyPath  = "/api/v1/internal/spaces/message-policy"
)

func validReviseBody() map[string]any {
	return map[string]any{
		"space_ref": "room-1", "thread_id": "thread-1", "message_id": "msg-1", "action": "edit",
		"content_digest": "sha256:" + strings.Repeat("a", 64), "idempotency_key": "revise-1",
	}
}

func TestMessagePolicyRoutesRequireScopes(t *testing.T) {
	const base = "/api/v1/internal/spaces"
	for _, tc := range []struct{ method, path, want string }{
		{http.MethodPost, base + "/thread-revise-decision", "spaces:issue"},
		{http.MethodGet, base + "/room-1/message-policy", "spaces:resolve"},
		{http.MethodPut, base + "/message-policy", "spaces:policy:write"},
	} {
		if got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil)); got != tc.want {
			t.Errorf("%s %s scope = %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}
	// The two user-facing scopes are delegated; the operator scope is not, and
	// it is held by Control's own policy principal only.
	if !serviceScopeRequiresVerifiedDelegation("spaces:issue") || !serviceScopeRequiresVerifiedDelegation("spaces:resolve") {
		t.Fatal("issue and resolve must require a verified gateway delegation")
	}
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, base + "/thread-revise-decision"},
		{http.MethodPut, base + "/thread-revise-decision"},
		{http.MethodPost, base + "/room-1/message-policy"},
		{http.MethodPut, base + "/room-1/message-policy"},
		{http.MethodDelete, base + "/message-policy"},
		{http.MethodPost, base + "/message-policy"},
	} {
		for _, rule := range spaceFeatureScopeRules {
			if scope, ok := rule(tc.method, tc.path); ok && scope == "spaces:policy:write" {
				t.Errorf("%s %s must not map to the operator write scope", tc.method, tc.path)
			}
		}
	}
}

func TestMessageLifecycleRoutesAreRegisteredExactlyOnce(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	for _, route := range []string{
		"POST /api/v1/internal/spaces/thread-revise-decision",
		"GET /api/v1/internal/spaces/:space_ref/message-policy",
		"PUT /api/v1/internal/spaces/message-policy",
	} {
		if counts[route] != 1 {
			t.Errorf("%s registered %d times, want 1", route, counts[route])
		}
	}
}

func TestMessageLifecycleRoutesAreNotReachableByAPlainServiceCredential(t *testing.T) {
	// Issuer and hints: the gateway principal without a verified delegation, and a
	// verified delegation from any other principal, are both refused.
	for name, ctx := range map[string]map[string]any{
		"gateway, no delegation": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "org_id": "o", "user_id": "u"},
		"other principal":        {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true, "org_id": "o", "user_id": "u"},
		"no auth":                {},
	} {
		engine := approvalHarness(t, &spaces.Repository{}, nil, ctx)
		for _, call := range []struct{ method, path string }{
			{http.MethodPost, reviseDecisionPath},
			{http.MethodGet, "/api/v1/internal/spaces/room-1/message-policy"},
		} {
			rec := doJSON(engine, call.method, call.path, validReviseBody())
			if rec.Code != http.StatusForbidden {
				t.Errorf("%s: %s %s = %d %s", name, call.method, call.path, rec.Code, rec.Body.String())
			}
		}
	}
	// The operator path: only Control's policy principal with spaces:policy:write.
	for name, ctx := range map[string]map[string]any{
		"gateway":                            gatewayContext(),
		"policy principal without the scope": {"auth_method": "service_principal", "service_id": controlSpacePolicyPrincipal, "service_scopes": []string{"spaces:issue"}},
		"other principal with the scope":     {"auth_method": "service_principal", "service_id": "documents-api", "service_scopes": []string{"spaces:policy:write"}},
		"no auth":                            {},
	} {
		engine := approvalHarness(t, &spaces.Repository{}, nil, ctx)
		rec := doJSON(engine, http.MethodPut, messagePolicyPath, map[string]any{"org_id": "org-1", "original_retention": "retain"})
		if rec.Code != http.StatusForbidden {
			t.Errorf("%s: PUT message-policy = %d %s", name, rec.Code, rec.Body.String())
		}
	}
}

func TestThreadReviseDecisionBodyIsValidatedBeforeAnyEvidenceRead(t *testing.T) {
	// An empty repository has no database, so a 400 proves the check came first.
	engine := approvalHarness(t, &spaces.Repository{}, nil, gatewayContext())
	for name, mutate := range map[string]func(map[string]any){
		"missing space":    func(b map[string]any) { delete(b, "space_ref") },
		"missing message":  func(b map[string]any) { delete(b, "message_id") },
		"missing digest":   func(b map[string]any) { delete(b, "content_digest") },
		"unknown action":   func(b map[string]any) { b["action"] = "redact" },
		"short digest":     func(b map[string]any) { b["content_digest"] = "sha256:abc" },
		"delete with text": func(b map[string]any) { b["action"] = "delete" },
	} {
		body := validReviseBody()
		mutate(body)
		rec := doJSON(engine, http.MethodPost, reviseDecisionPath, body)
		if rec.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d %s", name, rec.Code, rec.Body.String())
		}
	}
}

func TestMessageLifecycleRoutesFailClosedWithoutARepository(t *testing.T) {
	engine := approvalHarness(t, nil, nil, gatewayContext())
	for _, call := range []struct{ method, path string }{
		{http.MethodPost, reviseDecisionPath},
		{http.MethodGet, "/api/v1/internal/spaces/room-1/message-policy"},
	} {
		rec := doJSON(engine, call.method, call.path, validReviseBody())
		if rec.Code != http.StatusServiceUnavailable {
			t.Errorf("%s %s: want 503, got %d %s", call.method, call.path, rec.Code, rec.Body.String())
		}
	}
	operator := approvalHarness(t, nil, nil, policyWriterCtx())
	rec := doJSON(operator, http.MethodPut, messagePolicyPath, map[string]any{"org_id": "org-1", "original_retention": "retain"})
	if rec.Code != http.StatusServiceUnavailable {
		t.Errorf("PUT message-policy without a repository: %d %s", rec.Code, rec.Body.String())
	}
}

func TestMessagePolicyWriteBodyIsStrictAndClosed(t *testing.T) {
	engine := approvalHarness(t, &spaces.Repository{}, nil, policyWriterCtx())
	for name, body := range map[string]any{
		"missing org":     map[string]any{"original_retention": "retain"},
		"blank org":       map[string]any{"org_id": "  ", "original_retention": "retain"},
		"missing rule":    map[string]any{"org_id": "org-1"},
		"unknown rule":    map[string]any{"org_id": "org-1", "original_retention": "keep"},
		"unknown field":   map[string]any{"org_id": "org-1", "original_retention": "retain", "extra": 1},
		"not json":        "nope",
		"upper case rule": map[string]any{"org_id": "org-1", "original_retention": "RETAIN"},
	} {
		rec := doJSON(engine, http.MethodPut, messagePolicyPath, body)
		if rec.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d %s", name, rec.Code, rec.Body.String())
		}
	}
}
