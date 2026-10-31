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
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// setupMCPToolsDB starts a throwaway Postgres and applies every migration in
// order, the way docker-entrypoint.sh does at boot: the observation statement
// writes columns that migration 0016 adds to tables earlier ones create.
func setupMCPToolsDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capabilities"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		cleanupCtx, cleanupCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cleanupCancel()
		_ = container.Terminate(cleanupCtx)
	})
	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	t.Cleanup(pool.Close)
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		sqlBytes, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read %s: %v", path, readErr)
		}
		if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply %s: %v", filepath.Base(path), applyErr)
		}
	}
	return pool
}

func seedMCPServer(t *testing.T, pool *pgxpool.Pool, org, id, scope string, config string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO mcp_servers (id, org_id, name, endpoint_url, transport, auth_kind, config_json, scope, enabled, rollout_state, risk_level)
		VALUES ($1, $2, $3, 'https://mcp.example.test', 'http', 'none', $4::jsonb, $5, true, 'stable', 'medium')
	`, id, org, "name-"+id, config, scope); err != nil {
		t.Fatalf("seed server: %v", err)
	}
}

type toolRowSnapshot struct {
	class, floor, pinned, observed, state, declared, source string
	capabilityID                                            string
}

func readToolRow(t *testing.T, pool *pgxpool.Pool, org, tool string) toolRowSnapshot {
	t.Helper()
	var s toolRowSnapshot
	err := pool.QueryRow(context.Background(), `
		SELECT capability_id, side_effect_class, approval_floor, COALESCE(pinned_digest, ''), COALESCE(observed_digest, ''),
		       COALESCE(review_state, ''), COALESCE(declared_side_effect, ''), source
		FROM capability_tools WHERE org_id = $1 AND tool_name = $2 AND qualified_name IS NOT NULL
	`, org, tool).Scan(&s.capabilityID, &s.class, &s.floor, &s.pinned, &s.observed, &s.state, &s.declared, &s.source)
	if err != nil {
		t.Fatalf("read tool row %s: %v", tool, err)
	}
	return s
}

type capabilitySnapshot struct {
	enabled                                               bool
	kind, version, risk, class, floor, availability, code string
	orgID                                                 string
}

func readCapability(t *testing.T, pool *pgxpool.Pool, id string) capabilitySnapshot {
	t.Helper()
	var c capabilitySnapshot
	err := pool.QueryRow(context.Background(), `
		SELECT org_id, kind, version, risk_level, enabled, side_effect_class, approval_floor, availability_state, availability_reason_code
		FROM capabilities WHERE id = $1 AND deleted_at IS NULL
	`, id).Scan(&c.orgID, &c.kind, &c.version, &c.risk, &c.enabled, &c.class, &c.floor, &c.availability, &c.code)
	if err != nil {
		t.Fatalf("read capability %s: %v", id, err)
	}
	return c
}

func countAudit(t *testing.T, pool *pgxpool.Pool, entityID, action string) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), `
		SELECT count(*) FROM registry_audit_log WHERE entity_kind = 'capability' AND entity_id = $1 AND action = $2
	`, entityID, action).Scan(&n); err != nil {
		t.Fatalf("count audit: %v", err)
	}
	return n
}

func integrationObserve(t *testing.T, mcp *MCPHandler, org string, observations ...string) []observationResult {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/internal/mcp/tools/observe?org_id="+org, strings.NewReader(observationBody(observations...)))
	request.Header.Set(mcpServiceTokenHeader, expectedInternalServiceToken(observeRootSecret, org))
	response := httptest.NewRecorder()
	mcp.observeToolsInternal(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("observe status = %d, body=%s", response.Code, response.Body.String())
	}
	var reply observeReply
	if err := json.Unmarshal(response.Body.Bytes(), &reply); err != nil {
		t.Fatalf("decode: %v", err)
	}
	return reply.Results
}

func integrationApprove(t *testing.T, mcp *MCPHandler, org, actor string, scopes []string, server, tool, digest string) *httptest.ResponseRecorder {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp/"+server+"/tools/"+tool+"/approve", strings.NewReader(`{"approved_digest":"`+digest+`"}`))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: org, ActorID: actor, PrincipalType: "user", Scopes: scopes, RetentionPolicyPresent: true,
	}))
	response := httptest.NewRecorder()
	mcp.approveTool(response, request, server, tool)
	return response
}

// The three fixture tools of scripts/tests/fixtures/mcp-annotated-server.mjs:
// first sight, a changed definition, and its approval, against the real SQL.
func TestObservationDriftAndApprovalAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read","create_item","drop_all"]}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)

	first := integrationObserve(t, mcp, "org-a",
		observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
		observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, ""),
		observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
	)
	for _, result := range first {
		if result.Outcome != outcomeRecorded {
			t.Fatalf("first observation: %+v", result)
		}
	}
	for tool, want := range map[string][2]string{
		"echo_read":   {models.SideEffectExternalWrite, models.ApprovalFloorNone},
		"create_item": {models.SideEffectDestructive, models.ApprovalFloorNone},
		"drop_all":    {models.SideEffectUnclassified, models.ApprovalFloorAsk},
	} {
		row := readToolRow(t, pool, "org-a", tool)
		if row.class != want[0] || row.floor != want[1] || row.state != reviewApproved || row.source != "derived" {
			t.Fatalf("%s row = %+v, want %v approved", tool, row, want)
		}
		capability := readCapability(t, pool, row.capabilityID)
		if capability.orgID != "org-a" || capability.kind != models.KindMCPTool || !capability.enabled ||
			capability.risk != models.RiskHigh || capability.class != want[0] || capability.availability != "available" {
			t.Fatalf("%s capability = %+v", tool, capability)
		}
		if countAudit(t, pool, row.capabilityID, auditActionToolObserved) != 1 {
			t.Fatalf("%s: missing the %s audit row", tool, auditActionToolObserved)
		}
	}

	// A second sighting of the same definitions refreshes and writes no audit row.
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	echo := readToolRow(t, pool, "org-a", "echo_read")
	if countAudit(t, pool, echo.capabilityID, auditActionToolObserved) != 1 {
		t.Fatal("an unchanged observation was audited again")
	}
	versionBefore := readCapability(t, pool, echo.capabilityID).version

	// The server changes echo_read's definition: the next observation drifts it.
	drift := integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))
	if drift[0].ReviewState != reviewDrifted {
		t.Fatalf("drift result = %+v", drift[0])
	}
	echo = readToolRow(t, pool, "org-a", "echo_read")
	capability := readCapability(t, pool, echo.capabilityID)
	if echo.state != reviewDrifted || echo.pinned != knownAnswerToolDigest || echo.observed != changedToolDigest || echo.class != models.SideEffectExternalWrite {
		t.Fatalf("row after drift = %+v", echo)
	}
	if capability.enabled || capability.code != models.ReasonToolDefinitionChanged || capability.version != versionBefore {
		t.Fatalf("capability after drift = %+v", capability)
	}
	if countAudit(t, pool, echo.capabilityID, auditActionToolDrifted) != 1 {
		t.Fatal("drift was not audited exactly once")
	}
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))
	if countAudit(t, pool, echo.capabilityID, auditActionToolDrifted) != 1 {
		t.Fatal("a repeated drifted digest was audited again")
	}

	// The other two tools are untouched by echo_read's drift.
	if readToolRow(t, pool, "org-a", "create_item").state != reviewApproved {
		t.Fatal("drift spread to another tool")
	}

	// Approval needs authority: a non-admin of an org-scope server is refused, an admin re-pins.
	admin := []string{authz.WriteScope, "org:admin"}
	if response := integrationApprove(t, mcp, "org-a", "user-1", []string{authz.WriteScope}, "srv", "echo_read", changedToolDigest); response.Code != http.StatusForbidden {
		t.Fatalf("non-admin approval status = %d, want 403", response.Code)
	}
	if readToolRow(t, pool, "org-a", "echo_read").state != reviewDrifted {
		t.Fatal("a refused approval re-pinned the tool")
	}
	if response := integrationApprove(t, mcp, "org-b", "admin-b", admin, "srv", "echo_read", changedToolDigest); response.Code != http.StatusNotFound {
		t.Fatalf("another org's admin status = %d, want 404", response.Code)
	}

	// An admin reviewed changedToolDigest, then the server served another
	// definition and the next listing recorded it: the approval must not pin it.
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, anotherChangedToolDigest, ""))
	if response := integrationApprove(t, mcp, "org-a", "admin-1", admin, "srv", "echo_read", changedToolDigest); response.Code != http.StatusConflict {
		t.Fatalf("stale approval status = %d, want 409, body=%s", response.Code, response.Body.String())
	}
	echo = readToolRow(t, pool, "org-a", "echo_read")
	if echo.state != reviewDrifted || echo.pinned != knownAnswerToolDigest || echo.observed != anotherChangedToolDigest ||
		readCapability(t, pool, echo.capabilityID).enabled || countAudit(t, pool, echo.capabilityID, auditActionToolApproved) != 0 {
		t.Fatalf("a stale approval changed the tool: %+v", echo)
	}

	if response := integrationApprove(t, mcp, "org-a", "admin-1", admin, "srv", "echo_read", anotherChangedToolDigest); response.Code != http.StatusOK {
		t.Fatalf("admin approval status = %d, body=%s", response.Code, response.Body.String())
	}
	echo = readToolRow(t, pool, "org-a", "echo_read")
	capability = readCapability(t, pool, echo.capabilityID)
	if echo.state != reviewApproved || echo.pinned != anotherChangedToolDigest {
		t.Fatalf("row after approval = %+v", echo)
	}
	if !capability.enabled || capability.code != "" || capability.version == versionBefore {
		t.Fatalf("capability after approval = %+v", capability)
	}
	if countAudit(t, pool, echo.capabilityID, auditActionToolApproved) != 1 {
		t.Fatal("approval was not audited")
	}

	// Disabling the server switches every tool of it off in the same statement.
	if _, err := pool.Exec(context.Background(), `UPDATE mcp_servers SET enabled = true WHERE id = 'srv'`); err != nil {
		t.Fatal(err)
	}
	disable := withMCPToolCascade(
		`UPDATE mcp_servers SET enabled=false, updated_at=now() WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`, "$2", "$1")
	if _, err := pool.Exec(context.Background(), disable, "srv", "org-a"); err != nil {
		t.Fatalf("cascade: %v", err)
	}
	for _, tool := range []string{"echo_read", "create_item", "drop_all"} {
		row := readToolRow(t, pool, "org-a", tool)
		if readCapability(t, pool, row.capabilityID).enabled {
			t.Fatalf("%s stayed enabled after its server was disabled", tool)
		}
	}
}

func TestObservationNeverLowersAClassAndTwoOrgsNeverShareARow(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read"],"annotation_trust":true}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)

	// org-a trusts the server, so a read-only claim derives to read; org-b has no
	// such server (a server id belongs to one org), so its tool is pending and the
	// claim is not believed.
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	integrationObserve(t, mcp, "org-b", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	a, b := readToolRow(t, pool, "org-a", "echo_read"), readToolRow(t, pool, "org-b", "echo_read")
	if a.class != models.SideEffectRead || b.class != models.SideEffectExternalWrite || a.capabilityID == b.capabilityID {
		t.Fatalf("org-a %+v, org-b %+v", a, b)
	}

	// Trust is withdrawn: the next observation raises the class and never lowers it.
	if _, err := pool.Exec(context.Background(), `UPDATE mcp_servers SET config_json = '{"tool_allowlist":["echo_read"]}'::jsonb WHERE org_id = 'org-a'`); err != nil {
		t.Fatal(err)
	}
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	if got := readToolRow(t, pool, "org-a", "echo_read").class; got != models.SideEffectExternalWrite {
		t.Fatalf("class after trust was withdrawn = %s, want external_write", got)
	}
	// Trust is granted again: observation does not lower it back.
	if _, err := pool.Exec(context.Background(), `UPDATE mcp_servers SET config_json = '{"tool_allowlist":["echo_read"],"annotation_trust":true}'::jsonb WHERE org_id = 'org-a'`); err != nil {
		t.Fatal(err)
	}
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	if got := readToolRow(t, pool, "org-a", "echo_read").class; got != models.SideEffectExternalWrite {
		t.Fatalf("observation lowered the class to %s", got)
	}
}

func TestRegistrationUpsertKeepsAStoredTrustChoiceOnlyForTheSameEndpoint(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["a"],"annotation_trust":true}`)
	seedMCPServer(t, pool, "org-a", "own", "user", `{"tool_allowlist":["a"],"owner_user_id":"user-a","annotation_trust":true}`)
	mcp := NewMCPHandler(pool)
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	nonZDR := false
	token := sign("user", []string{authz.WriteScope}, &nonZDR)

	register := func(name, endpoint, scope, config string) {
		t.Helper()
		body := `{"name":"` + name + `","endpoint_url":"` + endpoint + `","transport":"http","auth_kind":"none","scope":"` + scope + `","enabled":true,"config_json":` + config + `}`
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+token)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusCreated {
			t.Fatalf("registration status = %d, body=%s", response.Code, response.Body.String())
		}
	}
	trusted := func(id string) bool {
		var raw []byte
		if err := pool.QueryRow(context.Background(), `SELECT config_json FROM mcp_servers WHERE id = $1`, id).Scan(&raw); err != nil {
			t.Fatal(err)
		}
		return strings.Contains(string(raw), `"annotation_trust": true`) || strings.Contains(string(raw), `"annotation_trust":true`)
	}

	register("name-srv", "https://mcp.example.test", "org", `{"tool_allowlist":["a"]}`) // the stored endpoint: the choice is kept
	if !trusted("srv") {
		t.Fatal("a re-registration of the same endpoint dropped the stored trust")
	}
	register("name-srv", "https://other.example.test", "org", `{"tool_allowlist":["a"]}`) // pointed elsewhere: the trust does not follow
	if trusted("srv") {
		t.Fatal("trust followed the record to a different endpoint")
	}

	// The owner's trust of their own server is not an admin's trust of a shared one:
	// widening the scope with the endpoint unchanged drops it.
	register("name-own", "https://mcp.example.test", "user", `{"tool_allowlist":["a"],"owner_user_id":"user-a"}`)
	if !trusted("own") {
		t.Fatal("a re-registration of the same endpoint and scope dropped the owner's trust")
	}
	register("name-own", "https://mcp.example.test", "org", `{"tool_allowlist":["a"]}`)
	if trusted("own") {
		t.Fatal("a non-admin carried the owner's trust onto a server shared beyond one user")
	}
}

// ---------------------------------------------------------------------------
// The reviewer's round, against the real statements
// ---------------------------------------------------------------------------

// hookedPool runs a function just before a statement executes: a stand-in for
// another request (a PATCH, a DELETE) landing between the handler's reads and its
// write.
type hookedPool struct {
	registryDatabase
	before func(query string)
}

func (p *hookedPool) QueryRow(ctx context.Context, query string, args ...any) pgx.Row {
	if p.before != nil {
		p.before(query)
	}
	return p.registryDatabase.QueryRow(ctx, query, args...)
}

const (
	disableServerSQL = `UPDATE mcp_servers SET enabled=false, updated_at=now() WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`
	deleteServerSQL  = `UPDATE mcp_servers SET deleted_at=now(), updated_at=now() WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`
)

func cascadeUpdate(t *testing.T, pool *pgxpool.Pool, statement string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), withMCPToolCascade(statement, "$2", "$1"), "srv", "org-a"); err != nil {
		t.Errorf("cascade: %v", err)
	}
}

// A disable or a delete lands after the first tool of a batch was written, with
// the batch's single read of the server still saying enabled. The writes after it
// must leave their tools off.
func TestAServerTakenOutMidBatchStaysOffAgainstPostgres(t *testing.T) {
	for name, statement := range map[string]string{"disabled": disableServerSQL, "deleted": deleteServerSQL} {
		t.Run(name, func(t *testing.T) {
			pool := setupMCPToolsDB(t)
			seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read","create_item","drop_all"]}`)
			hooked := &hookedPool{registryDatabase: pool}
			writes := 0
			hooked.before = func(query string) {
				if query != applyObservationSQL {
					return
				}
				writes++
				if writes == 2 {
					cascadeUpdate(t, pool, statement)
				}
			}
			mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)
			mcp.pool = hooked

			results := integrationObserve(t, mcp, "org-a",
				observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
				observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, ""),
				observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
			)
			for _, result := range results {
				if result.Outcome != outcomeRecorded {
					t.Fatalf("result = %+v", result)
				}
			}
			for _, tool := range []string{"echo_read", "create_item", "drop_all"} {
				row := readToolRow(t, pool, "org-a", tool)
				capability := readCapability(t, pool, row.capabilityID)
				if capability.enabled || capability.availability != "unavailable" || capability.code != "capability_disabled" {
					t.Fatalf("%s capability = %+v: a write after the server was taken out left it on", tool, capability)
				}
				if row.state != reviewApproved || row.pinned == "" {
					t.Fatalf("%s row = %+v: the registry row must still be recorded", tool, row)
				}
			}
		})
	}
}

func TestAnApprovalAfterTheServerWasDisabledDoesNotReenableTheToolAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read"]}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))

	hooked := &hookedPool{registryDatabase: pool}
	hooked.before = func(query string) {
		if query == approveToolSQL {
			cascadeUpdate(t, pool, disableServerSQL)
		}
	}
	mcp.pool = hooked
	if response := integrationApprove(t, mcp, "org-a", "admin-1", []string{authz.WriteScope, "org:admin"}, "srv", "echo_read", changedToolDigest); response.Code != http.StatusOK {
		t.Fatalf("approval status = %d, body=%s", response.Code, response.Body.String())
	}
	row := readToolRow(t, pool, "org-a", "echo_read")
	capability := readCapability(t, pool, row.capabilityID)
	if row.state != reviewApproved || row.pinned != changedToolDigest {
		t.Fatalf("the person's approval was not recorded: %+v", row)
	}
	if capability.enabled || capability.code != "capability_disabled" {
		t.Fatalf("capability = %+v: the approval re-enabled a tool of a disabled server", capability)
	}
}

func TestAStaleReplicaCannotRestoreADriftedToolAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read"]}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))
	before := readToolRow(t, pool, "org-a", "echo_read")

	result := integrationObserve(t, mcp, "org-a", observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))[0]
	after := readToolRow(t, pool, "org-a", "echo_read")
	if result.ReviewState != reviewDrifted || after != before || after.observed != changedToolDigest {
		t.Fatalf("a stale replica changed a drifted row: %+v -> %+v (result %+v)", before, after, result)
	}
	if readCapability(t, pool, after.capabilityID).enabled {
		t.Fatal("a stale replica re-enabled a drifted tool")
	}
}

func integrationRegister(t *testing.T, handler http.Handler, token, body string) {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("registration status = %d, body=%s", response.Code, response.Body.String())
	}
}

func integrationTrusted(t *testing.T, pool *pgxpool.Pool, id string) bool {
	t.Helper()
	var raw []byte
	if err := pool.QueryRow(context.Background(), `SELECT config_json FROM mcp_servers WHERE id = $1`, id).Scan(&raw); err != nil {
		t.Fatal(err)
	}
	return strings.Contains(string(raw), `"annotation_trust": true`) || strings.Contains(string(raw), `"annotation_trust":true`)
}

// A re-registration switches off the tools it no longer lists and those of a
// server it leaves outside stable or canary rollout, in its one statement; a tool
// comes back through the next observation once it is listed and rolled out again.
func TestARegistrationSwitchesOffRemovedToolsAndQuarantinedServersAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "srv", "org", `{"tool_allowlist":["echo_read","create_item","drop_all"]}`)
	mcp := NewMCPHandler(pool).WithMCPServiceToken(observeRootSecret)
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	nonZDR := false
	token := sign("user", []string{authz.WriteScope}, &nonZDR)

	all := []string{"echo_read", "create_item", "drop_all"}
	observeAll := func() {
		integrationObserve(t, mcp, "org-a",
			observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
			observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, ""),
			observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
		)
	}
	enabled := func(tool string) bool {
		return readCapability(t, pool, readToolRow(t, pool, "org-a", tool).capabilityID).enabled
	}
	registration := func(allowlist, rollout string) string {
		return `{"name":"name-srv","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","enabled":true,"rollout_state":"` +
			rollout + `","config_json":{"tool_allowlist":` + allowlist + `}}`
	}
	observeAll()
	for _, tool := range all {
		if !enabled(tool) {
			t.Fatalf("setup: %s is not enabled", tool)
		}
	}

	// Two tools leave the allowlist.
	integrationRegister(t, handler, token, registration(`["echo_read"]`, "stable"))
	if !enabled("echo_read") || enabled("create_item") || enabled("drop_all") {
		t.Fatalf("after the allowlist shrank: echo_read=%v create_item=%v drop_all=%v", enabled("echo_read"), enabled("create_item"), enabled("drop_all"))
	}
	if code := readCapability(t, pool, readToolRow(t, pool, "org-a", "create_item").capabilityID).code; code != "capability_disabled" {
		t.Fatalf("reason code = %q", code)
	}
	// An observation of a tool the server no longer lists does not bring it back.
	observeAll()
	if enabled("create_item") || enabled("drop_all") {
		t.Fatal("an observation re-enabled a tool that is not on the allowlist")
	}

	// Listed again, they return on the next observation.
	integrationRegister(t, handler, token, registration(`["echo_read","create_item","drop_all"]`, "stable"))
	observeAll()
	for _, tool := range all {
		if !enabled(tool) {
			t.Fatalf("%s did not come back after it was listed again", tool)
		}
	}

	// A quarantined server takes every tool off, and observation keeps them off.
	integrationRegister(t, handler, token, registration(`["echo_read","create_item","drop_all"]`, "quarantine"))
	for _, tool := range all {
		if enabled(tool) {
			t.Fatalf("%s stayed enabled under a quarantined server", tool)
		}
	}
	observeAll()
	for _, tool := range all {
		if enabled(tool) {
			t.Fatalf("an observation re-enabled %s under a quarantined server", tool)
		}
	}
}

// A non-admin who shares a user-scoped server does not carry the owner's trust
// onto it; an admin does, and a server that was already shared keeps what an
// admin gave it when its owner re-registers it.
func TestSharingAUserServerByANonAdminDropsItsStoredTrustAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	seedMCPServer(t, pool, "org-a", "own", "user", `{"tool_allowlist":["a"],"owner_user_id":"user-a","annotation_trust":true}`)
	seedMCPServer(t, pool, "org-a", "own-admin", "user", `{"tool_allowlist":["a"],"owner_user_id":"user-a","annotation_trust":true}`)
	seedMCPServer(t, pool, "org-a", "own-shared", "user", `{"tool_allowlist":["a"],"owner_user_id":"user-a","shared_with":["user-c"],"annotation_trust":true}`)
	mcp := NewMCPHandler(pool)
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	nonZDR := false
	nonAdmin := sign("user", []string{authz.WriteScope}, &nonZDR)
	admin := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)

	body := func(name, sharedWith string) string {
		return `{"name":"` + name + `","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"user","config_json":{"tool_allowlist":["a"],"owner_user_id":"user-a","shared_with":` + sharedWith + `}}`
	}
	integrationRegister(t, handler, nonAdmin, body("name-own", `[]`))
	if !integrationTrusted(t, pool, "own") {
		t.Fatal("a re-registration that shares with nobody dropped the owner's trust")
	}
	integrationRegister(t, handler, nonAdmin, body("name-own", `["user-b"]`))
	if integrationTrusted(t, pool, "own") {
		t.Fatal("a non-admin shared a trusted user server and kept its trust")
	}
	integrationRegister(t, handler, admin, body("name-own-admin", `["user-b"]`))
	if !integrationTrusted(t, pool, "own-admin") {
		t.Fatal("an admin sharing the server lost its trust")
	}
	integrationRegister(t, handler, nonAdmin, body("name-own-shared", `["user-c","user-b"]`))
	if !integrationTrusted(t, pool, "own-shared") {
		t.Fatal("a server that was already shared lost the trust it had when its owner re-registered it")
	}
}
