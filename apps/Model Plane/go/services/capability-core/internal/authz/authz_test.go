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
	globalTenantHealthWriter := authctx.Principal{OrganizationID: "global", ActorID: "health-global-tenant", PrincipalType: "service", Scopes: []string{HealthWriteScope}, RetentionPolicyPresent: true}
	globalHealthWriter := authctx.Principal{OrganizationID: "global", ActorID: ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{GlobalHealthWriteScope}, RetentionPolicyPresent: true}
	tenantGlobalHealthWriter := authctx.Principal{OrganizationID: "org-a", ActorID: ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{GlobalHealthWriteScope}, RetentionPolicyPresent: true}
	browserHealthWriter := authctx.Principal{OrganizationID: "global", ActorID: BrowserBrokerServiceID, PrincipalType: "service", Scopes: []string{BrowserHealthWriteScope, ReadScope}, RetentionPolicyPresent: true}

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
		{name: "tenant health workload may not claim global organization", principal: globalTenantHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "global health workload may enter health route", principal: globalHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability"},
		{name: "global health workload bound to tenant may not enter health route", principal: tenantGlobalHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "unrelated global health workload may not enter health route", principal: authctx.Principal{OrganizationID: "global", ActorID: "service:unrelated", PrincipalType: "service", Scopes: []string{GlobalHealthWriteScope}, RetentionPolicyPresent: true}, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "ordinary writer may not cancel Space schedules", principal: writer, method: http.MethodPost, target: "/api/v1/internal/space-deletion/cron", wantErr: true},
		{name: "exact deletion scope may cancel Space schedules", principal: authctx.Principal{OrganizationID: "org-a", ActorID: "service:control-space-deletion", PrincipalType: "service", Scopes: []string{SpaceDeletionScope}, RetentionPolicyPresent: true}, method: http.MethodPost, target: "/api/v1/internal/space-deletion/cron"},
		{name: "tenant-agnostic command execution is quarantined", principal: writer, method: http.MethodPost, target: "/api/v1/commands/exec", wantErr: true},
		{name: "service read needs scope", principal: authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service"}, method: http.MethodGet, target: "/api/v1/mcp", wantErr: true},
		{name: "service writer may read", principal: writer, method: http.MethodGet, target: "/api/v1/mcp"},
		{name: "browser-broker may attest browser health", principal: browserHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/browser/health"},
		{name: "browser health route is POST only", principal: browserHealthWriter, method: http.MethodPut, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "browser-broker bound to a tenant may not attest browser health", principal: func() authctx.Principal { p := browserHealthWriter; p.OrganizationID = "org-a"; return p }(), method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "unprefixed broker name may not attest browser health", principal: func() authctx.Principal { p := browserHealthWriter; p.ActorID = "browser-broker"; return p }(), method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "unrelated service with the browser scope may not attest browser health", principal: func() authctx.Principal { p := browserHealthWriter; p.ActorID = "service:unrelated"; return p }(), method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "browser-broker without its scope may not attest browser health", principal: authctx.Principal{OrganizationID: "global", ActorID: BrowserBrokerServiceID, PrincipalType: "service", Scopes: []string{HealthWriteScope, WriteScope}, RetentionPolicyPresent: true}, method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "user holding the browser scope may not attest browser health", principal: func() authctx.Principal { p := browserHealthWriter; p.PrincipalType = "user"; return p }(), method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "global execution health workload may not attest browser health", principal: globalHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/browser/health", wantErr: true},
		{name: "browser-broker scope opens no generic health route", principal: browserHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/availability", wantErr: true},
		{name: "browser-broker scope opens no owner-action route", principal: browserHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/owner-actions/health", wantErr: true},
		{name: "browser-broker scope is no catalog write", principal: browserHealthWriter, method: http.MethodPost, target: "/api/v1/capabilities/upsert", wantErr: true},
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

	browserHealthWriter := authctx.Principal{
		OrganizationID:         "global",
		ActorID:                BrowserBrokerServiceID,
		PrincipalType:          "service",
		Scopes:                 []string{BrowserHealthWriteScope},
		RetentionPolicyPresent: true,
	}
	browserHealth := func() *http.Request {
		return httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/browser/health", nil)
	}
	if err := AuthorizeHTTP(browserHealthWriter, browserHealth()); err != nil {
		t.Fatalf("explicit non-ZDR browser health writer denied: %v", err)
	}
	unspecifiedBrowserHealthWriter := browserHealthWriter
	unspecifiedBrowserHealthWriter.RetentionPolicyPresent = false
	if err := AuthorizeHTTP(unspecifiedBrowserHealthWriter, browserHealth()); err == nil {
		t.Fatal("browser health writer without a verified retention posture was authorized")
	}
	browserHealthWriter.ZeroDataRetention = true
	if err := AuthorizeHTTP(browserHealthWriter, browserHealth()); err == nil {
		t.Fatal("issuer-ZDR browser health writer was authorized to mutate durable availability")
	}
}

// The user a run acts for is asserted by the caller, so only the execution-core
// workload may assert one and a person may only name themselves. A forged
// assertion would let a later per-user policy be borrowed.
func TestAuthorizeGRPCPinsAssertedUserAndToolBindingReads(t *testing.T) {
	execution := authctx.Principal{OrganizationID: "org-a", ActorID: ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{ReadScope}}
	otherService := authctx.Principal{OrganizationID: "org-a", ActorID: "service:other", PrincipalType: "service", Scopes: []string{ReadScope}}
	unscoped := authctx.Principal{OrganizationID: "org-a", ActorID: ExecutionCoreServiceID, PrincipalType: "service"}
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	resolve := mpv1.CapabilityCore_ResolveToolBindings_FullMethodName
	evaluate := mpv1.CapabilityCore_EvaluatePolicy_FullMethodName

	tests := []struct {
		name      string
		principal authctx.Principal
		method    string
		request   any
		wantErr   bool
	}{
		{name: "execution-core may assert the user on a policy evaluation", principal: execution, method: evaluate, request: &mpv1.EvaluatePolicyRequest{UserId: "user-a", ToolName: "web_fetch"}},
		{name: "execution-core may evaluate without a user", principal: execution, method: evaluate, request: &mpv1.EvaluatePolicyRequest{ToolName: "web_fetch"}},
		{name: "another service may not assert a user on a policy evaluation", principal: otherService, method: evaluate, request: &mpv1.EvaluatePolicyRequest{UserId: "user-a"}, wantErr: true},
		{name: "another service may name a tool without a user", principal: otherService, method: evaluate, request: &mpv1.EvaluatePolicyRequest{ToolName: "web_fetch"}},
		{name: "execution-core may resolve bindings for a user", principal: execution, method: resolve, request: &mpv1.ResolveToolBindingsRequest{UserId: "user-b", ToolNames: []string{"shell"}}},
		{name: "another service may not assert a user when resolving", principal: otherService, method: resolve, request: &mpv1.ResolveToolBindingsRequest{UserId: "user-a"}, wantErr: true},
		{name: "another reading service may resolve without a user", principal: otherService, method: resolve, request: &mpv1.ResolveToolBindingsRequest{ToolNames: []string{"shell"}}},
		{name: "a service without a read scope may not resolve", principal: unscoped, method: resolve, request: &mpv1.ResolveToolBindingsRequest{}, wantErr: true},
		{name: "a person may resolve for themselves", principal: user, method: resolve, request: &mpv1.ResolveToolBindingsRequest{UserId: "user-a"}},
		{name: "a person may resolve without naming a user", principal: user, method: resolve, request: &mpv1.ResolveToolBindingsRequest{}},
		{name: "a person may not resolve for someone else", principal: user, method: resolve, request: &mpv1.ResolveToolBindingsRequest{UserId: "user-b"}, wantErr: true},
		{name: "a wrong-tenant resolution is denied", principal: execution, method: resolve, request: &mpv1.ResolveToolBindingsRequest{OrgId: "org-b"}, wantErr: true},
		{name: "a matching tenant resolution is allowed", principal: execution, method: resolve, request: &mpv1.ResolveToolBindingsRequest{OrgId: "org-a"}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := AuthorizeGRPC(tc.principal, tc.method, tc.request)
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeGRPC() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
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

// Tool policies (WS05-S5) are durable state: a person may read the policies in
// force for them, but writing or revoking one needs the signed write scope and an
// explicit non-ZDR posture, exactly as any other registry mutation. Whether the
// writer is a person and, for an org policy, an admin is the handler's check on
// the signed principal; these pins keep the route from ever being quarantined
// into a read or opened to a ZDR principal.
func TestAuthorizeHTTPPinsToolPolicyRoutes(t *testing.T) {
	person := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", Scopes: []string{WriteScope}, RetentionPolicyPresent: true}
	zdrPerson := person
	zdrPerson.ZeroDataRetention = true
	unstatedPerson := person
	unstatedPerson.RetentionPolicyPresent = false
	readOnlyPerson := person
	readOnlyPerson.Scopes = nil
	readOnlyZDRPerson := readOnlyPerson
	readOnlyZDRPerson.ZeroDataRetention = true

	tests := []struct {
		name      string
		principal authctx.Principal
		method    string
		target    string
		wantErr   bool
	}{
		{name: "a person lists the policies in force", principal: readOnlyPerson, method: http.MethodGet, target: "/api/v1/tool-policies?capability_id=cap.mcp_tool.x"},
		{name: "a ZDR person may still read them", principal: readOnlyZDRPerson, method: http.MethodGet, target: "/api/v1/tool-policies"},
		{name: "a forged org on the list is denied", principal: readOnlyPerson, method: http.MethodGet, target: "/api/v1/tool-policies?org_id=org-b", wantErr: true},
		{name: "a forged user on the list is denied", principal: readOnlyPerson, method: http.MethodGet, target: "/api/v1/tool-policies?user_id=user-b", wantErr: true},
		{name: "a writer sets a policy", principal: person, method: http.MethodPut, target: "/api/v1/tool-policies/cap.mcp_tool.x"},
		{name: "a writer revokes a policy", principal: person, method: http.MethodDelete, target: "/api/v1/tool-policies/cap.mcp_tool.x?scope=org"},
		{name: "a person without the write scope may not set one", principal: readOnlyPerson, method: http.MethodPut, target: "/api/v1/tool-policies/cap.mcp_tool.x", wantErr: true},
		{name: "a ZDR person may not set one", principal: zdrPerson, method: http.MethodPut, target: "/api/v1/tool-policies/cap.mcp_tool.x", wantErr: true},
		{name: "a ZDR person may not revoke one", principal: zdrPerson, method: http.MethodDelete, target: "/api/v1/tool-policies/cap.mcp_tool.x", wantErr: true},
		{name: "an unstated posture may not set one", principal: unstatedPerson, method: http.MethodPut, target: "/api/v1/tool-policies/cap.mcp_tool.x", wantErr: true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := AuthorizeHTTP(tc.principal, httptest.NewRequest(tc.method, tc.target, nil))
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeHTTP() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}
}

func TestAuthorizeHTTPPinsOrgAgentPolicyRoutes(t *testing.T) {
	member := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true}
	admin := authctx.Principal{OrganizationID: "org-a", ActorID: "user-admin", PrincipalType: "user", Scopes: []string{WriteScope, "org:admin"}, RetentionPolicyPresent: true}
	zdrAdmin := admin
	zdrAdmin.ZeroDataRetention = true

	tests := []struct {
		name      string
		principal authctx.Principal
		method    string
		target    string
		wantErr   bool
	}{
		{name: "a member reads the org policy", principal: member, method: http.MethodGet, target: "/api/v1/org-agent-policy"},
		{name: "a member reads the effective policy", principal: member, method: http.MethodGet, target: "/api/v1/org-agent-policy/effective?user=me"},
		{name: "a ZDR admin may still read the policy", principal: zdrAdmin, method: http.MethodGet, target: "/api/v1/org-agent-policy/history"},
		{name: "a forged org on the read is denied", principal: member, method: http.MethodGet, target: "/api/v1/org-agent-policy?org_id=org-b", wantErr: true},
		{name: "an admin writes the policy", principal: admin, method: http.MethodPut, target: "/api/v1/org-agent-policy"},
		{name: "a ZDR admin may not write it", principal: zdrAdmin, method: http.MethodPut, target: "/api/v1/org-agent-policy", wantErr: true},
		{name: "a member without the write scope may not write it", principal: member, method: http.MethodPut, target: "/api/v1/org-agent-policy", wantErr: true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := AuthorizeHTTP(tc.principal, httptest.NewRequest(tc.method, tc.target, nil))
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeHTTP() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}
}

func TestBundleReadRuleSwitchIsTheLiteralTrueOnly(t *testing.T) {
	for _, raw := range []string{"", "1", "TRUE", " true", "yes"} {
		if BundleReadRuleFromLookup(func(string) string { return raw }) {
			t.Errorf("%q enabled the bundle read rule", raw)
		}
	}
	if !BundleReadRuleFromLookup(func(key string) string {
		if key == BundleReadRuleEnv {
			return "true"
		}
		return ""
	}) {
		t.Fatal(`"true" did not enable the bundle read rule`)
	}
}

func TestAUserPrincipalCannotReadABlob(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	scopedUser := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", Scopes: []string{ReadScope, WriteScope}}
	serviceReader := authctx.Principal{OrganizationID: "org-a", ActorID: ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{ReadScope}}
	serviceWriter := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service", Scopes: []string{WriteScope}}
	unscopedService := authctx.Principal{OrganizationID: "org-a", ActorID: "svc-a", PrincipalType: "service"}
	blob := mpv1.CapabilityCore_GetSkillBlob_FullMethodName
	list := mpv1.CapabilityCore_ListSkillBundles_FullMethodName
	manifest := mpv1.CapabilityCore_GetSkillBundleManifest_FullMethodName

	tests := []struct {
		name      string
		principal authctx.Principal
		method    string
		wantErr   bool
	}{
		{name: "a person may not fetch a bundle body", principal: user, method: blob, wantErr: true},
		{name: "a person's scopes do not open a bundle body", principal: scopedUser, method: blob, wantErr: true},
		{name: "a service with the read scope fetches a body", principal: serviceReader, method: blob},
		{name: "a write scope alone is no read grant", principal: serviceWriter, method: blob, wantErr: true},
		{name: "a service without a scope may not fetch a body", principal: unscopedService, method: blob, wantErr: true},
		{name: "a person may list the org's bundles", principal: user, method: list},
		{name: "a person may read a bundle manifest", principal: user, method: manifest},
		{name: "a service with the read scope may list", principal: serviceReader, method: list},
		{name: "a service writer may not list", principal: serviceWriter, method: list, wantErr: true},
		{name: "a service without a scope may not read a manifest", principal: unscopedService, method: manifest, wantErr: true},
		{name: "an unsigned principal is denied", principal: authctx.Principal{PrincipalType: "service", Scopes: []string{ReadScope}}, method: blob, wantErr: true},
	}
	rule := AuthorizeGRPCWith(true)
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := rule(tc.principal, tc.method, nil)
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeGRPCWith(true) error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}

	// Off, the bundle RPCs keep the generic read rule: a person may still fetch a body.
	if err := AuthorizeGRPCWith(false)(user, blob, nil); err != nil {
		t.Fatalf("rule off must not change the generic read rule: %v", err)
	}
	// Other methods are untouched by the rule.
	if err := rule(user, mpv1.CapabilityCore_ListCapabilities_FullMethodName, nil); err != nil {
		t.Fatalf("rule on must leave other reads alone: %v", err)
	}
}
