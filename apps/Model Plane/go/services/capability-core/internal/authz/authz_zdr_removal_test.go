package authz

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// W3-DEC-ZDRDEL3 (owner decision D20): a zero-retention principal may make exactly the six removals, and no other durable
// write. An unstated posture is refused for every write, the removals included.
func TestAuthorizeHTTPZDRRemovals(t *testing.T) {
	zdrUser := authctx.Principal{
		OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user",
		Scopes: []string{WriteScope}, RetentionPolicyPresent: true, ZeroDataRetention: true,
	}
	zdrService := zdrUser
	zdrService.PrincipalType = "service"
	zdrService.ActorID = "capability-writer"
	unstated := zdrUser
	unstated.ZeroDataRetention = false
	unstated.RetentionPolicyPresent = false

	admitted := []struct{ method, target string }{
		{http.MethodDelete, "/api/v1/cron/x"},
		{http.MethodDelete, "/api/v1/skills/x"},
		{http.MethodDelete, "/api/v1/skills/x/draft"},
		{http.MethodDelete, "/api/v1/plugins/x"},
		{http.MethodPost, "/api/v1/plugins/x/uninstall"},
		{http.MethodDelete, "/api/v1/plugin-kill-list"},
	}
	refused := []struct{ method, target string }{
		{http.MethodDelete, "/api/v1/org-hooks/x"},
		{http.MethodDelete, "/api/v1/org-permission-rules/x"},
		{http.MethodDelete, "/api/v1/tool-policies/x"},
		{http.MethodDelete, "/api/v1/skills/x/bundle"},
		{http.MethodDelete, "/api/v1/mcp/x"},
		{http.MethodPost, "/api/v1/cron/x"},
		{http.MethodPost, "/api/v1/skills/x/draft"},
		{http.MethodPatch, "/api/v1/cron/x"},
		{http.MethodDelete, "/api/v1/plugins/x/components"},
		{http.MethodDelete, "/api/v1/cron/x%2Fy"},
		{http.MethodDelete, "/api/v1/cron/"},
		{http.MethodDelete, "/api/v1/cron/.."},
	}

	for _, principal := range []struct {
		name string
		p    authctx.Principal
	}{{"zero-retention user", zdrUser}, {"zero-retention service", zdrService}} {
		for _, row := range admitted {
			if err := AuthorizeHTTP(principal.p, httptest.NewRequest(row.method, row.target, nil)); err != nil {
				t.Errorf("%s: %s %s denied: %v", principal.name, row.method, row.target, err)
			}
		}
		for _, row := range refused {
			if err := AuthorizeHTTP(principal.p, httptest.NewRequest(row.method, row.target, nil)); err == nil {
				t.Errorf("%s: %s %s admitted under zero retention", principal.name, row.method, row.target)
			}
		}
	}
	for _, row := range append(append([]struct{ method, target string }{}, admitted...), refused...) {
		if err := AuthorizeHTTP(unstated, httptest.NewRequest(row.method, row.target, nil)); err == nil {
			t.Errorf("unstated posture: %s %s admitted", row.method, row.target)
		}
	}
}

// WS18-S2: the owner-action health route admits exactly the two owner workloads, conversation-core and execution-core,
// in the global organization and holding the owner-action scope. A tenant principal, the browser broker and any other
// service are refused.
func TestAuthorizeHTTPOwnerActionHealthRouteAdmitsOnlyTheTwoGlobalOwners(t *testing.T) {
	scopes := []string{OwnerActionHealthWriteScope}
	conversation := authctx.Principal{
		OrganizationID: "global", ActorID: ConversationCoreServiceID, PrincipalType: "service",
		Scopes: scopes, RetentionPolicyPresent: true,
	}
	execution := conversation
	execution.ActorID = ExecutionCoreServiceID
	tenantExecution := execution
	tenantExecution.OrganizationID = "org-a"
	tenantUser := authctx.Principal{
		OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user",
		Scopes: scopes, RetentionPolicyPresent: true,
	}
	browser := conversation
	browser.ActorID = BrowserBrokerServiceID
	browser.Scopes = []string{OwnerActionHealthWriteScope, BrowserHealthWriteScope}
	unrelated := conversation
	unrelated.ActorID = "service:unrelated"
	wikiStore := conversation
	wikiStore.ActorID = WikiStoreServiceID

	tests := []struct {
		name      string
		principal authctx.Principal
		wantErr   bool
	}{
		{"conversation-core in global may attest its ticket", conversation, false},
		{"execution-core in global may attest the Model-owned Space rows", execution, false},
		{"execution-core bound to a tenant may not attest", tenantExecution, true},
		{"a tenant user holding the scope may not attest", tenantUser, true},
		{"browser-broker holding the owner scope may not attest", browser, true},
		{"an unrelated global service may not attest", unrelated, true},
		// P-CFG: the room canvas owner; the attester table still limits it to the two canvas rows.
		{"wiki-store in global may attest the room canvas rows", wikiStore, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := AuthorizeHTTP(tc.principal, httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/owner-actions/health", nil))
			if (err != nil) != tc.wantErr {
				t.Fatalf("AuthorizeHTTP() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}
}
