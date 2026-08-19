package api

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

const mcpTestIssuer = "https://auth.example.test"

type databaseCall struct {
	query string
	args  []any
}

type recordingDatabase struct {
	execs     []databaseCall
	queries   []databaseCall
	queryRows []databaseCall
	execErr   error
	execErrAt int
	execTag   string
	queryErr  error
	rowErr    error
	rows      pgx.Rows
	// nextRows are queued QueryRow answers, consumed in order. A durable write
	// that persists through `INSERT ... RETURNING` needs a scannable result,
	// not the ErrNoRows default; queue one with expectUpsertReturningID.
	nextRows []pgx.Row
}

// databaseCalls counts every statement the stub has seen. Registration guards
// assert on this rather than on execs alone: the MCP create path persists
// through QueryRow (INSERT ... RETURNING id), so an execs-only count would
// silently stop guarding that rejected input never reaches the database.
func (database *recordingDatabase) databaseCalls() int {
	return len(database.execs) + len(database.queries) + len(database.queryRows)
}

// expectUpsertReturningID queues the id the database keeps for the next
// `INSERT ... RETURNING id`. The queue is per-subtest: an answer the handler
// never consumed is cleared and reported, so it cannot leak into a later
// subtest sharing this stub.
func (database *recordingDatabase) expectUpsertReturningID(t *testing.T, id string) {
	t.Helper()
	database.nextRows = append(database.nextRows, stringRow{value: id})
	t.Cleanup(func() {
		if len(database.nextRows) > 0 {
			database.nextRows = nil
			t.Errorf("queued RETURNING id was never consumed")
		}
	})
}

func (database *recordingDatabase) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	database.execs = append(database.execs, databaseCall{query: query, args: append([]any(nil), args...)})
	err := database.execErr
	if database.execErrAt > 0 && len(database.execs) != database.execErrAt {
		err = nil
	}
	tag := database.execTag
	if tag == "" {
		tag = "UPDATE 1"
	}
	return pgconn.NewCommandTag(tag), err
}

func (database *recordingDatabase) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	database.queries = append(database.queries, databaseCall{query: query, args: append([]any(nil), args...)})
	if database.rows != nil {
		return database.rows, database.queryErr
	}
	return emptyRows{}, database.queryErr
}

func (database *recordingDatabase) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	database.queryRows = append(database.queryRows, databaseCall{query: query, args: append([]any(nil), args...)})
	if database.rowErr != nil {
		return errorRow{err: database.rowErr}
	}
	if len(database.nextRows) > 0 {
		row := database.nextRows[0]
		database.nextRows = database.nextRows[1:]
		return row
	}
	return errorRow{err: pgx.ErrNoRows}
}

type errorRow struct{ err error }

func (row errorRow) Scan(...any) error { return row.err }

// stringRow answers an `INSERT ... RETURNING id` upsert with the id the
// database actually kept. On the ON CONFLICT path that is deliberately NOT the
// id the caller proposed — see the upsert comment in registry_apis.go.
type stringRow struct{ value string }

func (row stringRow) Scan(dest ...any) error {
	if len(dest) != 1 {
		return errors.New("stringRow: want exactly one scan destination")
	}
	target, ok := dest[0].(*string)
	if !ok {
		return errors.New("stringRow: scan destination is not *string")
	}
	*target = row.value
	return nil
}

type mcpConfigRow struct {
	config   any
	scope    string
	authKind string
}

func (row mcpConfigRow) Scan(dest ...any) error {
	if len(dest) != 3 {
		return errors.New("mcpConfigRow: want three scan destinations")
	}
	config, ok := dest[0].(*any)
	if !ok {
		return errors.New("mcpConfigRow: config destination is not *any")
	}
	scope, ok := dest[1].(*string)
	if !ok {
		return errors.New("mcpConfigRow: scope destination is not *string")
	}
	authKind, ok := dest[2].(*string)
	if !ok {
		return errors.New("mcpConfigRow: auth kind destination is not *string")
	}
	*config, *scope, *authKind = row.config, row.scope, row.authKind
	return nil
}

type emptyRows struct{}

func (emptyRows) Close()                                       {}
func (emptyRows) Err() error                                   { return nil }
func (emptyRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (emptyRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (emptyRows) Next() bool                                   { return false }
func (emptyRows) Scan(...any) error                            { return pgx.ErrNoRows }
func (emptyRows) Values() ([]any, error)                       { return nil, nil }
func (emptyRows) RawValues() [][]byte                          { return nil }
func (emptyRows) Conn() *pgx.Conn                              { return nil }

type publicMCPResolver struct{}

func (publicMCPResolver) LookupNetIP(context.Context, string, string) ([]netip.Addr, error) {
	return []netip.Addr{netip.MustParseAddr("93.184.216.34")}, nil
}

type fixedMCPResolver struct {
	addresses []netip.Addr
	err       error
}

func (resolver fixedMCPResolver) LookupNetIP(context.Context, string, string) ([]netip.Addr, error) {
	return append([]netip.Addr(nil), resolver.addresses...), resolver.err
}

type mcpTestClaims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes,omitempty"`
	ZDR           *bool    `json:"zdr,omitempty"`
	jwt.RegisteredClaims
}

func mcpAuthenticatedHandler(t *testing.T, handler http.Handler) (http.Handler, string, string) {
	t.Helper()
	authenticated, sign := mcpAuthenticatedHandlerWithSigner(t, handler)
	nonZDR := false
	return authenticated,
		sign("user", nil, &nonZDR),
		sign("user", []string{authz.WriteScope}, &nonZDR)
}

type mcpTokenSigner func(principalType string, scopes []string, zdr *bool) string

func mcpAuthenticatedHandlerWithSigner(t *testing.T, handler http.Handler) (http.Handler, mcpTokenSigner) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"capability-core"},
		Issuer:       mcpTestIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}),
	})
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	sign := func(principalType string, scopes []string, zdr *bool) string {
		actorID := "user-a"
		claims := mcpTestClaims{
			OrgID: "org-a", PrincipalType: principalType, Scopes: scopes, ZDR: zdr,
			RegisteredClaims: jwt.RegisteredClaims{
				Issuer: mcpTestIssuer, Audience: jwt.ClaimStrings{"capability-core"},
				IssuedAt: jwt.NewNumericDate(now.Add(-time.Minute)), NotBefore: jwt.NewNumericDate(now.Add(-time.Minute)),
				ExpiresAt: jwt.NewNumericDate(now.Add(time.Hour)),
			},
		}
		if principalType == "service" {
			actorID = "capability-writer"
			for _, scope := range scopes {
				if scope == authz.SpaceDeletionScope {
					actorID = spaceDeletionCoordinator
					break
				}
			}
			claims.ServiceID = actorID
		} else {
			claims.UserID = actorID
		}
		claims.Subject = actorID
		raw, signErr := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
		if signErr != nil {
			t.Fatal(signErr)
		}
		return raw
	}
	return verifier.HTTPMiddleware(authz.AuthorizeHTTP)(handler), sign
}

func TestZDRDurableMutationsAreDeniedBeforeCapabilityCoreHandlers(t *testing.T) {
	database := &recordingDatabase{}
	memory := NewMemoryHandler(nil, nil)
	memory.pool = database
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	memory.Register(mux)
	mcp.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)

	zdr := true
	for _, test := range []struct {
		name string
		path string
		body string
	}{
		{
			name: "memory body cannot downgrade issuer ZDR",
			path: "/api/v1/memory",
			body: `{"scope":"org","key":"retention","content":"must not persist","zdr":false}`,
		},
		{
			name: "MCP body cannot downgrade issuer ZDR",
			path: "/api/v1/mcp",
			body: validMCPRegistrationJSON("org-a"),
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			beforeExecs, beforeQueries, beforeRows := len(database.execs), len(database.queries), len(database.queryRows)
			request := httptest.NewRequest(http.MethodPost, test.path, strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+sign("service", []string{authz.WriteScope}, &zdr))
			request.Header.Set("X-ZDR", "false")
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusForbidden {
				t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
			}
			if len(database.execs) != beforeExecs || len(database.queries) != beforeQueries || len(database.queryRows) != beforeRows {
				t.Fatal("issuer-ZDR mutation reached a capability-core handler side effect")
			}
		})
	}

	nonZDR := false
	request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(`{"scope":"org","key":"retention","content":"explicitly permitted"}`))
	request.Header.Set("Authorization", "Bearer "+sign("service", []string{authz.WriteScope}, &nonZDR))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("explicit non-ZDR service writer status = %d, body=%s", response.Code, response.Body.String())
	}

	beforeExecs := len(database.execs)
	request = httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(`{"scope":"org","key":"retention","content":"read scope cannot persist"}`))
	request.Header.Set("Authorization", "Bearer "+sign("service", []string{authz.ReadScope}, &nonZDR))
	response = httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("read-scoped service status = %d, want 403", response.Code)
	}
	if len(database.execs) != beforeExecs {
		t.Fatal("service without capability:write reached durable storage")
	}
}

func TestMCPHandlerAuthenticationAndTenantContainment(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil).WithPublisher(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, readToken, writeToken := mcpAuthenticatedHandler(t, mux)

	t.Run("missing authentication", func(t *testing.T) {
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "/api/v1/mcp", nil))
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("status = %d, want 401", response.Code)
		}
	})

	t.Run("wrong tenant query", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/mcp?org_id=org-b", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403", response.Code)
		}
	})

	t.Run("own tenant list is pinned", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/mcp?org_id=org-a", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.queries[len(database.queries)-1]
		if len(call.args) != 1 || call.args[0] != "org-a" {
			t.Fatalf("list args = %v, want signed org", call.args)
		}
	})

	t.Run("database list error fails closed", func(t *testing.T) {
		database.queryErr = errors.New("database unavailable")
		t.Cleanup(func() { database.queryErr = nil })
		request := httptest.NewRequest(http.MethodGet, "/api/v1/mcp", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status = %d, want 500", response.Code)
		}
	})

	t.Run("write requires signed scope", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(`{"name":"denied"}`))
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403", response.Code)
		}
	})

	t.Run("invalid create body", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader("{"))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("status = %d, want 400", response.Code)
		}
	})

	t.Run("database create error fails closed", func(t *testing.T) {
		// The registration upsert persists through QueryRow (INSERT ...
		// RETURNING id), so the failure has to be injected on the row path —
		// execErr would leave this passing for the wrong reason.
		database.rowErr = errors.New("database unavailable")
		t.Cleanup(func() { database.rowErr = nil })
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(validMCPRegistrationJSON("org-a")))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status = %d, want 500", response.Code)
		}
	})

	t.Run("create replaces caller supplied tenant", func(t *testing.T) {
		database.expectUpsertReturningID(t, "mcp-kept-by-database")
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(validMCPRegistrationJSON("org-b")))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusCreated {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.queryRows[len(database.queryRows)-1]
		if !strings.Contains(call.query, "INSERT INTO mcp_servers") {
			t.Fatalf("last row call was not the registration upsert: %q", call.query)
		}
		if len(call.args) < 2 || call.args[1] != "org-a" {
			t.Fatalf("insert org = %v, want signed org-a", call.args)
		}
		// The response must echo the id the database kept, not the
		// caller-proposed id the ON CONFLICT path discards.
		var body struct {
			ID string `json:"id"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
			t.Fatal(err)
		}
		if body.ID != "mcp-kept-by-database" {
			t.Fatalf("response id = %q, want the id the database kept", body.ID)
		}
	})

	t.Run("get by id includes signed tenant", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/mcp/mcp-1", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		call := database.queryRows[len(database.queryRows)-1]
		if !strings.Contains(call.query, "org_id=$2") || len(call.args) != 2 || call.args[1] != "org-a" {
			t.Fatalf("unscoped by-id query: %q args=%v", call.query, call.args)
		}
	})

	t.Run("patch by id includes signed tenant", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-1", strings.NewReader(`{"enabled":false,"rollout_state":"quarantine"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		call := database.execs[len(database.execs)-1]
		if !strings.Contains(call.query, "org_id=$6") || len(call.args) != 6 || call.args[5] != "org-a" {
			t.Fatalf("unscoped patch query: %q args=%v", call.query, call.args)
		}
	})

	t.Run("patch shared users preserves durable MCP config", func(t *testing.T) {
		database.nextRows = append(database.nextRows, mcpConfigRow{
			config: map[string]any{
				"tool_allowlist": []string{"records.search"},
				"owner_user_id":  "user-a",
				"shared_with":    []string{},
			},
			scope:    "user",
			authKind: "none",
		})
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-1", strings.NewReader(`{"shared_with":["user-b"]}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
		}
		call := database.execs[len(database.execs)-1]
		if len(call.args) != 6 || call.args[5] != "org-a" {
			t.Fatalf("share patch was not tenant scoped: args=%v", call.args)
		}
		config, ok := call.args[2].([]byte)
		if !ok || !strings.Contains(string(config), `"shared_with":["user-b"]`) {
			t.Fatalf("share patch did not persist shared_with: %#v", call.args[2])
		}
	})

	t.Run("patch with no tenant row changed returns not found", func(t *testing.T) {
		previousTag := database.execTag
		database.execTag = "UPDATE 0"
		t.Cleanup(func() { database.execTag = previousTag })
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-foreign", strings.NewReader(`{"enabled":false}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
		}
	})

	t.Run("patch database error fails closed", func(t *testing.T) {
		database.execErr = errors.New("database unavailable")
		t.Cleanup(func() { database.execErr = nil })
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-1", strings.NewReader(`{"enabled":false}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status = %d, want 500", response.Code)
		}
	})

	t.Run("combined patch database error fails closed", func(t *testing.T) {
		database.execErr = errors.New("database unavailable")
		database.execErrAt = len(database.execs) + 1
		t.Cleanup(func() { database.execErr, database.execErrAt = nil, 0 })
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-1", strings.NewReader(`{"enabled":false,"rollout_state":"quarantine"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status = %d, want 500", response.Code)
		}
	})

	t.Run("patch cannot bypass full validation to enable", func(t *testing.T) {
		before := len(database.execs)
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/mcp-1", strings.NewReader(`{"enabled":true,"rollout_state":"stable"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusUnprocessableEntity {
			t.Fatalf("status = %d, want 422", response.Code)
		}
		if len(database.execs) != before {
			t.Fatal("unsafe enable patch reached database")
		}
	})

	t.Run("delete by id includes signed tenant", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodDelete, "/api/v1/mcp/mcp-1", nil)
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		call := database.execs[len(database.execs)-1]
		if !strings.Contains(call.query, "org_id=$3") || len(call.args) != 3 || call.args[2] != "org-a" {
			t.Fatalf("unscoped delete query: %q args=%v", call.query, call.args)
		}
	})

	t.Run("delete with no tenant row changed returns not found", func(t *testing.T) {
		previousTag := database.execTag
		database.execTag = "UPDATE 0"
		t.Cleanup(func() { database.execTag = previousTag })
		request := httptest.NewRequest(http.MethodDelete, "/api/v1/mcp/mcp-foreign", nil)
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
		}
	})

	t.Run("delete database error fails closed", func(t *testing.T) {
		database.execErr = errors.New("database unavailable")
		t.Cleanup(func() { database.execErr = nil })
		request := httptest.NewRequest(http.MethodDelete, "/api/v1/mcp/mcp-1", nil)
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status = %d, want 500", response.Code)
		}
	})

	t.Run("unsupported method", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodOptions, "/api/v1/mcp", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusMethodNotAllowed {
			t.Fatalf("status = %d, want 405", response.Code)
		}
	})
}

func validMCPRegistrationJSON(orgID string) string {
	return `{"org_id":"` + orgID + `","name":"tenant-mcp","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","config_json":{"tool_allowlist":["records.search"]}}`
}

func TestMCPRegistrationRejectsUnsafeConfiguration(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	tests := []struct {
		name string
		body string
	}{
		{name: "stdio transport", body: `{"name":"bad","endpoint_url":"stdio:///usr/bin/mcp","transport":"stdio","config_json":{"tool_allowlist":["read"]}}`},
		{name: "stdio https hybrid", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"stdio","config_json":{"tool_allowlist":["read"]}}`},
		{name: "stdio command and args", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"stdio","config_json":{"tool_allowlist":["read"],"command":"sh","args":["-c","id"]}}`},
		{name: "plain HTTP", body: `{"name":"bad","endpoint_url":"http://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "loopback IPv4", body: `{"name":"bad","endpoint_url":"https://127.0.0.1","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "private IPv4", body: `{"name":"bad","endpoint_url":"https://10.0.0.7","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "link local IPv4", body: `{"name":"bad","endpoint_url":"https://169.254.169.254/latest/meta-data","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "loopback IPv6", body: `{"name":"bad","endpoint_url":"https://[::1]","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "metadata hostname", body: `{"name":"bad","endpoint_url":"https://metadata.google.internal","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "localhost suffix", body: `{"name":"bad","endpoint_url":"https://service.localhost","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "encoded host", body: `{"name":"bad","endpoint_url":"https://127%2e0%2e0%2e1","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "numeric alternate host", body: `{"name":"bad","endpoint_url":"https://2130706433","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "userinfo credential", body: `{"name":"bad","endpoint_url":"https://user:password@mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "query credential", body: `{"name":"bad","endpoint_url":"https://mcp.example.test?token=credential","transport":"http","config_json":{"tool_allowlist":["read"]}}`},
		{name: "redirect target", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","redirect_url":"https://127.0.0.1","config_json":{"tool_allowlist":["read"]}}`},
		{name: "missing allowlist", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{}}`},
		{name: "wildcard allowlist", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["*"]}}`},
		{name: "raw token", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","token":"credential","config_json":{"tool_allowlist":["read"]}}`},
		{name: "raw authorization header", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"],"headers":{"authorization":"credential"}}}`},
		{name: "raw auth config", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"],"api_key":"credential"}}`},
		{name: "raw secret instead of reference", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"bearer","config_json":{"tool_allowlist":["read"],"secret_ref":"credential"}}`},
		{name: "HTTP URL instead of secret reference", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"bearer","config_json":{"tool_allowlist":["read"],"secret_ref":"https://vault.example.test/credential"}}`},
		{name: "unrecognized top level", body: `{"name":"bad","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"]},"command":"sh"}`},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			before := database.databaseCalls()
			request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if response.Code != http.StatusBadRequest && response.Code != http.StatusUnprocessableEntity {
				t.Fatalf("status = %d, want 400 or 422; body=%s", response.Code, response.Body.String())
			}
			if got := database.databaseCalls(); got != before {
				t.Fatalf("unsafe registration reached database: before=%d after=%d", before, got)
			}
		})
	}
}

func TestMCPRegistrationRejectsPrivateDNSResolution(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = fixedMCPResolver{addresses: []netip.Addr{netip.MustParseAddr("10.20.30.40")}}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(validMCPRegistrationJSON("org-a")))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("privately resolved endpoint reached database")
	}
}

func TestMCPRegistrationAcceptsManagedSecretReference(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	database.expectUpsertReturningID(t, "mcp-accounting")
	body := `{"name":"accounting","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"oauth","config_json":{"tool_allowlist":["invoices.read"],"secret_ref":"vault://model-plane/mcp-accounting"}}`
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", response.Code, response.Body.String())
	}
	// The registration is one durable statement, the RETURNING upsert.
	if len(database.queryRows) != 1 || len(database.execs) != 0 {
		t.Fatalf("database writes: upserts=%d execs=%d, want exactly 1 upsert",
			len(database.queryRows), len(database.execs))
	}
}

func TestMCPRegistrationBodyIsBounded(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	body := `{"name":"` + strings.Repeat("a", 128<<10) + `","endpoint_url":"https://mcp.example.test","transport":"http","config_json":{"tool_allowlist":["read"]}}`
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusBadRequest && response.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("status = %d, want 400 or 413", response.Code)
	}
	if database.databaseCalls() != 0 {
		t.Fatal("oversize registration reached database")
	}
}

func TestMCPServerRowNeverSerializesRawConfig(t *testing.T) {
	row := mcpServerRow{
		ID:         "mcp-1",
		ConfigJSON: map[string]any{"tool_allowlist": []string{"read"}, "secret_ref": "secret://provider/credential"},
	}
	payload, err := json.Marshal(row)
	if err != nil {
		t.Fatal(err)
	}
	for _, forbidden := range []string{"config_json", "secret://", "credential"} {
		if strings.Contains(string(payload), forbidden) {
			t.Fatalf("serialized registry row leaked %q: %s", forbidden, payload)
		}
	}
}

func TestMCPServerViewRedactsSecretAndQuarantinesLegacyUnsafeRows(t *testing.T) {
	safe := mcpServerRow{
		ID: "mcp-1", EndpointURL: "https://mcp.example.test", Transport: "http", AuthKind: "oauth",
		Scope: "workspace", Enabled: true, RolloutState: "stable",
		ConfigJSON: map[string]any{"tool_allowlist": []string{"invoices.read"}, "secret_ref": "vault://model-plane/mcp-accounting"},
	}
	hydrateMCPServerView(&safe)
	if safe.ConfigurationState != "valid" || !safe.SecretConfigured || len(safe.ToolAllowlist) != 1 {
		t.Fatalf("safe view = %+v", safe)
	}
	payload, err := json.Marshal(safe)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(payload), "vault://") || strings.Contains(string(payload), "config_json") {
		t.Fatalf("safe view leaked raw config: %s", payload)
	}

	legacy := mcpServerRow{
		ID: "mcp-legacy", EndpointURL: "https://embedded:credential@mcp.example.test", Transport: "stdio", AuthKind: "none",
		Scope: "workspace", Enabled: true, RolloutState: "stable",
		ConfigJSON: map[string]any{"tool_allowlist": []string{}},
	}
	hydrateMCPServerView(&legacy)
	if legacy.Enabled || legacy.EndpointURL != "" || legacy.RolloutState != "quarantine" || legacy.ConfigurationState != "invalid" {
		t.Fatalf("unsafe legacy row was not fail-closed: %+v", legacy)
	}
}

func TestMCPHandlerRejectsDirectUnauthenticatedWrite(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(validMCPRegistrationJSON("org-a")))
	response := httptest.NewRecorder()
	mcp.listOrCreate(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403", response.Code)
	}
	if database.databaseCalls() != 0 {
		t.Fatal("unauthenticated direct handler call reached database")
	}
}

func TestMCPEndpointValidationEdgeCases(t *testing.T) {
	endpoint, host, err := parseMCPEndpoint("https://MCP.Example.Test:8443/rpc")
	if err != nil || endpoint != "https://mcp.example.test:8443/rpc" || host != "mcp.example.test" {
		t.Fatalf("valid endpoint = %q host=%q err=%v", endpoint, host, err)
	}
	for _, raw := range []string{
		"", "https://single-label", "https://mcp.example.test.", "https://xn--example.test",
		"https://mcp.example.test:99999", "https://mcp.example.test/#fragment",
		"https://service.cluster.local", "https://lookup.in-addr.arpa",
	} {
		if _, _, err := parseMCPEndpoint(raw); err == nil {
			t.Errorf("parseMCPEndpoint(%q) succeeded", raw)
		}
	}
}

func TestMCPEndpointInternalAllowlist(t *testing.T) {
	// Baseline: without the operator opt-in, a plain-HTTP internal host is rejected.
	t.Setenv("MCP_INTERNAL_ALLOWED_HOSTS", "")
	if _, _, err := parseMCPEndpoint("http://mcp-bridge:9201"); err == nil {
		t.Fatal("http://mcp-bridge without allowlist should be rejected")
	}
	// Opt-in: the trusted internal host is accepted over HTTP and the http scheme
	// is preserved (the co-located bridge is not TLS-terminated).
	t.Setenv("MCP_INTERNAL_ALLOWED_HOSTS", "mcp-bridge, other-internal")
	endpoint, host, err := parseMCPEndpoint("http://mcp-bridge:9201")
	if err != nil || host != "mcp-bridge" || endpoint != "http://mcp-bridge:9201" {
		t.Fatalf("allowlisted internal endpoint = %q host=%q err=%v", endpoint, host, err)
	}
	// A host NOT on the allowlist still cannot use plain HTTP.
	if _, _, err := parseMCPEndpoint("http://evil.example"); err == nil {
		t.Fatal("http://evil.example should still be rejected")
	}
}

func TestMCPConfigValidationEdgeCases(t *testing.T) {
	valid, err := normalizeMCPConfig(mcpServerConfig{
		ToolAllowlist: []string{"records.read"}, OwnerUserID: "user-1", SharedWith: []string{"user-2"},
	}, "none", "user")
	if err != nil || len(valid.ToolAllowlist) != 1 {
		t.Fatalf("valid user config = %+v err=%v", valid, err)
	}

	tooMany := make([]string, maxMCPToolAllowlistEntries+1)
	for index := range tooMany {
		tooMany[index] = "tool." + strconv.Itoa(index)
	}
	invalid := []mcpServerConfig{
		{ToolAllowlist: []string{"read", "read"}},
		{ToolAllowlist: tooMany},
		{ToolAllowlist: []string{"read"}, SecretRef: "vault://provider/item"},
		{ToolAllowlist: []string{"read"}, OwnerUserID: "bad owner"},
		{ToolAllowlist: []string{"read"}, OwnerUserID: "user-1", SharedWith: []string{"bad user"}},
	}
	for index, config := range invalid {
		if _, err := normalizeMCPConfig(config, "none", "user"); err == nil {
			t.Errorf("invalid config %d succeeded", index)
		}
	}
}

func TestDecodeStoredMCPConfigRepresentations(t *testing.T) {
	payload := `{"tool_allowlist":["read"]}`
	for _, raw := range []any{[]byte(payload), payload, json.RawMessage(payload), map[string]any{"tool_allowlist": []string{"read"}}} {
		config, err := decodeStoredMCPConfig(raw)
		if err != nil || len(config.ToolAllowlist) != 1 {
			t.Fatalf("decode %T = %+v err=%v", raw, config, err)
		}
	}
	if config, err := decodeStoredMCPConfig(nil); err != nil || len(config.ToolAllowlist) != 0 {
		t.Fatalf("nil config = %+v err=%v", config, err)
	}
	for _, raw := range []any{`{`, strings.Repeat("x", maxMCPRegistrationBodyBytes+1)} {
		if _, err := decodeStoredMCPConfig(raw); err == nil {
			t.Errorf("decode %T invalid payload succeeded", raw)
		}
	}
}

func TestMCPRegistrationFailsClosedWhenDNSUnavailable(t *testing.T) {
	handler := &MCPHandler{resolver: fixedMCPResolver{err: errors.New("resolver unavailable")}}
	_, _, err := handler.normalizeMCPRegistration(context.Background(), mcpServerRegistration{
		Name: "accounting", EndpointURL: "https://mcp.example.test", Transport: "http", AuthKind: "none",
		ConfigJSON: mcpServerConfig{ToolAllowlist: []string{"records.read"}},
	}, "org-a")
	if err == nil {
		t.Fatal("registration succeeded without DNS evidence")
	}
}
