//go:build integration

package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

type orgHookListAnswer struct {
	OrgID    string            `json:"org_id"`
	Revision int64             `json:"revision"`
	Rules    []orgHookRuleView `json:"rules"`
}

func callOrgHooks(t *testing.T, pool *pgxpool.Pool, principal authctx.Principal, method, target, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewOrgHooksHandler(pool).Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

func orgHookAdminFor(org string) authctx.Principal {
	return authctx.Principal{OrganizationID: org, ActorID: "user-admin", PrincipalType: "user", Scopes: []string{authz.WriteScope, "org:admin"}, RetentionPolicyPresent: true}
}

// The rule chain, the revision bump behind every write, the duplicate and limit refusals, the org isolation,
// and the content-free audit rows, against the real SQL.
func TestOrgHooksAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	ctx := context.Background()
	orgA := "org-hooks-a-" + uuid.NewString()
	orgB := "org-hooks-b-" + uuid.NewString()
	orgC := "org-hooks-c-" + uuid.NewString()
	memberA := authctx.Principal{OrganizationID: orgA, ActorID: "user-m", PrincipalType: "user", RetentionPolicyPresent: true}
	zdrAdminA := orgHookAdminFor(orgA)
	zdrAdminA.ZeroDataRetention = true

	created := callOrgHooks(t, pool, orgHookAdminFor(orgA), http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"no shell","priority":10}`)
	if created.Code != http.StatusOK {
		t.Fatalf("create: HTTP %d %s", created.Code, created.Body.String())
	}
	var rule orgHookWriteView
	if err := json.NewDecoder(created.Body).Decode(&rule); err != nil {
		t.Fatal(err)
	}
	if rule.Revision != 1 || rule.ID == "" || rule.CreatedBy != "user-admin" {
		t.Fatalf("create answer %+v, want revision 1 with an id and the admin as creator", rule)
	}

	var policyRevision int64
	if err := pool.QueryRow(ctx, `SELECT revision FROM org_agent_policy WHERE org_id = $1`, orgA).Scan(&policyRevision); err != nil || policyRevision != 1 {
		t.Fatalf("policy revision %d (err %v), want the rule write to have made it 1", policyRevision, err)
	}

	duplicate := callOrgHooks(t, pool, orgHookAdminFor(orgA), http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"again"}`)
	if duplicate.Code != http.StatusConflict || !strings.Contains(duplicate.Body.String(), "duplicate_rule") {
		t.Fatalf("duplicate: HTTP %d %s, want 409 duplicate_rule", duplicate.Code, duplicate.Body.String())
	}

	list := callOrgHooks(t, pool, memberA, http.MethodGet, orgHooksPath, "")
	var answer orgHookListAnswer
	if err := json.NewDecoder(list.Body).Decode(&answer); err != nil || list.Code != http.StatusOK {
		t.Fatalf("member list: HTTP %d (err %v)", list.Code, err)
	}
	if answer.Revision != 1 || len(answer.Rules) != 1 || answer.Rules[0].ID != rule.ID {
		t.Fatalf("member list %+v, want the one rule at revision 1", answer)
	}

	updated := callOrgHooks(t, pool, orgHookAdminFor(orgA), http.MethodPut, orgHooksPath+"/"+rule.ID,
		`{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"no shell at all","priority":10,"enabled":false}`)
	if updated.Code != http.StatusOK {
		t.Fatalf("update: HTTP %d %s", updated.Code, updated.Body.String())
	}
	revoked := callOrgHooks(t, pool, orgHookAdminFor(orgA), http.MethodDelete, orgHooksPath+"/"+rule.ID, "")
	if revoked.Code != http.StatusOK || !strings.Contains(revoked.Body.String(), `"revision":3`) {
		t.Fatalf("revoke: HTTP %d %s, want revision 3", revoked.Code, revoked.Body.String())
	}

	afterRevoke := callOrgHooks(t, pool, memberA, http.MethodGet, orgHooksPath, "")
	if err := json.NewDecoder(afterRevoke.Body).Decode(&answer); err != nil {
		t.Fatal(err)
	}
	if answer.Revision != 3 || len(answer.Rules) != 0 {
		t.Fatalf("after revoke %+v, want no active rules at revision 3", answer)
	}

	recreated := callOrgHooks(t, pool, orgHookAdminFor(orgA), http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"shell","decision":"deny","reason":"no shell again"}`)
	if recreated.Code != http.StatusOK {
		t.Fatalf("recreate after revoke: HTTP %d %s, want the revoked key to be free", recreated.Code, recreated.Body.String())
	}

	otherOrg := callOrgHooks(t, pool, orgHookAdminFor(orgB), http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"shell","decision":"ask","reason":"ask in org b"}`)
	if otherOrg.Code != http.StatusOK {
		t.Fatalf("org b create: HTTP %d %s", otherOrg.Code, otherOrg.Body.String())
	}
	listA := callOrgHooks(t, pool, memberA, http.MethodGet, orgHooksPath, "")
	if err := json.NewDecoder(listA.Body).Decode(&answer); err != nil {
		t.Fatal(err)
	}
	if len(answer.Rules) != 1 || answer.Rules[0].Decision != "deny" {
		t.Fatalf("org a sees %+v, want only its own deny rule", answer.Rules)
	}

	var revisions []int64
	if err := pool.QueryRow(ctx, `SELECT array_agg(revision ORDER BY revision) FROM org_agent_policy_history WHERE org_id = $1`, orgA).Scan(&revisions); err != nil {
		t.Fatal(err)
	}
	if fmt.Sprint(revisions) != "[1 2 3 4]" {
		t.Fatalf("policy history for org a is %v, want the contiguous chain 1..4", revisions)
	}
	var auditDiffs []string
	rows, err := pool.Query(ctx, `SELECT diff_json::text FROM registry_audit_log WHERE org_id = $1 AND action = 'org_hook_changed'`, orgA)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	for rows.Next() {
		var diff string
		if err := rows.Scan(&diff); err != nil {
			t.Fatal(err)
		}
		auditDiffs = append(auditDiffs, diff)
	}
	if len(auditDiffs) != 4 {
		t.Fatalf("audit rows for org a: %d, want one per write (4)", len(auditDiffs))
	}
	for _, diff := range auditDiffs {
		if strings.Contains(diff, "no shell") || strings.Contains(diff, "shell\"") {
			t.Fatalf("audit row carries rule content: %s", diff)
		}
	}

	zdrWrite := callOrgHooks(t, pool, zdrAdminA, http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"read_file","decision":"deny","reason":"zdr org"}`)
	if zdrWrite.Code != http.StatusForbidden || !strings.Contains(zdrWrite.Body.String(), "org_hook_write_under_zdr_refused") {
		t.Fatalf("zdr write: HTTP %d %s, want 403 refused", zdrWrite.Code, zdrWrite.Body.String())
	}

	// The cap: an org holds maxOrgHookRules active rules, and the next one is refused without a revision bump.
	for i := 0; i < maxOrgHookRules; i++ {
		response := callOrgHooks(t, pool, orgHookAdminFor(orgC), http.MethodPost, orgHooksPath,
			fmt.Sprintf(`{"event":"pre_tool","tool_matcher":"tool_%d","decision":"deny","reason":"rule %d"}`, i, i))
		if response.Code != http.StatusOK {
			t.Fatalf("rule %d of org c: HTTP %d %s", i, response.Code, response.Body.String())
		}
	}
	atLimit := callOrgHooks(t, pool, orgHookAdminFor(orgC), http.MethodPost, orgHooksPath,
		`{"event":"pre_tool","tool_matcher":"one_too_many","decision":"deny","reason":"over the limit"}`)
	if atLimit.Code != http.StatusUnprocessableEntity || !strings.Contains(atLimit.Body.String(), "rule_limit_reached") {
		t.Fatalf("rule %d: HTTP %d %s, want 422 rule_limit_reached", maxOrgHookRules+1, atLimit.Code, atLimit.Body.String())
	}
	var revisionC int64
	if err := pool.QueryRow(ctx, `SELECT revision FROM org_agent_policy WHERE org_id = $1`, orgC).Scan(&revisionC); err != nil || revisionC != maxOrgHookRules {
		t.Fatalf("org c revision %d (err %v), want %d: a refused create must bump nothing", revisionC, err, maxOrgHookRules)
	}
}
