//go:build integration

package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

func callOrgAgentPolicy(t *testing.T, pool *pgxpool.Pool, principal authctx.Principal, method, target, ifMatch, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewOrgAgentPolicyHandler(pool, true).Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	if ifMatch != "" {
		request.Header.Set("If-Match", ifMatch)
	}
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

// The revision chain, the stale-write refusal, the org isolation and the content-free
// audit row, against the real SQL.
func TestOrgAgentPolicyAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	ctx := context.Background()
	admin := authctx.Principal{OrganizationID: "org-a", ActorID: "user-admin", PrincipalType: "user", Scopes: []string{authz.WriteScope, "org:admin"}, RetentionPolicyPresent: true}
	member := authctx.Principal{OrganizationID: "org-a", ActorID: "user-m", PrincipalType: "user", RetentionPolicyPresent: true}
	otherOrg := authctx.Principal{OrganizationID: "org-b", ActorID: "user-b", PrincipalType: "user", RetentionPolicyPresent: true}
	const path = "/api/v1/org-agent-policy"

	first := callOrgAgentPolicy(t, pool, admin, http.MethodPut, path, "0", `{"settings":{"mode_cap":"read_only"}}`)
	if first.Code != http.StatusOK {
		t.Fatalf("first write = %d %s", first.Code, first.Body.String())
	}

	second := callOrgAgentPolicy(t, pool, admin, http.MethodPut, path, "1", `{"settings":{"mode_cap":"ask_first","lockdown":false}}`)
	if second.Code != http.StatusOK {
		t.Fatalf("write at the current revision = %d %s", second.Code, second.Body.String())
	}
	var moved struct {
		Revision int64 `json:"revision"`
	}
	if err := json.Unmarshal(second.Body.Bytes(), &moved); err != nil || moved.Revision != 2 {
		t.Fatalf("revision after the second write = %d (%v), want 2", moved.Revision, err)
	}

	stale := callOrgAgentPolicy(t, pool, admin, http.MethodPut, path, "1", `{"settings":{"mode_cap":"auto"}}`)
	if stale.Code != http.StatusConflict || orgAgentErrorCode(t, stale) != "stale_revision" {
		t.Fatalf("stale write = %d %s, want 409 stale_revision", stale.Code, stale.Body.String())
	}

	read := callOrgAgentPolicy(t, pool, member, http.MethodGet, path, "", "")
	var view struct {
		Revision int64          `json:"revision"`
		Settings map[string]any `json:"settings"`
	}
	if err := json.Unmarshal(read.Body.Bytes(), &view); err != nil {
		t.Fatalf("member read: %v", err)
	}
	if view.Revision != 2 || view.Settings["mode_cap"] != "ask_first" {
		t.Fatalf("member read = revision %d mode_cap %v, want 2 ask_first", view.Revision, view.Settings["mode_cap"])
	}

	foreign := callOrgAgentPolicy(t, pool, otherOrg, http.MethodGet, path, "", "")
	if err := json.Unmarshal(foreign.Body.Bytes(), &view); err != nil || view.Revision != 0 {
		t.Fatalf("another org read revision %d (%v), want the defaults at 0", view.Revision, err)
	}

	history := callOrgAgentPolicy(t, pool, member, http.MethodGet, path+"/history", "", "")
	var entries []struct {
		Revision int64 `json:"revision"`
	}
	if err := json.Unmarshal(history.Body.Bytes(), &entries); err != nil || len(entries) != 2 || entries[0].Revision != 2 || entries[1].Revision != 1 {
		t.Fatalf("history = %s, want revisions 2 then 1", history.Body.String())
	}

	var audited, leaked int
	if err := pool.QueryRow(ctx, `
		SELECT count(*), count(*) FILTER (WHERE diff_json::text LIKE '%read_only%' OR diff_json::text LIKE '%ask_first%')
		FROM registry_audit_log WHERE entity_kind = 'org_agent_policy' AND org_id = 'org-a' AND action = 'org_agent_policy_changed'
	`).Scan(&audited, &leaked); err != nil {
		t.Fatal(err)
	}
	if audited != 2 || leaked != 0 {
		t.Fatalf("audit rows = %d with %d carrying a value, want 2 and 0", audited, leaked)
	}
}
