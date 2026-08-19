package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
)

func init() { gin.SetMode(gin.TestMode) }

// The facade handlers must reject incomplete requests (400) and fail safe when
// the repository is not wired (503) — never a panic or a silent 200. These
// paths need no DB; the grant logic itself is covered by the AclRepository
// integration tests.
func TestAuthzVisibleValidation(t *testing.T) {
	s := &Server{aclRepo: nil} // not wired

	// Missing required params → 400 (checked before the nil-repo guard).
	c, w := newGinCtx("/api/v1/internal/authz/visible?org_id=org-1")
	s.authzVisible(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("missing subject_id/resource_type: want 400, got %d", w.Code)
	}

	// All params present but repo unwired → 503 (fail safe, not 500/200).
	c, w = newGinCtx("/api/v1/internal/authz/visible?org_id=org-1&subject_id=u1&resource_type=document")
	s.authzVisible(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repo: want 503, got %d", w.Code)
	}
}

func TestAuthzCheckValidation(t *testing.T) {
	s := &Server{aclRepo: nil}

	c, w := newGinCtx("/api/v1/internal/authz/check?org_id=org-1&resource_type=document")
	s.authzCheck(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("missing resource_id/subject_id: want 400, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/authz/check?org_id=org-1&resource_type=document&resource_id=d1&subject_id=u1")
	s.authzCheck(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repo: want 503, got %d", w.Code)
	}
}

func TestRequireServicePrincipal(t *testing.T) {
	s := &Server{}

	c, w := newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "bearer")
	s.requireServicePrincipal(c)
	if w.Code != http.StatusForbidden {
		t.Fatalf("bearer caller: want 403, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "service_principal")
	s.requireServicePrincipal(c)
	if w.Code != http.StatusOK {
		t.Fatalf("service principal: want pass-through (200), got %d", w.Code)
	}
}

func TestRequireSpaceLifecycleRegistrar(t *testing.T) {
	s := &Server{}
	for _, serviceID := range []string{"", "verevon-gateway", "application-space-lifecycle"} {
		c, w := newGinCtx("/api/v1/internal/spaces/register")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", serviceID)
		s.requireSpaceLifecycleRegistrar(c)
		if serviceID == "application-space-lifecycle" {
			if w.Code != http.StatusOK {
				t.Fatalf("Space lifecycle principal: want pass-through (200), got %d", w.Code)
			}
		} else if w.Code != http.StatusForbidden {
			t.Fatalf("service id %q: want 403, got %d", serviceID, w.Code)
		}
	}
}

func TestRequireSpaceAudiencePublisherRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: "verevon-gateway", scopes: []string{"spaces:audience:publish"}},
		{serviceID: applicationSpaceLifecyclePrincipal, scopes: []string{"spaces:register"}},
		{serviceID: applicationSpaceLifecyclePrincipal, scopes: []string{"spaces:audience:publish"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/recipient-audiences")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceAudiencePublisher(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible audience publisher denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible audience publisher allowed: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireSpacePolicyWriterRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: "verevon-gateway", scopes: []string{"spaces:policy:write"}},
		{serviceID: controlSpacePolicyPrincipal, scopes: []string{"spaces:register"}},
		{serviceID: controlSpacePolicyPrincipal, scopes: []string{"spaces:policy:write"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/effect-policy")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpacePolicyWriter(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("policy writer: want pass-through (200), got %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible policy writer allowed: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireSpaceImportReauthorizer(t *testing.T) {
	s := &Server{}
	for _, serviceID := range []string{"", "verevon-gateway", importsCorePrincipal} {
		c, w := newGinCtx("/api/v1/internal/spaces/import-execution-decision")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", serviceID)
		s.requireSpaceImportReauthorizer(c)
		if serviceID == importsCorePrincipal {
			if w.Code != http.StatusOK {
				t.Fatalf("imports-core reauthorizer: want pass-through (200), got %d", w.Code)
			}
		} else if w.Code != http.StatusForbidden {
			t.Fatalf("service id %q: want 403, got %d", serviceID, w.Code)
		}
	}
}

func TestRequireSpaceScheduleFireReauthorizerRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: importsCorePrincipal, scopes: []string{"spaces:schedule:reauthorize"}},
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:import:reauthorize"}},
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:schedule:reauthorize"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/schedule-fire-decision")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceScheduleFireReauthorizer(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible schedule reauthorizer denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible schedule reauthorizer allowed: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireSpaceScheduledRunExecutorRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:schedule:execute"}},
		{serviceID: orchestratorCorePrincipal, scopes: []string{"spaces:schedule:reauthorize"}},
		{serviceID: orchestratorCorePrincipal, scopes: []string{"spaces:schedule:execute"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/scheduled-run-execution-decision")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceScheduledRunExecutor(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible scheduled-run executor denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible scheduled-run executor accepted: %d", w.Code)
		}
	}
}

func TestRequireSpaceScheduledStepExecutorRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:schedule:step"}},
		{serviceID: orchestratorCorePrincipal, scopes: []string{"spaces:schedule:execute"}},
		{serviceID: orchestratorCorePrincipal, scopes: []string{"spaces:schedule:step"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/scheduled-step-decision")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceScheduledStepExecutor(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible scheduled-step executor denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible scheduled-step executor accepted: %d", w.Code)
		}
	}
}

func TestRequireSpaceAgentActionAuthorizerRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:agent-action:reauthorize"}},
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:schedule:reauthorize"}},
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:agent-action:reauthorize"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/run-action-decision")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceAgentActionAuthorizer(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible agent action authorizer denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible agent action authorizer accepted: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireSpaceAgentActionViewerRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: capabilityCorePrincipal, scopes: []string{"spaces:agent-action:view"}},
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:agent-action:reauthorize"}},
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:agent-action:view"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/model-action-view")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceAgentActionViewer(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible model action viewer denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible model action viewer accepted: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireCurrentRunActionAuthorityCheckerRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:agent-action:current-authority"}},
		{serviceID: conversationCorePrincipal, scopes: []string{"spaces:agent-action:reauthorize"}},
		{serviceID: conversationCorePrincipal, scopes: []string{"spaces:agent-action:current-authority"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/run-action-authority-check")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireCurrentRunActionAuthorityChecker(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible current authority checker denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible current authority checker accepted: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireOwnerEffectReservationCoordinatorRequiresExactPrincipalAndScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		serviceID string
		scopes    []string
		allowed   bool
	}{
		{serviceID: executionCorePrincipal, scopes: []string{"spaces:agent-action:reservation"}},
		{serviceID: conversationCorePrincipal, scopes: []string{"spaces:agent-action:current-authority"}},
		{serviceID: conversationCorePrincipal, scopes: []string{"spaces:agent-action:reservation"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/owner-effect-reservations/reserve")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", candidate.serviceID)
		c.Set("service_scopes", candidate.scopes)
		s.requireOwnerEffectReservationCoordinator(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible reservation coordinator denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible reservation coordinator accepted: service=%q scopes=%v", candidate.serviceID, candidate.scopes)
		}
	}
}

func TestRequireSpaceDeletionAuthorizerRequiresDedicatedScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		scopes  []string
		allowed bool
	}{
		{scopes: []string{"spaces:register"}},
		{scopes: []string{"spaces:deletion:authorize"}, allowed: true},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/deletion-authorizations")
		c.Set("auth_method", "service_principal")
		c.Set("service_id", applicationSpaceLifecyclePrincipal)
		c.Set("service_scopes", candidate.scopes)
		s.requireSpaceDeletionAuthorizer(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("eligible deletion authorizer denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("ineligible deletion authorizer allowed: scopes=%v", candidate.scopes)
		}
	}
}

func TestRegisterSpaceFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"space_ref":"space-1","org_id":"org-1","owner_principal_id":"user-1","kind":"personal","lifecycle":"pending_registration","lifecycle_revision":1}`)
	s.registerSpace(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestRequireVerifiedSpaceResolver(t *testing.T) {
	s := &Server{}
	c, w := newGinCtx("/api/v1/internal/spaces/space-1/membership")
	c.Set("auth_method", "service_principal")
	c.Set("service_id", verevonGatewayPrincipal)
	s.requireVerifiedSpaceResolver(c)
	if w.Code != http.StatusForbidden {
		t.Fatalf("unsigned delegation: want 403, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/spaces/space-1/membership")
	c.Set("auth_method", "service_principal")
	c.Set("service_id", verevonGatewayPrincipal)
	c.Set("delegation_verified", true)
	s.requireVerifiedSpaceResolver(c)
	if w.Code != http.StatusOK {
		t.Fatalf("verified gateway delegation: want pass-through (200), got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/spaces/space-1/membership")
	c.Set("auth_method", "service_principal")
	c.Set("service_id", applicationSpaceLifecyclePrincipal)
	c.Set("delegation_verified", true)
	s.requireVerifiedSpaceResolver(c)
	if w.Code != http.StatusForbidden {
		t.Fatalf("wrong workload: want 403, got %d", w.Code)
	}
}

func TestResolveCurrentSpaceMembershipFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinCtx("/api/v1/internal/spaces/space-1/membership")
	s.resolveCurrentSpaceMembership(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestIssuePersonalThreadDecisionFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"space_ref":"space-1","session_key":"session-1","idempotency_key":"thread-create-1"}`)
	s.issuePersonalThreadDecision(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestIssuePersonalRetrievalDecisionFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"space_ref":"space-1","idempotency_key":"retrieval-1"}`)
	s.issuePersonalRetrievalDecision(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestIssueRetrievalDecisionFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"space_ref":"space-1","idempotency_key":"retrieval-1"}`)
	s.issueRetrievalDecision(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestIssuePersonalImportDecisionFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"space_ref":"space-1","idempotency_key":"import-1"}`)
	s.issuePersonalImportDecision(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

func TestUpsertSpaceEffectPolicyFailsClosedWhenRepositoryIsUnavailable(t *testing.T) {
	s := &Server{}
	c, w := newGinJSONCtx(`{"org_id":"org-1","privacy_policy_ref":"privacy-1","purpose":"assistant_collaboration","lawful_basis":"contract","privacy_class":"internal","retention_class":"standard","residency":"swedencentral","deletion_scope":"space"}`)
	s.upsertSpaceEffectPolicy(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired Space repository: want 503, got %d", w.Code)
	}
}

// Static service credentials prove only workload identity. Without a verified
// delegation binding tenant, subject, resource and actor, neither a user bearer
// nor a service principal may reach the authz facade.
func TestRequireVerifiedAuthzDelegation(t *testing.T) {
	s := &Server{}
	for _, authMethod := range []string{"bearer", "service_principal"} {
		c, w := newGinCtx("/api/v1/internal/authz/visible")
		c.Set("auth_method", authMethod)
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusForbidden {
			t.Fatalf("%s caller: want 403, got %d", authMethod, w.Code)
		}
	}

	t.Run("matching signed read delegation passes", func(t *testing.T) {
		c, w := newGinCtx("/api/v1/internal/authz/visible?org_id=org-1&subject_id=user-1&resource_type=document")
		c.Set("auth_method", "service_principal")
		c.Set("delegation_verified", true)
		c.Set("delegated_user_proof_verified", true)
		c.Set("delegation_version", "v2")
		c.Set("delegation_operation", "authz:visible")
		c.Set("delegation_resource_type", "document")
		c.Set("delegation_reason", "resolve explicit grants")
		c.Set("delegation_zdr", "true")
		c.Set("org_id", "org-1")
		c.Set("user_id", "user-1")
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusOK {
			t.Fatalf("matching signed read: want pass-through, got %d", w.Code)
		}
	})

	for _, target := range []string{
		"/api/v1/internal/authz/visible?org_id=other-org&subject_id=user-1&resource_type=document",
		"/api/v1/internal/authz/visible?org_id=org-1&subject_id=other-user&resource_type=document",
		"/api/v1/internal/authz/grants?org_id=org-1&resource_type=document&resource_id=doc-1",
	} {
		c, w := newGinCtx(target)
		c.Set("auth_method", "service_principal")
		c.Set("delegation_verified", true)
		c.Set("delegated_user_proof_verified", true)
		c.Set("delegation_version", "v2")
		c.Set("delegation_operation", "authz:visible")
		c.Set("delegation_resource_type", "document")
		c.Set("delegation_reason", "resolve explicit grants")
		c.Set("delegation_zdr", "true")
		c.Set("org_id", "org-1")
		c.Set("user_id", "user-1")
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusForbidden {
			t.Fatalf("unbounded target %q: want 403, got %d", target, w.Code)
		}
	}
}

func newGinCtx(target string) (*gin.Context, *httptest.ResponseRecorder) {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodGet, target, nil)
	return c, w
}

func newGinJSONCtx(body string) (*gin.Context, *httptest.ResponseRecorder) {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/register", bytes.NewBufferString(body))
	c.Request.Header.Set("Content-Type", "application/json")
	return c, w
}
