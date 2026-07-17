package authz

import (
	"net/http"
	"net/http/httptest"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

func TestAuthorizeHTTPPinsTenantAndRequiresWriteScope(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	writer := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service", Scopes: []string{"capability:write"}, RetentionPolicyPresent: true}
	healthWriter := authctx.Principal{OrganizationID: "org-a", ActorID: "health-a", PrincipalType: "service", Scopes: []string{HealthWriteScope}, RetentionPolicyPresent: true}
	globalHealthWriter := authctx.Principal{OrganizationID: "ops", ActorID: "health-global", PrincipalType: "service", Scopes: []string{GlobalHealthWriteScope}, RetentionPolicyPresent: true}

	tests := []struct {
		name      string
		principal authctx.Principal
		method    string
		target    string
		wantErr   bool
	}{
		{name: "user may read own tenant", principal: user, method: http.MethodGet, target: "/api/v1/mcp?org_id=org-a"},
		{name: "wrong tenant is denied", principal: user, method: http.MethodGet, target: "/api/v1/mcp?org_id=org-b", wantErr: true},
		{name: "forged actor query is denied", principal: user, method: http.MethodGet, target: "/api/v1/memory?user_id=user-b", wantErr: true},
		{name: "user without scope may not mutate", principal: user, method: http.MethodPost, target: "/api/v1/mcp", wantErr: true},
		{name: "signed writer may mutate", principal: writer, method: http.MethodPost, target: "/api/v1/mcp"},
		{name: "ordinary writer may not attest health", principal: writer, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "user may not attest health", principal: user, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "exact health workload may attest", principal: healthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability"},
		{name: "global health workload may enter health route", principal: globalHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability"},
		{name: "tenant-agnostic command execution is quarantined", principal: writer, method: http.MethodPost, target: "/api/v1/commands/exec", wantErr: true},
		{name: "service read needs scope", principal: authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service"}, method: http.MethodGet, target: "/api/v1/mcp", wantErr: true},
		{name: "service writer may read", principal: writer, method: http.MethodGet, target: "/api/v1/mcp"},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(tc.method, tc.target, nil)
			err := AuthorizeHTTP(tc.principal, req)
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeHTTP() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}
}

func TestAuthorizeHTTPRequiresExplicitNonZDRPostureForDurableMutations(t *testing.T) {
	writer := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "capability-writer",
		PrincipalType:          "service",
		Scopes:                 []string{WriteScope},
		RetentionPolicyPresent: true,
	}
	zdrWriter := writer
	zdrWriter.ZeroDataRetention = true
	unspecifiedWriter := writer
	unspecifiedWriter.RetentionPolicyPresent = false

	for _, target := range []string{
		"/api/v1/memory",
		"/api/v1/mcp",
		"/api/v1/skills",
		"/api/v1/plugins",
		"/api/v1/tasks",
		"/api/v1/cron",
		"/api/v1/capabilities",
		"/api/v1/capabilities/scopes/grant",
		"/api/v1/routing",
		"/api/v1/safety",
	} {
		t.Run(target, func(t *testing.T) {
			request := httptest.NewRequest(http.MethodPost, target, nil)
			if err := AuthorizeHTTP(writer, request); err != nil {
				t.Fatalf("explicit non-ZDR writer denied: %v", err)
			}
			if err := AuthorizeHTTP(zdrWriter, request); err == nil {
				t.Fatal("issuer-ZDR writer was authorized to persist durable state")
			}
			if err := AuthorizeHTTP(unspecifiedWriter, request); err == nil {
				t.Fatal("writer without a verified retention posture was authorized to persist durable state")
			}
		})
	}

	readRequest := httptest.NewRequest(http.MethodGet, "/api/v1/memory", nil)
	if err := AuthorizeHTTP(zdrWriter, readRequest); err != nil {
		t.Fatalf("ZDR read was denied: %v", err)
	}

	healthWriter := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "health-writer",
		PrincipalType:          "service",
		Scopes:                 []string{HealthWriteScope},
		RetentionPolicyPresent: true,
	}
	if err := AuthorizeHTTP(healthWriter, httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/availability", nil)); err != nil {
		t.Fatalf("explicit non-ZDR health writer denied: %v", err)
	}
	healthWriter.ZeroDataRetention = true
	if err := AuthorizeHTTP(healthWriter, httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/availability", nil)); err == nil {
		t.Fatal("issuer-ZDR health writer was authorized to mutate durable availability")
	}
}

func TestAuthorizeGRPCPinsEvaluatePolicyAndProtectsPromotion(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	writer := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service", Scopes: []string{"capability:write"}}
	reader := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-reader", PrincipalType: "service", Scopes: []string{"capability:read"}}
	globalWriter := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-admin", PrincipalType: "service", Scopes: []string{"capability:global:write"}, RetentionPolicyPresent: true}

	if err := AuthorizeGRPC(user, mpv1.CapabilityCore_EvaluatePolicy_FullMethodName, &mpv1.EvaluatePolicyRequest{OrgId: "org-b"}); err == nil {
		t.Fatal("wrong-tenant EvaluatePolicy request was authorized")
	}
	if err := AuthorizeGRPC(user, mpv1.CapabilityCore_EvaluatePolicy_FullMethodName, &mpv1.EvaluatePolicyRequest{}); err == nil {
		t.Fatal("direct user policy evaluation may forge agent/run identity")
	}
	if err := AuthorizeGRPC(reader, mpv1.CapabilityCore_EvaluatePolicy_FullMethodName, &mpv1.EvaluatePolicyRequest{}); err != nil {
		t.Fatalf("empty org should be derived for trusted service: %v", err)
	}
	if err := AuthorizeGRPC(user, mpv1.CapabilityCore_PromoteSkill_FullMethodName, &mpv1.PromoteSkillRequest{}); err == nil {
		t.Fatal("unscoped user promotion was authorized")
	}
	if err := AuthorizeGRPC(writer, mpv1.CapabilityCore_PromoteSkill_FullMethodName, &mpv1.PromoteSkillRequest{}); err == nil {
		t.Fatal("tenant writer was allowed to mutate the global skill registry")
	}
	if err := AuthorizeGRPC(globalWriter, mpv1.CapabilityCore_PromoteSkill_FullMethodName, &mpv1.PromoteSkillRequest{}); err != nil {
		t.Fatalf("global promotion service was denied: %v", err)
	}
	zdrGlobalWriter := globalWriter
	zdrGlobalWriter.ZeroDataRetention = true
	if err := AuthorizeGRPC(zdrGlobalWriter, mpv1.CapabilityCore_PromoteSkill_FullMethodName, &mpv1.PromoteSkillRequest{}); err == nil {
		t.Fatal("issuer-ZDR service was authorized to mutate the global skill registry")
	}
	unspecifiedRetentionGlobalWriter := globalWriter
	unspecifiedRetentionGlobalWriter.RetentionPolicyPresent = false
	if err := AuthorizeGRPC(unspecifiedRetentionGlobalWriter, mpv1.CapabilityCore_PromoteSkill_FullMethodName, &mpv1.PromoteSkillRequest{}); err == nil {
		t.Fatal("global skill promotion was authorized without a verified retention posture")
	}
	if err := AuthorizeGRPC(authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service"}, mpv1.CapabilityCore_ListCapabilities_FullMethodName, &mpv1.ListCapabilitiesRequest{}); err == nil {
		t.Fatal("unscoped service read was authorized")
	}
}
