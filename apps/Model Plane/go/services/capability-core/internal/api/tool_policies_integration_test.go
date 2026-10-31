//go:build integration

package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

type policyCaller struct {
	org, actor string
	scopes     []string
	zdr        bool
}

func (c policyCaller) principal() authctx.Principal {
	return authctx.Principal{
		OrganizationID: c.org, ActorID: c.actor, PrincipalType: "user", Scopes: c.scopes,
		RetentionPolicyPresent: true, ZeroDataRetention: c.zdr,
	}
}

func callToolPolicies(t *testing.T, pool *pgxpool.Pool, caller policyCaller, method, target, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewToolPoliciesHandler(pool).Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), caller.principal()))
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

func policyAudit(t *testing.T, pool *pgxpool.Pool, policyID, action string) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), `
		SELECT count(*) FROM registry_audit_log WHERE entity_kind = 'tool_policy' AND entity_id = $1 AND action = $2
	`, policyID, action).Scan(&n); err != nil {
		t.Fatalf("count policy audit: %v", err)
	}
	return n
}

func policyID(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	var reply struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &reply); err != nil || reply.ID == "" {
		t.Fatalf("no policy id in %s", response.Body.String())
	}
	return reply.ID
}

func allowJSON(digest string) string {
	return `{"decision":"allow","pinned_digest":"` + digest + `"}`
}

// Setting, tightening, relaxing and revoking policies against the real SQL: what
// the API refuses, what is audited, and that the engine reads what was stored.
func TestToolPoliciesAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	ctx := context.Background()
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read","create_item","drop_all"]}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)
	integrationObserve(t, mcp, "org-a",
		observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
		observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, ""),
		observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
	)
	echo := readToolRow(t, pool, "org-a", "echo_read")
	create := readToolRow(t, pool, "org-a", "create_item")
	drop := readToolRow(t, pool, "org-a", "drop_all")
	echoPath := "/api/v1/tool-policies/" + echo.capabilityID

	alice := policyCaller{org: "org-a", actor: "user-alice", scopes: []string{authz.WriteScope}}
	bob := policyCaller{org: "org-a", actor: "user-bob", scopes: []string{authz.WriteScope}}
	admin := policyCaller{org: "org-a", actor: "user-admin", scopes: []string{authz.WriteScope, "org:admin"}}
	source := NewToolPoliciesHandler(pool)

	t.Run("generated column marks exactly the mcp_tool rows relaxable", func(t *testing.T) {
		var mcpRelaxable, otherRelaxable int
		if err := pool.QueryRow(ctx, `SELECT count(*) FILTER (WHERE kind = 'mcp_tool' AND policy_relaxable), count(*) FILTER (WHERE kind <> 'mcp_tool' AND policy_relaxable) FROM capabilities`).Scan(&mcpRelaxable, &otherRelaxable); err != nil {
			t.Fatal(err)
		}
		if mcpRelaxable != 3 || otherRelaxable != 0 {
			t.Fatalf("relaxable mcp_tool rows = %d (want 3), other rows = %d (want 0)", mcpRelaxable, otherRelaxable)
		}
	})

	t.Run("the migration re-applies cleanly", func(t *testing.T) {
		sqlBytes, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0017_tool_policies.up.sql"))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
			t.Fatalf("second apply: %v", err)
		}
	})

	var aliceAllowID string
	t.Run("a person relaxes a pinned MCP tool for themselves, and setting it again keeps the id", func(t *testing.T) {
		response := callToolPolicies(t, pool, alice, http.MethodPut, echoPath, allowJSON(knownAnswerToolDigest))
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		aliceAllowID = policyID(t, response)
		if policyAudit(t, pool, aliceAllowID, auditActionToolPolicySet) != 1 {
			t.Fatal("the write was not audited")
		}
		again := callToolPolicies(t, pool, alice, http.MethodPut, echoPath, `{"decision":"ask","reason":"changed my mind"}`)
		if again.Code != http.StatusOK || policyID(t, again) != aliceAllowID {
			t.Fatalf("a second set did not update in place: %d %s", again.Code, again.Body.String())
		}
		if policyAudit(t, pool, aliceAllowID, auditActionToolPolicySet) != 2 {
			t.Fatal("the second write was not audited")
		}
		var diff string
		if err := pool.QueryRow(ctx, `SELECT diff_json::text FROM registry_audit_log WHERE entity_id = $1 ORDER BY ts DESC LIMIT 1`, aliceAllowID).Scan(&diff); err != nil {
			t.Fatal(err)
		}
		if strings.Contains(diff, "changed my mind") {
			t.Fatalf("the audit row carries the person's reason: %s", diff)
		}
		if response := callToolPolicies(t, pool, alice, http.MethodPut, echoPath, allowJSON(knownAnswerToolDigest)); response.Code != http.StatusOK {
			t.Fatalf("re-allow status = %d", response.Code)
		}
	})

	t.Run("a policy applies to its person and the org, never to another person", func(t *testing.T) {
		got, err := source.ActiveToolPolicies(ctx, "org-a", "user-alice", echo.capabilityID)
		if err != nil || len(got) != 1 || got[0].Decision != policy.ToolPolicyAllow || got[0].ScopeKind != policy.ToolPolicyScopeUser || got[0].PinnedDigest != knownAnswerToolDigest {
			t.Fatalf("alice's policies = %+v err=%v", got, err)
		}
		if got, err := source.ActiveToolPolicies(ctx, "org-a", "user-bob", echo.capabilityID); err != nil || len(got) != 0 {
			t.Fatalf("bob sees %+v err=%v", got, err)
		}
		if got, err := source.ActiveToolPolicies(ctx, "org-a", "", echo.capabilityID); err != nil || len(got) != 0 {
			t.Fatalf("an unnamed person sees %+v err=%v", got, err)
		}
		if got, err := source.ActiveToolPolicies(ctx, "org-b", "user-alice", echo.capabilityID); err != nil || len(got) != 0 {
			t.Fatalf("another org sees %+v err=%v", got, err)
		}
	})

	t.Run("what an allow may not do", func(t *testing.T) {
		cases := []struct {
			name   string
			caller policyCaller
			path   string
			body   string
			want   int
		}{
			{"a digest other than the pinned one", alice, echoPath, allowJSON(changedToolDigest), http.StatusConflict},
			{"a destructive tool, by a person", alice, "/api/v1/tool-policies/" + create.capabilityID, allowJSON(changedToolDigest), http.StatusUnprocessableEntity},
			{"an unclassified tool, even by an admin for the org", admin, "/api/v1/tool-policies/" + drop.capabilityID, `{"scope":"org","decision":"allow","pinned_digest":"` + anotherChangedToolDigest + `"}`, http.StatusUnprocessableEntity},
			{"a built-in tool", alice, "/api/v1/tool-policies/cap.command.shell", allowJSON(knownAnswerToolDigest), http.StatusUnprocessableEntity},
			{"an unknown capability", alice, "/api/v1/tool-policies/cap.does.not.exist", `{"decision":"ask"}`, http.StatusNotFound},
			{"another org's tool", policyCaller{org: "org-b", actor: "user-x", scopes: []string{authz.WriteScope}}, echoPath, `{"decision":"ask"}`, http.StatusNotFound},
			{"an org policy by a non-admin", alice, echoPath, `{"scope":"org","decision":"never"}`, http.StatusForbidden},
			{"a write under ZDR", policyCaller{org: "org-a", actor: "user-alice", scopes: []string{authz.WriteScope}, zdr: true}, echoPath, `{"decision":"never"}`, http.StatusForbidden},
		}
		for _, tc := range cases {
			response := callToolPolicies(t, pool, tc.caller, http.MethodPut, tc.path, tc.body)
			if response.Code != tc.want {
				t.Errorf("%s: status = %d, want %d, body=%s", tc.name, response.Code, tc.want, response.Body.String())
			}
		}
		var n int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies WHERE capability_id = ANY($1)`, []string{create.capabilityID, drop.capabilityID, "cap.command.shell", "cap.does.not.exist"}).Scan(&n); err != nil || n != 0 {
			t.Fatalf("a refused write left %d rows (err=%v)", n, err)
		}
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies WHERE org_id = 'org-b' OR scope_kind = 'org'`).Scan(&n); err != nil || n != 0 {
			t.Fatalf("a refused write left %d org or foreign rows (err=%v)", n, err)
		}
	})

	t.Run("an admin tightens a built-in tool for the org and relaxes a destructive MCP tool", func(t *testing.T) {
		if response := callToolPolicies(t, pool, admin, http.MethodPut, "/api/v1/tool-policies/cap.command.shell", `{"scope":"org","decision":"never","reason":"no shell for anyone"}`); response.Code != http.StatusOK {
			t.Fatalf("org never on a built-in: %d %s", response.Code, response.Body.String())
		}
		if response := callToolPolicies(t, pool, alice, http.MethodPut, "/api/v1/tool-policies/cap.command.shell", `{"decision":"ask"}`); response.Code != http.StatusOK {
			t.Fatalf("a person's ask on a built-in: %d %s", response.Code, response.Body.String())
		}
		if response := callToolPolicies(t, pool, admin, http.MethodPut, "/api/v1/tool-policies/"+create.capabilityID, `{"scope":"org","decision":"allow","pinned_digest":"`+changedToolDigest+`"}`); response.Code != http.StatusOK {
			t.Fatalf("org allow on a destructive tool: %d %s", response.Code, response.Body.String())
		}
		got, err := source.ActiveToolPolicies(ctx, "org-a", "user-bob", "cap.command.shell")
		if err != nil || len(got) != 1 || got[0].Decision != policy.ToolPolicyNever || got[0].ScopeKind != policy.ToolPolicyScopeOrg {
			t.Fatalf("bob under the org never = %+v err=%v", got, err)
		}
	})

	t.Run("the list shows the org's policies and the caller's own", func(t *testing.T) {
		response := callToolPolicies(t, pool, bob, http.MethodGet, "/api/v1/tool-policies", "")
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d", response.Code)
		}
		var views []toolPolicyView
		if err := json.Unmarshal(response.Body.Bytes(), &views); err != nil {
			t.Fatal(err)
		}
		for _, v := range views {
			if v.ScopeKind == "user" && v.ScopeValue != "user-bob" {
				t.Fatalf("bob was shown %s's policy", v.ScopeValue)
			}
		}
		if len(views) != 2 {
			t.Fatalf("bob sees %d policies, want the 2 org ones: %+v", len(views), views)
		}
		filtered := callToolPolicies(t, pool, alice, http.MethodGet, "/api/v1/tool-policies?capability_id="+echo.capabilityID, "")
		if err := json.Unmarshal(filtered.Body.Bytes(), &views); err != nil || len(views) != 1 || views[0].Decision != policy.ToolPolicyAllow {
			t.Fatalf("filtered list = %+v err=%v", views, err)
		}
	})

	t.Run("the engine reads the stored policies: allow, org never, and a changed definition", func(t *testing.T) {
		capability := &models.Capability{
			ID: echo.capabilityID, Kind: models.KindMCPTool, Version: "1.0.0", RiskLevel: models.RiskHigh,
			EnabledForScopes: []string{policy.ScopeWildcard}, SideEffectClass: models.SideEffectExternalWrite,
		}
		binding := &policy.ToolBinding{
			ToolName: "mcp__srv__echo_read", CapabilityID: echo.capabilityID, SideEffectClass: models.SideEffectExternalWrite,
			ApprovalFloor: models.ApprovalFloorNone, ToolDigest: knownAnswerToolDigest, ReviewState: policy.ReviewStateApproved,
		}
		evaluate := func(enforced bool, user string, binding *policy.ToolBinding) *policy.Result {
			t.Helper()
			result, err := policy.New(nil).WithToolPolicies(source, enforced).EvaluateTool(ctx, capability, binding, user, "run-1", "agent-1", "org-a", "global")
			if err != nil {
				t.Fatal(err)
			}
			return result
		}
		// Alice's current policy on echo_read is the allow she re-set above.
		if got := evaluate(false, "user-alice", binding); got.Decision != policy.DecisionAsk {
			t.Fatalf("with the switch off the stored allow applied: %+v", got)
		}
		if got := evaluate(true, "user-alice", binding); got.Decision != policy.DecisionAllow || got.PolicySource != policy.PolicySourceToolPolicyAllow {
			t.Fatalf("alice's allow: %+v", got)
		}
		if got := evaluate(true, "user-bob", binding); got.Decision != policy.DecisionAsk {
			t.Fatalf("bob has no policy and must still be asked: %+v", got)
		}
		// The definition changes and is re-approved: the pinned digest moves on.
		moved := *binding
		moved.ToolDigest = changedToolDigest
		if got := evaluate(true, "user-alice", &moved); got.Decision != policy.DecisionAsk {
			t.Fatalf("an allow for the old definition still applied: %+v", got)
		}
		// An org admin forbids the tool: it beats alice's allow.
		if response := callToolPolicies(t, pool, admin, http.MethodPut, echoPath, `{"scope":"org","decision":"never"}`); response.Code != http.StatusOK {
			t.Fatalf("org never: %d %s", response.Code, response.Body.String())
		}
		if got := evaluate(true, "user-alice", binding); got.Decision != policy.DecisionDeny || got.PolicySource != policy.PolicySourceToolPolicyNever {
			t.Fatalf("an org never did not beat alice's allow: %+v", got)
		}
	})

	t.Run("revoking keeps the row, stops the policy, and the next set starts a new row", func(t *testing.T) {
		if response := callToolPolicies(t, pool, bob, http.MethodDelete, echoPath, ""); response.Code != http.StatusNotFound {
			t.Fatalf("bob revoked a policy he never set: %d", response.Code)
		}
		if response := callToolPolicies(t, pool, alice, http.MethodDelete, echoPath+"?scope=org", ""); response.Code != http.StatusForbidden {
			t.Fatalf("a non-admin revoked an org policy: %d", response.Code)
		}
		response := callToolPolicies(t, pool, alice, http.MethodDelete, echoPath, "")
		if response.Code != http.StatusOK || policyID(t, response) != aliceAllowID {
			t.Fatalf("revoke: %d %s", response.Code, response.Body.String())
		}
		if policyAudit(t, pool, aliceAllowID, auditActionToolPolicyRevoked) != 1 {
			t.Fatal("the revocation was not audited")
		}
		var revoked int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies WHERE id = $1 AND revoked_at IS NOT NULL`, aliceAllowID).Scan(&revoked); err != nil || revoked != 1 {
			t.Fatalf("the revoked row is not kept (n=%d err=%v)", revoked, err)
		}
		if response := callToolPolicies(t, pool, alice, http.MethodDelete, echoPath, ""); response.Code != http.StatusNotFound {
			t.Fatalf("a second revoke: %d", response.Code)
		}
		if got, _ := source.ActiveToolPolicies(ctx, "org-a", "user-alice", echo.capabilityID); len(got) != 1 || got[0].ScopeKind != policy.ToolPolicyScopeOrg {
			t.Fatalf("alice still has her own policy: %+v", got)
		}
		again := callToolPolicies(t, pool, alice, http.MethodPut, echoPath, `{"decision":"never"}`)
		if again.Code != http.StatusOK || policyID(t, again) == aliceAllowID {
			t.Fatalf("a set after a revoke reused the revoked row: %d %s", again.Code, again.Body.String())
		}
	})

	t.Run("the table refuses what the API never writes", func(t *testing.T) {
		for _, statement := range []string{
			`INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, set_by) VALUES ('x1','org-a','org','user-alice','c','ask','a')`,
			`INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, set_by) VALUES ('x2','org-a','user','user-alice','c','allow','a')`,
			`INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, set_by) VALUES ('x3','org-a','user','user-alice','c','deny','a')`,
			`INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, set_by) VALUES ('x4','org-a','team','t','c','ask','a')`,
		} {
			if _, err := pool.Exec(ctx, statement); err == nil {
				t.Errorf("the table accepted %s", statement)
			}
		}
	})
}
