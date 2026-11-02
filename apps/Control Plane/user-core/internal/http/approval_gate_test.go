package http

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2: the execution-core surface (tool-gate-view, rule use, continuation
// check). Pure tier: principal and scope guards, body shape and the run-binding
// refusals, none of which need a database. The rule, entitlement and revocation
// legs are in approval_gate_live_test.go.

const (
	toolGateViewPath     = "/api/v1/internal/spaces/tool-gate-view"
	approvalUsePath      = "/api/v1/internal/spaces/approval-grants/use"
	approvalContinuePath = "/api/v1/internal/spaces/approval-continuation-check"
)

type fakeRunAuthority struct {
	authority spaces.RunActionAuthority
	err       error
}

func (f fakeRunAuthority) ResolveRunActionAuthority(context.Context, string, string) (spaces.RunActionAuthority, error) {
	return f.authority, f.err
}

func executionCoreContext(scopes ...string) map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": executionCorePrincipal, "service_scopes": scopes,
	}
}

func validApprovalGateBody() map[string]any {
	return map[string]any{
		"run_id": "run-1", "org_id": "org-1", "agent_subject_id": "agent-1",
		"tool_key": "mcp__github__list_issues", "action_key": "mcp__github__list_issues",
		"schema_hash": "sha256:" + strings.Repeat("a", 64), "approval_id": "approval-1",
	}
}

var approvalGateRoutes = []struct{ path, scope string }{
	{toolGateViewPath, spaceToolGateViewScope},
	{approvalUsePath, spaceApprovalGrantUseScope},
	{approvalContinuePath, spaceApprovalContinuationScope},
}

func TestApprovalGateRoutesRefuseTheGatewayCredential(t *testing.T) {
	ctx := gatewayContext()
	ctx["service_scopes"] = []string{spaceToolGateViewScope, spaceApprovalGrantUseScope, spaceApprovalContinuationScope}
	engine := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, ctx)
	for _, route := range approvalGateRoutes {
		rec := doJSON(engine, http.MethodPost, route.path, validApprovalGateBody())
		body := wantApprovalError(t, rec, http.StatusForbidden, "service_scope_required")
		if body.Error.Details["scope"] != route.scope {
			t.Errorf("%s: details = %v, want the required scope %q", route.path, body.Error.Details, route.scope)
		}
	}
}

func TestApprovalGateRoutesNeedExecutionCoreAndTheirOwnScope(t *testing.T) {
	for _, held := range approvalGateRoutes {
		engine := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext(held.scope))
		for _, route := range approvalGateRoutes {
			rec := doJSON(engine, http.MethodPost, route.path, map[string]any{})
			if route.scope == held.scope {
				// The guard passes; the empty body is then a 400.
				if rec.Code == http.StatusForbidden {
					t.Errorf("%s refused its own scope: %s", route.path, rec.Body.String())
				}
				continue
			}
			wantApprovalError(t, rec, http.StatusForbidden, "service_scope_required")
		}
	}
	// Right scope, wrong principal; and no scopes at all.
	other := executionCoreContext(spaceApprovalGrantUseScope)
	other["service_id"] = "documents-api"
	wantApprovalError(t, doJSON(approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, other), http.MethodPost, approvalUsePath, validApprovalGateBody()),
		http.StatusForbidden, "service_scope_required")
	wantApprovalError(t, doJSON(approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext()), http.MethodPost, approvalUsePath, validApprovalGateBody()),
		http.StatusForbidden, "service_scope_required")
	unauthenticated := map[string]any{"service_id": executionCorePrincipal, "service_scopes": []string{spaceApprovalGrantUseScope}}
	wantApprovalError(t, doJSON(approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, unauthenticated), http.MethodPost, approvalUsePath, validApprovalGateBody()),
		http.StatusForbidden, "service_scope_required")
}

func TestApprovalGateBodiesAreStrictAndShapeChecked(t *testing.T) {
	engines := map[string]string{}
	for _, route := range approvalGateRoutes {
		engines[route.path] = route.scope
	}
	for path, scope := range engines {
		engine := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext(scope))
		t.Run("unknown field "+path, func(t *testing.T) {
			body := validApprovalGateBody()
			body["subject_id"] = "someone-else"
			wantApprovalError(t, doJSON(engine, http.MethodPost, path, body), http.StatusBadRequest, "invalid_request")
		})
		t.Run("not json "+path, func(t *testing.T) {
			wantApprovalError(t, doJSON(engine, http.MethodPost, path, "[1,2"), http.StatusBadRequest, "invalid_request")
		})
	}
	use := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext(spaceApprovalGrantUseScope))
	for name, mutate := range map[string]func(map[string]any){
		"missing run":   func(b map[string]any) { delete(b, "run_id") },
		"missing tool":  func(b map[string]any) { delete(b, "tool_key") },
		"bad hash":      func(b map[string]any) { b["schema_hash"] = "sha256:short" },
		"missing agent": func(b map[string]any) { delete(b, "agent_subject_id") },
	} {
		t.Run("use "+name, func(t *testing.T) {
			body := validApprovalGateBody()
			mutate(body)
			wantApprovalError(t, doJSON(use, http.MethodPost, approvalUsePath, body), http.StatusBadRequest, "invalid_request")
		})
	}
	cont := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext(spaceApprovalContinuationScope))
	for name, mutate := range map[string]func(map[string]any){
		"missing approval id": func(b map[string]any) { delete(b, "approval_id") },
		"blank approval id":   func(b map[string]any) { b["approval_id"] = "  " },
		"oversized":           func(b map[string]any) { b["approval_id"] = strings.Repeat("a", 201) },
	} {
		t.Run("continuation "+name, func(t *testing.T) {
			body := validApprovalGateBody()
			mutate(body)
			wantApprovalError(t, doJSON(cont, http.MethodPost, approvalContinuePath, body), http.StatusBadRequest, "invalid_request")
		})
	}
	view := approvalHarness(t, &spaces.Repository{}, fakeRunAuthority{}, executionCoreContext(spaceToolGateViewScope))
	for _, field := range []string{"run_id", "org_id"} {
		body := validApprovalGateBody()
		delete(body, field)
		wantApprovalError(t, doJSON(view, http.MethodPost, toolGateViewPath, body), http.StatusBadRequest, "invalid_request")
	}
}

func TestApprovalGateRunBindingFailsClosed(t *testing.T) {
	scopes := map[string]string{toolGateViewPath: spaceToolGateViewScope, approvalUsePath: spaceApprovalGrantUseScope, approvalContinuePath: spaceApprovalContinuationScope}
	for path, scope := range scopes {
		// No resolver wired.
		wantApprovalError(t, doJSON(approvalHarness(t, &spaces.Repository{}, nil, executionCoreContext(scope)), http.MethodPost, path, validApprovalGateBody()),
			http.StatusServiceUnavailable, "approval_authority_unavailable")
		// The resolver cannot answer.
		failing := fakeRunAuthority{err: errors.New("session-core down")}
		rec := doJSON(approvalHarness(t, &spaces.Repository{}, failing, executionCoreContext(scope)), http.MethodPost, path, validApprovalGateBody())
		wantApprovalError(t, rec, http.StatusServiceUnavailable, "approval_authority_unavailable")
		if strings.Contains(rec.Body.String(), "session-core down") {
			t.Errorf("%s: the resolver error leaked: %s", path, rec.Body.String())
		}
		// A run that no longer resolves to a complete, active binding.
		inactive := fakeRunAuthority{authority: spaces.RunActionAuthority{RunID: "run-1"}}
		wantApprovalError(t, doJSON(approvalHarness(t, &spaces.Repository{}, inactive, executionCoreContext(scope)), http.MethodPost, path, validApprovalGateBody()),
			http.StatusForbidden, "authority_changed")
		// No repository: fail closed rather than panic.
		wantApprovalError(t, doJSON(approvalHarness(t, nil, fakeRunAuthority{}, executionCoreContext(scope)), http.MethodPost, path, validApprovalGateBody()),
			http.StatusServiceUnavailable, "approval_authority_unavailable")
	}
}
