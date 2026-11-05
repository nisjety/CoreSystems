//go:build integration

package api

import (
	"context"
	"net/http"
	"os"
	"path/filepath"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// setupMCPToolsDB creates and destroys its own disposable Postgres container.
// It never connects to the application's databases.
func TestScopedBrowserPolicyAgainstIsolatedPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	ctx := context.Background()
	alice := policyCaller{org: "org-a", actor: "user-alice", scopes: []string{authz.WriteScope}}
	admin := policyCaller{org: "org-a", actor: "user-admin", scopes: []string{authz.WriteScope, "org:admin"}}
	path := "/api/v1/tool-policies/cap.browser.open"
	if r := callToolPolicies(t, pool, alice, http.MethodPut, path, `{"decision":"ask"}`); r.Code != http.StatusOK {
		t.Fatalf("user ask: %d %s", r.Code, r.Body.String())
	}
	cap := &models.Capability{ID: "cap.browser.open", Kind: models.KindTool, RiskLevel: models.RiskHigh, EnabledForScopes: []string{policy.ScopeWildcard}, SideEffectClass: models.SideEffectExternalWrite}
	binding := &policy.ToolBinding{ToolName: "browser_agent", CapabilityID: cap.ID, SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone}
	engine := policy.New(nil).WithToolPolicies(NewToolPoliciesHandler(pool), false).WithBrowserAgentPolicyEnforced(true)
	evaluate := func(org, user string, b *policy.ToolBinding) *policy.Result {
		t.Helper()
		r, err := engine.EvaluateTool(ctx, cap, b, user, "run-a", "execution-core", org, "global")
		if err != nil {
			t.Fatal(err)
		}
		return r
	}
	if r := evaluate("org-a", "user-alice", binding); r.Decision != policy.DecisionAsk || r.PolicySource != policy.PolicySourceToolPolicyAsk || !r.ToolPolicyChecked {
		t.Fatalf("user policy not enforced: %+v", r)
	}
	if r := evaluate("org-a", "user-bob", binding); r.Decision != policy.DecisionAsk || r.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("another user policy leaked: %+v", r)
	}
	if r := callToolPolicies(t, pool, admin, http.MethodPut, path, `{"scope":"org","decision":"never"}`); r.Code != http.StatusOK {
		t.Fatalf("org never: %d %s", r.Code, r.Body.String())
	}
	for _, user := range []string{"user-alice", "user-bob"} {
		if r := evaluate("org-a", user, binding); r.Decision != policy.DecisionDeny || r.PolicySource != policy.PolicySourceToolPolicyNever {
			t.Fatalf("org never not enforced for %s: %+v", user, r)
		}
	}
	if r := evaluate("org-b", "user-alice", binding); r.Decision != policy.DecisionAsk || r.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("another org policy leaked: %+v", r)
	}
	other := *binding
	other.ToolName = "browser.console"
	if r := evaluate("org-a", "user-alice", &other); r.Decision != policy.DecisionAsk || r.ToolPolicyChecked {
		t.Fatalf("other tool changed: %+v", r)
	}
	var before int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies`).Scan(&before); err != nil {
		t.Fatal(err)
	}
	migration, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0030_tool_policy_expiry.up.sql"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = pool.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("reapply expiry migration: %v", err)
	}
	var after int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies`).Scan(&after); err != nil || before != after {
		t.Fatalf("migration changed row count: before=%d after=%d error=%v", before, after, err)
	}
	if r := evaluate("org-a", "user-alice", binding); r.Decision != policy.DecisionDeny {
		t.Fatalf("migration changed existing policy: %+v", r)
	}
}
