package api

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// orgHookFakeDB fails the test on any database call. Every case in this file is refused before the rule
// store is touched, so a call here is the very defect a case exists to catch.
type orgHookFakeDB struct {
	t *testing.T
}

func (d orgHookFakeDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Helper()
	d.t.Fatal("the rule store was written for a request that had to be refused")
	return pgconn.CommandTag{}, nil
}

func (d orgHookFakeDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Helper()
	d.t.Fatal("the rule store was listed for a request that had to be refused")
	return nil, nil
}

func (d orgHookFakeDB) QueryRow(context.Context, string, ...any) pgx.Row {
	d.t.Helper()
	d.t.Fatal("the rule store was read or written for a request that had to be refused")
	return nil
}

func serveOrgHooks(t *testing.T, db registryDatabase, principal *authctx.Principal, method, target, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewOrgHooksHandler(db).Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

const validOrgHookBody = `{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"no shell for this org","priority":10}`

func TestOrgHookWritesAreRefusedBeforeTheStore(t *testing.T) {
	admin := authctx.Principal{OrganizationID: "org-a", ActorID: "user-admin", PrincipalType: "user", Scopes: []string{authz.WriteScope, "org:admin"}, RetentionPolicyPresent: true}
	member := authctx.Principal{OrganizationID: "org-a", ActorID: "user-m", PrincipalType: "user", RetentionPolicyPresent: true}
	writerOnly := authctx.Principal{OrganizationID: "org-a", ActorID: "user-w", PrincipalType: "user", Scopes: []string{authz.WriteScope}, RetentionPolicyPresent: true}
	zdrAdmin := admin
	zdrAdmin.ZeroDataRetention = true
	noRetention := admin
	noRetention.RetentionPolicyPresent = false
	service := admin
	service.PrincipalType = "service"

	tests := []struct {
		name      string
		principal *authctx.Principal
		method    string
		target    string
		body      string
		status    int
		code      string
	}{
		{name: "an unauthenticated write", principal: nil, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusUnauthorized, code: "authentication required"},
		{name: "a member writes", principal: &member, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusForbidden, code: "capability write scope required"},
		{name: "a writer without the admin scope", principal: &writerOnly, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusForbidden, code: "only an admin may write org hook rules"},
		{name: "a ZDR admin writes", principal: &zdrAdmin, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusForbidden, code: "org_hook_write_under_zdr_refused"},
		{name: "a principal with no retention posture writes", principal: &noRetention, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusForbidden, code: "org_hook_write_under_zdr_refused"},
		{name: "a service identity writes", principal: &service, method: http.MethodPost, target: orgHooksPath, body: validOrgHookBody, status: http.StatusForbidden, code: "only a person may write org hook rules"},
		{name: "a member updates a rule", principal: &member, method: http.MethodPut, target: orgHooksPath + "/ohr_1", body: validOrgHookBody, status: http.StatusForbidden, code: "capability write scope required"},
		{name: "a member revokes a rule", principal: &member, method: http.MethodDelete, target: orgHooksPath + "/ohr_1", status: http.StatusForbidden, code: "capability write scope required"},
		{name: "an unknown event", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"on_error","tool_matcher":"shell","decision":"deny","reason":"x"}`, status: http.StatusBadRequest, code: "invalid_event"},
		{name: "an allow rule", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"pre_tool","tool_matcher":"shell","decision":"allow","reason":"x"}`, status: http.StatusBadRequest, code: "invalid_decision"},
		{name: "a post-tool ask", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"post_tool","tool_matcher":"shell","decision":"ask","reason":"x"}`, status: http.StatusUnprocessableEntity, code: "post_tool_must_deny"},
		{name: "a wildcard in the middle of a matcher", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"pre_tool","tool_matcher":"sh*ll","decision":"deny","reason":"x"}`, status: http.StatusBadRequest, code: "invalid_matcher"},
		{name: "a rule with no reason", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"   "}`, status: http.StatusBadRequest, code: "invalid_reason"},
		{name: "a field the rule does not have", principal: &admin, method: http.MethodPost, target: orgHooksPath, body: `{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"x","matcher":"shell"}`, status: http.StatusBadRequest, code: "invalid request body"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			response := serveOrgHooks(t, orgHookFakeDB{t: t}, tc.principal, tc.method, tc.target, tc.body)
			if response.Code != tc.status || !strings.Contains(response.Body.String(), tc.code) {
				t.Fatalf("status %d body %s, want %d mentioning %q", response.Code, response.Body.String(), tc.status, tc.code)
			}
		})
	}
}

func TestOrgHookMatcherFollowsExecutionCoreGrammar(t *testing.T) {
	stored := map[string]string{
		"":          "*",
		"*":         "*",
		"  shell  ": "shell",
		"shell":     "shell",
		"shell*":    "shell*",
		"browser.*": "browser.*",
		"mcp:srv.t": "mcp:srv.t",
	}
	for raw, want := range stored {
		got, ok := normaliseOrgHookMatcher(raw)
		if !ok || got != want {
			t.Fatalf("matcher %q stored as %q (ok %v), want %q", raw, got, ok, want)
		}
	}
	for _, raw := range []string{"sh*ll", "*shell", "shell**", "sh ell", "-shell", "shell\n*x", "a/b"} {
		if got, ok := normaliseOrgHookMatcher(raw); ok {
			t.Fatalf("matcher %q accepted as %q, want refused", raw, got)
		}
	}
}

func TestOrgHookRuleValidationStoresOnlyDenyAndAsk(t *testing.T) {
	disabled := false
	rule, refusal := validateOrgHookRule(orgHookRuleBody{Event: "pre_tool", ToolMatcher: " shell ", Decision: " DENY ", Reason: "  keep out  ", Priority: 7, Enabled: &disabled})
	if refusal != nil {
		t.Fatalf("a valid rule was refused: %s", refusal.code)
	}
	want := orgHookRule{event: "pre_tool", matcher: "shell", decision: "deny", reason: "keep out", priority: 7, enabled: false}
	if rule != want {
		t.Fatalf("rule %+v, want %+v", rule, want)
	}

	rule, refusal = validateOrgHookRule(orgHookRuleBody{Event: "pre_tool", Decision: "ask", Reason: "check first"})
	if refusal != nil || !rule.enabled || rule.matcher != "*" {
		t.Fatalf("an omitted enabled and matcher must mean an enabled rule for any tool, got %+v (refusal %v)", rule, refusal)
	}

	if _, refusal := validateOrgHookRule(orgHookRuleBody{Event: "post_tool", Decision: "deny", Reason: "withhold the result"}); refusal != nil {
		t.Fatalf("a post-tool deny is the storable post-tool rule, refused as %s", refusal.code)
	}

	atLimit := strings.Repeat("é", maxOrgHookReasonRunes)
	if _, refusal := validateOrgHookRule(orgHookRuleBody{Event: "pre_tool", Decision: "ask", Reason: atLimit}); refusal != nil {
		t.Fatalf("a reason of %d characters was refused as %s", maxOrgHookReasonRunes, refusal.code)
	}
	if _, refusal := validateOrgHookRule(orgHookRuleBody{Event: "pre_tool", Decision: "ask", Reason: atLimit + "é"}); refusal == nil || refusal.code != "invalid_reason" {
		t.Fatalf("a reason over %d characters was not refused as invalid_reason", maxOrgHookReasonRunes)
	}
}

// The audit row is content-free: the operation, the event, the decision and the enabled state, never the
// matcher or the admin's reason.
func TestOrgHookAuditDiffHoldsNoMatcherOrReason(t *testing.T) {
	rule := orgHookRule{event: "pre_tool", matcher: "shell", decision: "deny", reason: "a private reason", priority: 3, enabled: true}
	diff, err := orgHookAuditDiff("create", &rule)
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"decision":"deny","enabled":true,"event":"pre_tool","op":"create"}`; diff != want {
		t.Fatalf("audit diff %s, want %s", diff, want)
	}
	revoke, err := orgHookAuditDiff("revoke", nil)
	if err != nil || revoke != `{"op":"revoke"}` {
		t.Fatalf("revoke audit diff %s (err %v), want {\"op\":\"revoke\"}", revoke, err)
	}
}

// Every rule write moves the policy revision and appends its history row in the statement that changes the
// rule. A rule the gateway caches under a revision that does not hold it would be served as current.
func TestOrgHookWritesBumpThePolicyRevisionInTheSameStatement(t *testing.T) {
	statements := map[string]string{"create": orgHookCreateSQL, "update": orgHookUpdateSQL, "revoke": orgHookRevokeSQL}
	for name, statement := range statements {
		for _, want := range []string{
			"revision = org_agent_policy.revision + 1",
			"INSERT INTO org_agent_policy_history",
			"INSERT INTO registry_audit_log",
			"FROM changed",
		} {
			if !strings.Contains(statement, want) {
				t.Fatalf("the %s write does not contain %q", name, want)
			}
		}
	}
}
