package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/services/capability-core/internal/crypto"
)

// serverExistsDatabase answers the "does this server belong to this org"
// existence probe affirmatively so a test can reach the write itself, while
// still recording the probe's arguments.
type serverExistsDatabase struct {
	recordingDatabase
}

func (database *serverExistsDatabase) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	database.queryRows = append(database.queryRows, databaseCall{query: query, args: append([]any(nil), args...)})
	return trueRow{}
}

type trueRow struct{}

func (trueRow) Scan(dest ...any) error {
	if len(dest) == 1 {
		if target, ok := dest[0].(*bool); ok {
			*target = true
		}
	}
	return nil
}

func testVault(t *testing.T) *crypto.Vault {
	t.Helper()
	vault, err := crypto.NewVault([]byte("0123456789abcdef0123456789abcdef"))
	if err != nil {
		t.Fatalf("crypto.NewVault: %v", err)
	}
	return vault
}

const internalUpsertPath = "/api/v1/internal/mcp/oauth-tokens?server_id=mcp-1&org_id=org-a"

func internalUpsertBody() string {
	return `{"access_token":"refreshed-access","refresh_token":"kept-refresh","token_type":"Bearer",` +
		`"scope":"invoices.read","expires_in_seconds":3600,` +
		`"token_endpoint":"https://auth.example.test/token","client_id":"client-123"}`
}

func TestInternalOAuthTokensUpsertRequiresServiceToken(t *testing.T) {
	for _, testCase := range []struct {
		name           string
		configured     string
		presented      string
		wantStatusCode int
	}{
		{name: "no header presented", configured: "shared-secret", presented: "", wantStatusCode: http.StatusForbidden},
		{name: "wrong secret presented", configured: "shared-secret", presented: "guess", wantStatusCode: http.StatusForbidden},
		// An unset secret must not degrade into "any caller is trusted": the
		// route stays closed rather than accepting an empty header match.
		{name: "path disabled when unconfigured", configured: "", presented: "", wantStatusCode: http.StatusForbidden},
		{name: "correct secret accepted", configured: "shared-secret", presented: "shared-secret", wantStatusCode: http.StatusOK},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			database := &serverExistsDatabase{}
			mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken(testCase.configured)
			mcp.pool = database
			request := httptest.NewRequest(http.MethodPut, internalUpsertPath, strings.NewReader(internalUpsertBody()))
			if testCase.presented != "" {
				request.Header.Set(mcpServiceTokenHeader, testCase.presented)
			}
			response := httptest.NewRecorder()
			mcp.oauthTokensUpsertInternal(response, request)
			if response.Code != testCase.wantStatusCode {
				t.Fatalf("status = %d, want %d (%s)", response.Code, testCase.wantStatusCode, response.Body.String())
			}
			if testCase.wantStatusCode != http.StatusOK && len(database.execs) != 0 {
				t.Fatal("rejected internal upsert still wrote to the database")
			}
		})
	}
}

func TestInternalOAuthTokensUpsertRequiresTenancyScopeAndPut(t *testing.T) {
	for _, testCase := range []struct {
		name           string
		method         string
		target         string
		wantStatusCode int
	}{
		{name: "get is not routed", method: http.MethodGet, target: internalUpsertPath, wantStatusCode: http.StatusNotFound},
		{name: "org_id required", method: http.MethodPut, target: "/api/v1/internal/mcp/oauth-tokens?server_id=mcp-1", wantStatusCode: http.StatusBadRequest},
		{name: "server_id required", method: http.MethodPut, target: "/api/v1/internal/mcp/oauth-tokens?org_id=org-a", wantStatusCode: http.StatusBadRequest},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			database := &serverExistsDatabase{}
			mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken("shared-secret")
			mcp.pool = database
			request := httptest.NewRequest(testCase.method, testCase.target, strings.NewReader(internalUpsertBody()))
			request.Header.Set(mcpServiceTokenHeader, "shared-secret")
			response := httptest.NewRecorder()
			mcp.oauthTokensUpsertInternal(response, request)
			if response.Code != testCase.wantStatusCode {
				t.Fatalf("status = %d, want %d (%s)", response.Code, testCase.wantStatusCode, response.Body.String())
			}
			if len(database.execs) != 0 {
				t.Fatal("rejected internal upsert still wrote to the database")
			}
		})
	}
}

func TestInternalOAuthTokensUpsertFailsClosedWithoutVault(t *testing.T) {
	database := &serverExistsDatabase{}
	mcp := NewMCPHandler(nil).WithMCPServiceToken("shared-secret")
	mcp.pool = database
	request := httptest.NewRequest(http.MethodPut, internalUpsertPath, strings.NewReader(internalUpsertBody()))
	request.Header.Set(mcpServiceTokenHeader, "shared-secret")
	response := httptest.NewRecorder()
	mcp.oauthTokensUpsertInternal(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503", response.Code)
	}
	if len(database.execs) != 0 {
		t.Fatal("upsert stored tokens with no vault configured")
	}
}

// The refresh write-back must be as tenant-scoped and as encrypted as the
// interactive connect flow's per-user PUT — the service token widens who may
// call, never what a call may reach.
func TestInternalOAuthTokensUpsertScopesAndEncryptsLikeThePerUserPath(t *testing.T) {
	vault := testVault(t)
	database := &serverExistsDatabase{}
	mcp := NewMCPHandler(nil).WithVault(vault).WithMCPServiceToken("shared-secret")
	mcp.pool = database
	request := httptest.NewRequest(http.MethodPut, internalUpsertPath, strings.NewReader(internalUpsertBody()))
	request.Header.Set(mcpServiceTokenHeader, "shared-secret")
	response := httptest.NewRecorder()
	mcp.oauthTokensUpsertInternal(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (%s)", response.Code, response.Body.String())
	}

	if len(database.queryRows) != 1 {
		t.Fatalf("existence probes = %d, want 1", len(database.queryRows))
	}
	probe := database.queryRows[0]
	if !strings.Contains(probe.query, "FROM mcp_servers WHERE id=$1 AND org_id=$2") {
		t.Fatalf("existence probe is not (server_id, org_id) scoped: %s", probe.query)
	}
	if probe.args[0] != "mcp-1" || probe.args[1] != "org-a" {
		t.Fatalf("existence probe args = %v, want [mcp-1 org-a]", probe.args)
	}

	if len(database.execs) != 1 {
		t.Fatalf("writes = %d, want 1", len(database.execs))
	}
	write := database.execs[0]
	if write.args[0] != "mcp-1:org-a" || write.args[1] != "mcp-1" || write.args[2] != "org-a" {
		t.Fatalf("row key args = %v, want [mcp-1:org-a mcp-1 org-a]", write.args[:3])
	}
	access, ok := write.args[3].(string)
	if !ok || access == "refreshed-access" {
		t.Fatalf("access token was stored in plaintext: %v", write.args[3])
	}
	// AAD binding intact: the ciphertext only opens under this exact
	// server+org pair, so a row written here cannot be replayed into another.
	plain, err := vault.Decrypt(access, []byte("mcp-1:org-a"))
	if err != nil || plain != "refreshed-access" {
		t.Fatalf("decrypt under (mcp-1, org-a) = %q, %v", plain, err)
	}
	if _, err := vault.Decrypt(access, []byte("mcp-1:org-b")); err == nil {
		t.Fatal("ciphertext decrypted under a foreign org's AAD")
	}
	refresh, _ := write.args[4].(string)
	if refresh == "kept-refresh" {
		t.Fatal("refresh token was stored in plaintext")
	}
	if plainRefresh, err := vault.Decrypt(refresh, []byte("mcp-1:org-a")); err != nil || plainRefresh != "kept-refresh" {
		t.Fatalf("carried-forward refresh token = %q, %v", plainRefresh, err)
	}

	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("response body: %v", err)
	}
	if body["stored"] != true {
		t.Fatalf("response = %v, want stored:true", body)
	}
}
