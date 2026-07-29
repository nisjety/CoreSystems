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

const internalResolvePath = "/api/v1/internal/mcp/oauth-token?server_id=mcp-1&org_id=org-a"

func internalUpsertBody() string {
	return `{"access_token":"refreshed-access","refresh_token":"kept-refresh","token_type":"Bearer",` +
		`"scope":"invoices.read","expires_in_seconds":3600,` +
		`"token_endpoint":"https://auth.example.test/token","client_id":"client-123"}`
}

// CROSS-LANGUAGE KNOWN-ANSWER VECTOR. The identical (secret, org_id, expected
// hex) triple is asserted in model-gateway's Rust tests
// (rust/services/model-gateway/src/mcp_oauth.rs,
// derived_service_token_matches_the_cross_language_known_answer_vector). If
// either side's algorithm, encoding, or message framing drifts, one of the two
// tests fails here rather than silently 403ing every MCP OAuth token resolve in
// production.
const (
	vectorSecret   = "mcp-oauth-service-token-test-vector"
	vectorOrg      = "org-a"
	vectorExpected = "9b70d49c9cecb4ae9d5c871372409886a492d8dde023674d24053f916f6dfd8b"
)

func TestInternalServiceTokenDerivationMatchesTheGatewayKnownAnswerVector(t *testing.T) {
	if got := expectedInternalServiceToken(vectorSecret, vectorOrg); got != vectorExpected {
		t.Fatalf("expectedInternalServiceToken(%q, %q) = %q, want %q — model-gateway's "+
			"derive_org_scoped_service_token asserts the same vector; the two sides have drifted",
			vectorSecret, vectorOrg, got, vectorExpected)
	}
}

// The point of the derivation, stated at the algorithm level: nothing about one
// org's token helps against another's, and the root secret never appears on the
// wire.
func TestInternalServiceTokenDerivationIsOrgSpecific(t *testing.T) {
	orgA := expectedInternalServiceToken(vectorSecret, "org-a")
	orgB := expectedInternalServiceToken(vectorSecret, "org-b")
	if orgA == orgB {
		t.Fatal("two different orgs derived the same token")
	}
	if orgA == vectorSecret || strings.Contains(orgA, vectorSecret) {
		t.Fatal("derived token leaks the root secret")
	}
	if len(orgA) != 64 {
		t.Fatalf("derived token length = %d, want 64 (hex of a 32-byte digest)", len(orgA))
	}
	if orgA != strings.ToLower(orgA) {
		t.Fatalf("derived token %q is not lowercase hex", orgA)
	}
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
		// The raw root secret is no longer a valid credential on the wire —
		// only its per-org derivation is.
		{name: "raw root secret rejected", configured: "shared-secret", presented: "shared-secret", wantStatusCode: http.StatusForbidden},
		// An unset secret must not degrade into "any caller is trusted": the
		// route stays closed rather than accepting an empty header match.
		{name: "path disabled when unconfigured", configured: "", presented: "", wantStatusCode: http.StatusForbidden},
		// ...including against a token correctly derived from the empty
		// secret, which an attacker could compute for free.
		{
			name:           "unconfigured secret does not accept its own derivation",
			configured:     "",
			presented:      expectedInternalServiceToken("", "org-a"),
			wantStatusCode: http.StatusForbidden,
		},
		{
			name:           "org-derived token accepted",
			configured:     "shared-secret",
			presented:      expectedInternalServiceToken("shared-secret", "org-a"),
			wantStatusCode: http.StatusOK,
		},
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

// THE POINT OF THE WHOLE CHANGE: a service token intercepted from one tenant's
// traffic must not unlock any other tenant. Both internal routes hand out or
// overwrite LIVE decrypted OAuth credentials, so a token minted for org-b
// presented against an org-a target has to be refused outright — and refused
// before the handler body touches the database.
func TestInternalOAuthRoutesRejectAServiceTokenDerivedForADifferentOrg(t *testing.T) {
	const rootSecret = "shared-secret"
	foreignToken := expectedInternalServiceToken(rootSecret, "org-b")
	ownToken := expectedInternalServiceToken(rootSecret, "org-a")
	if foreignToken == ownToken {
		t.Fatal("test is vacuous: org-a and org-b derived the same token")
	}

	t.Run("upsert", func(t *testing.T) {
		database := &serverExistsDatabase{}
		mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken(rootSecret)
		mcp.pool = database
		// Target is org-a; the credential was minted for org-b.
		request := httptest.NewRequest(http.MethodPut, internalUpsertPath, strings.NewReader(internalUpsertBody()))
		request.Header.Set(mcpServiceTokenHeader, foreignToken)
		response := httptest.NewRecorder()
		mcp.oauthTokensUpsertInternal(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403 — org-b's token overwrote org-a's tokens (%s)",
				response.Code, response.Body.String())
		}
		if len(database.execs) != 0 || len(database.queryRows) != 0 {
			t.Fatal("cross-org upsert reached the database")
		}
	})

	t.Run("resolve", func(t *testing.T) {
		database := &recordingDatabase{}
		mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken(rootSecret)
		mcp.pool = database
		request := httptest.NewRequest(http.MethodGet, internalResolvePath, nil)
		request.Header.Set(mcpServiceTokenHeader, foreignToken)
		response := httptest.NewRecorder()
		mcp.oauthTokenResolveInternal(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403 — org-b's token read org-a's live credentials (%s)",
				response.Code, response.Body.String())
		}
		if len(database.queryRows) != 0 {
			t.Fatal("cross-org resolve reached the database")
		}
	})

	// Control arm: the same gate does admit the org's OWN derivation, so the
	// rejections above are the binding at work and not a blanket denial. The
	// resolve body then 404s on the empty test database, which is proof enough
	// that the request got past the gate.
	t.Run("own org admitted", func(t *testing.T) {
		database := &recordingDatabase{}
		mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken(rootSecret)
		mcp.pool = database
		request := httptest.NewRequest(http.MethodGet, internalResolvePath, nil)
		request.Header.Set(mcpServiceTokenHeader, ownToken)
		response := httptest.NewRecorder()
		mcp.oauthTokenResolveInternal(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, want 404 (past the gate, no stored row) (%s)",
				response.Code, response.Body.String())
		}
		if len(database.queryRows) != 1 {
			t.Fatalf("token reads = %d, want 1", len(database.queryRows))
		}
		if database.queryRows[0].args[0] != "mcp-1" || database.queryRows[0].args[1] != "org-a" {
			t.Fatalf("token read args = %v, want [mcp-1 org-a]", database.queryRows[0].args)
		}
	})
}

// Binding the credential to org_id means org_id has to be parsed before the
// gate can run, so a malformed request now answers 400 rather than 403. That is
// a deliberate ordering, and it grants nothing: the handler body is still
// unreachable, as the zero database interactions below show.
func TestInternalOAuthTokensUpsertValidatesParamsBeforeTheGateWithoutAdmittingAnything(t *testing.T) {
	database := &serverExistsDatabase{}
	mcp := NewMCPHandler(nil).WithVault(testVault(t)).WithMCPServiceToken("shared-secret")
	mcp.pool = database
	// No credential presented at all, and no org_id to bind one to.
	request := httptest.NewRequest(http.MethodPut,
		"/api/v1/internal/mcp/oauth-tokens?server_id=mcp-1", strings.NewReader(internalUpsertBody()))
	response := httptest.NewRecorder()
	mcp.oauthTokensUpsertInternal(response, request)
	if response.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 (%s)", response.Code, response.Body.String())
	}
	if len(database.execs) != 0 || len(database.queryRows) != 0 {
		t.Fatal("param rejection still reached the database")
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
			request.Header.Set(mcpServiceTokenHeader, expectedInternalServiceToken("shared-secret", "org-a"))
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
	request.Header.Set(mcpServiceTokenHeader, expectedInternalServiceToken("shared-secret", "org-a"))
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
	request.Header.Set(mcpServiceTokenHeader, expectedInternalServiceToken("shared-secret", "org-a"))
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
