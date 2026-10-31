package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// knownAnswerToolDigest is the digest model-gateway computes for the `echo_read`
// tool of its own known-answer test (mcp_jsonrpc.rs, ECHO_READ_DIGEST). It is
// here so that the value the Rust side produces is, byte for byte, one this side
// accepts and stores; the registry never recomputes a digest, only compares.
const (
	knownAnswerToolDigest       = "aad57097d17d7540d459e94756b10a2b703a9281b3988441933a3ac682bc8f95"
	knownAnswerDescriptionSHA   = "154d65d3a5b2b5e71b01b1e62a7417b232c56cc2261e14bd1cdd4b8861ac43ac"
	knownAnswerSchemaSHA        = "e3dce8d1afb8d03604af3bc6729219a5cd7df05558807b6e44e391d56fdf75d9"
	changedToolDigest           = "1111111111111111111111111111111111111111111111111111111111111111"
	anotherChangedToolDigest    = "2222222222222222222222222222222222222222222222222222222222222222"
	observeRootSecret           = "mcp-oauth-service-token-test-vector"
	observeOrg                  = "org-a"
	observeServerID             = "srv"
	observeServerAllowlistTools = `["echo_read","create_item","drop_all"]`
)

// ---------------------------------------------------------------------------
// A registry stand-in that answers the observation and approval statements the
// way Postgres would, so the handler's reads, decisions and compare-and-set
// writes can be followed through whole sequences (first sight, drift, approval)
// without a database. The SQL text itself is exercised by the integration test.
// ---------------------------------------------------------------------------

type fakeToolRow struct {
	exists       bool
	capabilityID string
	class        string
	floor        string
	pinned       string
	observed     string
	state        string
	declared     string
	source       string
}

type fakeCapabilityRow struct {
	enabled    bool
	version    string
	risk       string
	reasonCode string
	class      string
}

type fakeServerRow struct {
	enabled bool
	rollout string
	scope   string
	config  map[string]any
}

type toolRegistry struct {
	recordingDatabase
	servers map[string]fakeServerRow
	tools   map[string]*fakeToolRow // keyed by tool name (one server in these tests)
	caps    map[string]fakeCapabilityRow
	audits  []string
	applies []databaseCall
	// contend makes every observation write lose its compare-and-set.
	contend bool
	// failApply makes the write itself fail.
	failApply error
	// beforeApply runs at the start of the nth observation write, and
	// beforeApprove at the start of an approval write: a stand-in for something
	// else (a PATCH, a DELETE) landing between the handler's reads and its write.
	beforeApply   func(n int)
	beforeApprove func()
}

// serverLive mirrors the `srv` CTE of the write statements: the server row as it
// is when the write runs, enabled, stable or canary, and still listing the tool.
// The SQL itself is pinned by TestToolWriteStatementsDecideEnabledAgainstTheServerRowAsItIsNow
// and run by the integration test; this keeps the sequences below honest about
// what the statement is meant to decide.
func (r *toolRegistry) serverLive(serverID, tool string) bool {
	server, ok := r.servers[serverID]
	if !ok || !server.enabled || (server.rollout != "stable" && server.rollout != "canary") {
		return false
	}
	allowlist, _ := server.config["tool_allowlist"].([]string)
	for _, allowed := range allowlist {
		if allowed == tool {
			return true
		}
	}
	return false
}

func newToolRegistry() *toolRegistry {
	return &toolRegistry{
		servers: map[string]fakeServerRow{},
		tools:   map[string]*fakeToolRow{},
		caps:    map[string]fakeCapabilityRow{},
	}
}

func (r *toolRegistry) addServer(id, scope string, trusted bool, allowlist ...string) {
	config := map[string]any{"tool_allowlist": allowlist}
	if trusted {
		config["annotation_trust"] = true
	}
	if scope == "user" {
		config["owner_user_id"] = "user-a"
	}
	r.servers[id] = fakeServerRow{enabled: true, rollout: "stable", scope: scope, config: config}
}

// values scans a fixed list of already-known values into Scan destinations.
type valuesRow []any

func (row valuesRow) Scan(dest ...any) error {
	if len(dest) != len(row) {
		return fmt.Errorf("valuesRow: %d destinations for %d values", len(dest), len(row))
	}
	for i, value := range row {
		switch target := dest[i].(type) {
		case *string:
			*target = value.(string)
		case *bool:
			*target = value.(bool)
		case *int64:
			*target = value.(int64)
		case *any:
			*target = value
		case **time.Time:
			*target = nil
		default:
			return fmt.Errorf("valuesRow: unsupported destination %T", dest[i])
		}
	}
	return nil
}

func (r *toolRegistry) QueryRow(ctx context.Context, query string, args ...any) pgx.Row {
	switch {
	case strings.Contains(query, "FROM mcp_servers") && strings.Contains(query, "SELECT enabled, rollout_state, config_json"):
		server, ok := r.servers[args[0].(string)]
		if !ok {
			return errorRow{err: pgx.ErrNoRows}
		}
		return valuesRow{server.enabled, server.rollout, any(server.config)}
	case strings.Contains(query, "FROM mcp_servers") && strings.Contains(query, "SELECT scope, enabled, rollout_state, config_json"):
		server, ok := r.servers[args[0].(string)]
		if !ok {
			return errorRow{err: pgx.ErrNoRows}
		}
		return valuesRow{server.scope, server.enabled, server.rollout, any(server.config)}
	case strings.Contains(query, "FROM capability_tools") && strings.Contains(query, "capability_id=$2 AND tool_name=$3"):
		row := r.tools[args[2].(string)]
		if row == nil || !row.exists {
			return errorRow{err: pgx.ErrNoRows}
		}
		return valuesRow{row.class, row.floor, row.pinned, row.observed, row.state}
	case strings.Contains(query, "FROM capability_tools") && strings.Contains(query, "AND server_id=$2 AND tool_name=$3"):
		row := r.tools[args[2].(string)]
		if row == nil || !row.exists {
			return errorRow{err: pgx.ErrNoRows}
		}
		return valuesRow{row.capabilityID, row.class, row.floor, row.pinned, row.observed, row.state, row.declared, row.source}
	case query == applyObservationSQL:
		return r.apply(args)
	case query == approveToolSQL:
		return r.approve(args)
	default:
		return r.recordingDatabase.QueryRow(ctx, query, args...)
	}
}

func (r *toolRegistry) apply(args []any) pgx.Row {
	r.applies = append(r.applies, databaseCall{query: applyObservationSQL, args: append([]any(nil), args...)})
	if r.beforeApply != nil {
		r.beforeApply(len(r.applies))
	}
	if r.failApply != nil {
		return errorRow{err: r.failApply}
	}
	if r.contend {
		return valuesRow{int64(0)}
	}
	toolName := args[2].(string)
	row := r.tools[toolName]
	if row != nil && row.exists {
		// The compare-and-set: the write holds only if the row still is what the
		// decision was made from, and only for a derived row.
		if row.source != "derived" || row.pinned != args[15].(string) || row.observed != args[16].(string) ||
			row.state != args[17].(string) || row.class != args[18].(string) {
			return valuesRow{int64(0)}
		}
	}
	r.tools[toolName] = &fakeToolRow{
		exists: true, capabilityID: args[1].(string), class: args[3].(string), floor: args[4].(string),
		pinned: args[7].(string), observed: args[8].(string), declared: args[11].(string),
		state: args[13].(string), source: "derived",
	}
	enabled, reasonCode := args[21].(bool), args[23].(string)
	if enabled && !r.serverLive(args[5].(string), toolName) {
		enabled, reasonCode = false, "capability_disabled"
	}
	r.caps[args[1].(string)] = fakeCapabilityRow{
		version: args[19].(string), risk: args[20].(string), enabled: enabled,
		reasonCode: reasonCode, class: args[3].(string),
	}
	if action := args[29].(string); action != "" {
		r.audits = append(r.audits, action)
	}
	return valuesRow{int64(1)}
}

func (r *toolRegistry) approve(args []any) pgx.Row {
	if r.beforeApprove != nil {
		r.beforeApprove()
	}
	toolName := args[2].(string)
	row := r.tools[toolName]
	if r.contend || row == nil || row.source != "derived" || row.pinned != args[5].(string) ||
		row.observed != args[6].(string) || row.state != args[7].(string) || row.class != args[8].(string) {
		return valuesRow{int64(0)}
	}
	row.class, row.floor, row.pinned, row.state = args[3].(string), args[4].(string), row.observed, reviewApproved
	enabled, reasonCode := args[11].(bool), args[13].(string)
	if enabled && !r.serverLive(args[21].(string), toolName) {
		enabled, reasonCode = false, "capability_disabled"
	}
	r.caps[args[1].(string)] = fakeCapabilityRow{
		version: args[9].(string), risk: args[10].(string), enabled: enabled,
		reasonCode: reasonCode, class: args[3].(string),
	}
	r.audits = append(r.audits, args[18].(string))
	return valuesRow{int64(1)}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func observationHandler(t *testing.T, registry *toolRegistry, rootSecret string) *MCPHandler {
	t.Helper()
	mcp := NewMCPHandler(nil).WithMCPServiceToken(rootSecret)
	mcp.pool = registry
	return mcp
}

func observationBody(observations ...string) string {
	return `{"observations":[` + strings.Join(observations, ",") + `]}`
}

func observationJSON(tool, declared, digest string, hints string) string {
	if hints == "" {
		hints = `{"read_only_hint":null,"destructive_hint":null,"idempotent_hint":null,"open_world_hint":null}`
	}
	return fmt.Sprintf(`{"server_id":%q,"tool_name":%q,"qualified_name":%q,"digest":%q,"description_sha256":%q,"schema_sha256":%q,"declared_side_effect":%q,"annotations":%s}`,
		observeServerID, tool, "mcp__"+observeServerID+"__"+tool, digest, knownAnswerDescriptionSHA, knownAnswerSchemaSHA, declared, hints)
}

type observeReply struct {
	Results []observationResult `json:"results"`
}

func postObservation(t *testing.T, mcp *MCPHandler, org, token, body string) (*httptest.ResponseRecorder, observeReply) {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/internal/mcp/tools/observe?org_id="+org, strings.NewReader(body))
	if token != "" {
		request.Header.Set(mcpServiceTokenHeader, token)
	}
	response := httptest.NewRecorder()
	mcp.observeToolsInternal(response, request)
	var reply observeReply
	if response.Code == http.StatusOK {
		if err := json.Unmarshal(response.Body.Bytes(), &reply); err != nil {
			t.Fatalf("decode reply: %v (%s)", err, response.Body.String())
		}
	}
	return response, reply
}

func observe(t *testing.T, mcp *MCPHandler, observations ...string) observeReply {
	t.Helper()
	response, reply := postObservation(t, mcp, observeOrg, expectedInternalServiceToken(observeRootSecret, observeOrg), observationBody(observations...))
	if response.Code != http.StatusOK {
		t.Fatalf("observe status = %d, body=%s", response.Code, response.Body.String())
	}
	return reply
}

func onlyResult(t *testing.T, reply observeReply) observationResult {
	t.Helper()
	if len(reply.Results) != 1 {
		t.Fatalf("results = %+v, want exactly one", reply.Results)
	}
	return reply.Results[0]
}

// ---------------------------------------------------------------------------
// decideObservation: the rules, one at a time
// ---------------------------------------------------------------------------

func registeredServer(trusted bool, allowlist ...string) observedServer {
	server := observedServer{registered: true, enabled: true, trusted: trusted, allowlist: map[string]struct{}{}}
	for _, tool := range allowlist {
		server.allowlist[tool] = struct{}{}
	}
	return server
}

func observationOf(tool, declared, digest string) toolObservation {
	return toolObservation{
		ServerID: observeServerID, ToolName: tool, QualifiedName: "mcp__" + observeServerID + "__" + tool,
		Digest: digest, DescriptionSHA256: knownAnswerDescriptionSHA, SchemaSHA256: knownAnswerSchemaSHA,
		DeclaredSideEffect: declared,
	}
}

func TestFirstSightPinsTheDigestAndApprovesOnlyARegisteredAllowlistedTool(t *testing.T) {
	obs := observationOf("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest)

	t.Run("registered and allowlisted", func(t *testing.T) {
		d := decideObservation(storedTool{}, obs, registeredServer(false, "echo_read"))
		if d.state != reviewApproved || d.pinned != knownAnswerToolDigest || !d.enabled || d.reasonCode != "" || d.audit != auditActionToolObserved {
			t.Fatalf("decision = %+v", d)
		}
	})
	t.Run("server not registered", func(t *testing.T) {
		d := decideObservation(storedTool{}, obs, observedServer{})
		if d.state != reviewPending || d.pinned != knownAnswerToolDigest || d.enabled || d.reasonCode != models.ReasonToolNotApproved {
			t.Fatalf("decision = %+v", d)
		}
	})
	t.Run("not on the allowlist", func(t *testing.T) {
		d := decideObservation(storedTool{}, obs, registeredServer(false, "something_else"))
		if d.state != reviewPending || d.enabled {
			t.Fatalf("decision = %+v", d)
		}
	})
	t.Run("approved but the server is off", func(t *testing.T) {
		server := registeredServer(false, "echo_read")
		server.enabled = false
		d := decideObservation(storedTool{}, obs, server)
		if d.state != reviewApproved || d.enabled || d.reasonCode != "capability_disabled" {
			t.Fatalf("decision = %+v", d)
		}
	})
}

func TestUntrustedReadOnlyClaimDerivesAnExternalWriteAndSilenceDerivesUnclassified(t *testing.T) {
	cases := []struct {
		declared string
		trusted  bool
		class    string
		floor    string
		risk     string
	}{
		{models.DeclaredReadOnly, false, models.SideEffectExternalWrite, models.ApprovalFloorNone, models.RiskHigh},
		{models.DeclaredReadOnly, true, models.SideEffectRead, models.ApprovalFloorNone, models.RiskLow},
		{models.DeclaredNonDestructiveWrite, false, models.SideEffectExternalWrite, models.ApprovalFloorNone, models.RiskHigh},
		{models.DeclaredNonDestructiveWrite, true, models.SideEffectExternalWrite, models.ApprovalFloorNone, models.RiskHigh},
		{models.DeclaredDestructive, false, models.SideEffectDestructive, models.ApprovalFloorNone, models.RiskHigh},
		{models.DeclaredDestructive, true, models.SideEffectDestructive, models.ApprovalFloorNone, models.RiskHigh},
		{models.DeclaredUndeclared, false, models.SideEffectUnclassified, models.ApprovalFloorAsk, models.RiskHigh},
		{models.DeclaredUndeclared, true, models.SideEffectUnclassified, models.ApprovalFloorAsk, models.RiskHigh},
	}
	for _, c := range cases {
		name := fmt.Sprintf("%s trusted=%v", c.declared, c.trusted)
		t.Run(name, func(t *testing.T) {
			d := decideObservation(storedTool{}, observationOf("t", c.declared, knownAnswerToolDigest), registeredServer(c.trusted, "t"))
			if d.class != c.class || d.floor != c.floor {
				t.Fatalf("class/floor = %s/%s, want %s/%s", d.class, d.floor, c.class, c.floor)
			}
			if got := riskLevelForClass(d.class); got != c.risk {
				t.Fatalf("risk = %s, want %s", got, c.risk)
			}
		})
	}
}

func TestADigestThatDiffersFromThePinDriftsTheToolAndLeavesItsClassAlone(t *testing.T) {
	stored := storedTool{exists: true, class: models.SideEffectExternalWrite, floor: models.ApprovalFloorNone,
		pinned: knownAnswerToolDigest, observed: knownAnswerToolDigest, state: reviewApproved}
	// The new definition claims to be a pure read from a server that is now trusted:
	// none of that may take effect while the tool is drifted.
	obs := observationOf("echo_read", models.DeclaredReadOnly, changedToolDigest)
	d := decideObservation(stored, obs, registeredServer(true, "echo_read"))
	if d.state != reviewDrifted || d.enabled || d.reasonCode != models.ReasonToolDefinitionChanged {
		t.Fatalf("decision = %+v", d)
	}
	if d.class != models.SideEffectExternalWrite || d.pinned != knownAnswerToolDigest {
		t.Fatalf("drift changed the class or moved the pin: %+v", d)
	}
	if d.audit != auditActionToolDrifted {
		t.Fatalf("audit = %q, want %q", d.audit, auditActionToolDrifted)
	}

	// The same drifted digest seen again is a refresh, not a new event.
	again := storedTool{exists: true, class: d.class, floor: d.floor, pinned: d.pinned, observed: changedToolDigest, state: reviewDrifted}
	d2 := decideObservation(again, obs, registeredServer(true, "echo_read"))
	if d2.state != reviewDrifted || d2.audit != "" {
		t.Fatalf("a repeated drifted digest: %+v", d2)
	}
	// A further change is a new event.
	d3 := decideObservation(again, observationOf("echo_read", models.DeclaredReadOnly, anotherChangedToolDigest), registeredServer(true, "echo_read"))
	if d3.audit != auditActionToolDrifted {
		t.Fatalf("a second change was not audited: %+v", d3)
	}
}

func TestObservationNeverLowersAClassOnlyRaisesIt(t *testing.T) {
	base := storedTool{exists: true, floor: models.ApprovalFloorNone, pinned: knownAnswerToolDigest, observed: knownAnswerToolDigest, state: reviewApproved}

	t.Run("a trusted read-only claim does not lower an external write", func(t *testing.T) {
		stored := base
		stored.class = models.SideEffectExternalWrite
		d := decideObservation(stored, observationOf("t", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(true, "t"))
		if d.class != models.SideEffectExternalWrite {
			t.Fatalf("class = %s, observation lowered it", d.class)
		}
	})
	t.Run("withdrawing trust raises a read to an external write", func(t *testing.T) {
		stored := base
		stored.class = models.SideEffectRead
		d := decideObservation(stored, observationOf("t", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(false, "t"))
		if d.class != models.SideEffectExternalWrite || d.audit != auditActionToolObserved {
			t.Fatalf("decision = %+v", d)
		}
	})
	t.Run("an unclassified tool stays unclassified when the server later claims read-only", func(t *testing.T) {
		stored := base
		stored.class = models.SideEffectUnclassified
		stored.floor = models.ApprovalFloorAsk
		d := decideObservation(stored, observationOf("t", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(true, "t"))
		if d.class != models.SideEffectUnclassified || d.floor != models.ApprovalFloorAsk {
			t.Fatalf("decision = %+v", d)
		}
	})
	t.Run("the floor never falls below what is stored or what the class implies", func(t *testing.T) {
		stored := base
		stored.class = models.SideEffectRead
		stored.floor = models.ApprovalFloorAsk
		d := decideObservation(stored, observationOf("t", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(true, "t"))
		if d.floor != models.ApprovalFloorAsk {
			t.Fatalf("floor = %s, observation lowered it", d.floor)
		}
	})
	t.Run("every stored class survives every declared claim from a trusted or untrusted server", func(t *testing.T) {
		for _, class := range []string{models.SideEffectRead, models.SideEffectSandboxCompute, models.SideEffectContextWrite,
			models.SideEffectExternalWrite, models.SideEffectDestructive, models.SideEffectOwnerEffect, models.SideEffectUnclassified} {
			for _, declared := range []string{models.DeclaredReadOnly, models.DeclaredNonDestructiveWrite, models.DeclaredDestructive, models.DeclaredUndeclared} {
				for _, trusted := range []bool{false, true} {
					stored := base
					stored.class = class
					stored.floor = models.MinimumApprovalFloor(class)
					d := decideObservation(stored, observationOf("t", declared, knownAnswerToolDigest), registeredServer(trusted, "t"))
					if models.SideEffectClassRank(d.class) < models.SideEffectClassRank(class) {
						t.Fatalf("stored %s with %s trusted=%v was lowered to %s", class, declared, trusted, d.class)
					}
					if models.ApprovalFloorRank(d.floor) < models.ApprovalFloorRank(stored.floor) {
						t.Fatalf("stored floor %s was lowered to %s", stored.floor, d.floor)
					}
				}
			}
		}
	})
}

func TestAPendingToolIsApprovedOnItsOwnPinOnceItsServerQualifies(t *testing.T) {
	stored := storedTool{exists: true, class: models.SideEffectExternalWrite, floor: models.ApprovalFloorNone,
		pinned: knownAnswerToolDigest, observed: knownAnswerToolDigest, state: reviewPending}
	obs := observationOf("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest)

	if d := decideObservation(stored, obs, observedServer{}); d.state != reviewPending {
		t.Fatalf("still unregistered: %+v", d)
	}
	d := decideObservation(stored, obs, registeredServer(false, "echo_read"))
	if d.state != reviewApproved || !d.enabled || d.audit != auditActionToolObserved {
		t.Fatalf("now registered and allowlisted: %+v", d)
	}
	// A different digest is never taken on trust, however the server qualifies.
	moved := decideObservation(stored, observationOf("echo_read", models.DeclaredReadOnly, changedToolDigest), registeredServer(false, "echo_read"))
	if moved.state != reviewDrifted {
		t.Fatalf("a changed digest on a pending tool: %+v", moved)
	}
}

func TestADriftedToolStaysDriftedWhateverDigestIsReported(t *testing.T) {
	stored := storedTool{exists: true, class: models.SideEffectExternalWrite, floor: models.ApprovalFloorNone,
		pinned: knownAnswerToolDigest, observed: changedToolDigest, state: reviewDrifted}
	// The digest equal to the pin is what a replica with a stale tool cache reports
	// too, so it is not evidence the server is back to what a person approved.
	d := decideObservation(stored, observationOf("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(false, "echo_read"))
	if d.state != reviewDrifted || d.enabled || d.reasonCode != models.ReasonToolDefinitionChanged ||
		d.pinned != knownAnswerToolDigest || d.class != models.SideEffectExternalWrite {
		t.Fatalf("decision = %+v", d)
	}
	if !d.unchanged || d.audit != "" {
		t.Fatalf("a drifted tool seen with the pinned digest must be left exactly as it is: %+v", d)
	}
	// A server that qualifies in every other way changes nothing either.
	if d := decideObservation(stored, observationOf("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest), registeredServer(true, "echo_read")); d.state != reviewDrifted || d.enabled {
		t.Fatalf("a trusted, allowlisted server restored a drifted tool: %+v", d)
	}
}

func TestAStaleReplicaReportingTheOldDigestCannotRestoreADriftedTool(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)

	// Replica A sees the original definition, replica B (a fresher cache) sees the change.
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	if result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))); result.ReviewState != reviewDrifted {
		t.Fatalf("setup: %+v", result)
	}
	drifted := *registry.tools["echo_read"]
	driftedCapability := registry.caps[drifted.capabilityID]
	writes, audits := len(registry.applies), len(registry.audits)

	// Replica A, still holding the old definition, posts the old digest after the drift.
	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if result.Outcome != outcomeRecorded || result.ReviewState != reviewDrifted {
		t.Fatalf("stale replica result = %+v, want the tool still drifted", result)
	}
	row := *registry.tools["echo_read"]
	if row != drifted {
		t.Fatalf("a stale replica rewrote a drifted row: %+v -> %+v", drifted, row)
	}
	if registry.caps[row.capabilityID] != driftedCapability || registry.caps[row.capabilityID].enabled {
		t.Fatalf("a stale replica changed or re-enabled the capability: %+v", registry.caps[row.capabilityID])
	}
	if len(registry.applies) != writes || len(registry.audits) != audits {
		t.Fatalf("a stale replica wrote: writes %d -> %d, audits %v", writes, len(registry.applies), registry.audits)
	}
	if row.observed != changedToolDigest {
		t.Fatalf("the digest a person is about to review was replaced by the stale one: %s", row.observed)
	}

	// Only a person approving what is observed moves the row.
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	if response := approve(t, handler, sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR), observeServerID, "echo_read"); response.Code != http.StatusOK {
		t.Fatalf("approval status = %d, body=%s", response.Code, response.Body.String())
	}
	if registry.tools["echo_read"].state != reviewApproved || !registry.caps[drifted.capabilityID].enabled {
		t.Fatalf("approval did not restore the tool: %+v", registry.tools["echo_read"])
	}
}

func TestMCPToolCapabilityIDsAreStablePerOrgAndNeverShared(t *testing.T) {
	a := mcpToolCapabilityID("org-a", "mcp__srv__echo_read")
	if a != mcpToolCapabilityID("org-a", "mcp__srv__echo_read") {
		t.Fatal("the id is not a function of the org and name")
	}
	if a == mcpToolCapabilityID("org-b", "mcp__srv__echo_read") {
		t.Fatal("two orgs share a tool row id")
	}
	if a == mcpToolCapabilityID("org-a", "mcp__srv__create_item") {
		t.Fatal("two tools share a row id")
	}
	if !strings.HasPrefix(a, "cap.mcp_tool.") || len(a) != len("cap.mcp_tool.")+32 {
		t.Fatalf("id = %q", a)
	}
	if mcpToolVersion(knownAnswerToolDigest) == mcpToolVersion(changedToolDigest) {
		t.Fatal("a re-pinned tool kept its version")
	}
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

func TestObservationValidationRejectsWhatCannotBeAddressedUnambiguously(t *testing.T) {
	good := observationOf("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest)
	if reason := validateObservation(good); reason != "" {
		t.Fatalf("a good observation was rejected: %s", reason)
	}
	cases := map[string]func(o *toolObservation){
		"invalid_server_id":            func(o *toolObservation) { o.ServerID = "bad server" },
		"ambiguous_server_id":          func(o *toolObservation) { o.ServerID = "a__b"; o.QualifiedName = "" },
		"invalid_tool_name":            func(o *toolObservation) { o.ToolName = "no spaces"; o.QualifiedName = "" },
		"qualified_name_mismatch":      func(o *toolObservation) { o.QualifiedName = "mcp__other__echo_read" },
		"invalid_digest":               func(o *toolObservation) { o.Digest = strings.ToUpper(knownAnswerToolDigest) },
		"invalid_hash":                 func(o *toolObservation) { o.SchemaSHA256 = "short" },
		"invalid_declared_side_effect": func(o *toolObservation) { o.DeclaredSideEffect = "harmless" },
	}
	for want, mutate := range cases {
		o := good
		mutate(&o)
		if got := validateObservation(o); got != want {
			t.Errorf("reason = %q, want %q", got, want)
		}
	}
}

func TestMCPRegistrationRejectsADoubleUnderscoreServerID(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	body := `{"id":"my__server","name":"tenant-mcp","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","config_json":{"tool_allowlist":["records.search"]}}`
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a rejected server id reached the database")
	}

	// A single underscore is fine, and so is an id the registry generates.
	database.expectUpsertReturningID(t, "mcp_ok")
	body = strings.Replace(body, "my__server", "my_server", 1)
	request = httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response = httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", response.Code, response.Body.String())
	}
}

// ---------------------------------------------------------------------------
// The observation route
// ---------------------------------------------------------------------------

func TestObservationRouteIsGatedByTheOrgDerivedCredentialAndNothingElse(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	body := observationBody(observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))

	for _, c := range []struct {
		name  string
		org   string
		token string
		want  int
	}{
		{"no credential", observeOrg, "", http.StatusForbidden},
		{"wrong secret", observeOrg, "guess", http.StatusForbidden},
		{"the raw root secret is not a credential", observeOrg, observeRootSecret, http.StatusForbidden},
		{"a credential minted for another org", observeOrg, expectedInternalServiceToken(observeRootSecret, "org-b"), http.StatusForbidden},
		{"a valid credential replayed against another org", "org-b", expectedInternalServiceToken(observeRootSecret, observeOrg), http.StatusForbidden},
		{"the global org is never writable", "global", expectedInternalServiceToken(observeRootSecret, "global"), http.StatusForbidden},
		{"no org at all", "", "", http.StatusBadRequest},
	} {
		t.Run(c.name, func(t *testing.T) {
			before := registry.databaseCalls() + len(registry.applies)
			response, _ := postObservation(t, mcp, c.org, c.token, body)
			if response.Code != c.want {
				t.Fatalf("status = %d, want %d (%s)", response.Code, c.want, response.Body.String())
			}
			if registry.databaseCalls()+len(registry.applies) != before {
				t.Fatal("a refused observation reached the database")
			}
		})
	}

	t.Run("an unconfigured root secret keeps the route closed even for its own derivation", func(t *testing.T) {
		closed := observationHandler(t, registry, "")
		response, _ := postObservation(t, closed, observeOrg, expectedInternalServiceToken("", observeOrg), body)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403", response.Code)
		}
	})
	t.Run("only POST", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/internal/mcp/tools/observe?org_id="+observeOrg, nil)
		response := httptest.NewRecorder()
		mcp.observeToolsInternal(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, want 404", response.Code)
		}
	})
	t.Run("the credential works", func(t *testing.T) {
		response, reply := postObservation(t, mcp, observeOrg, expectedInternalServiceToken(observeRootSecret, observeOrg), body)
		if response.Code != http.StatusOK || len(reply.Results) != 1 || reply.Results[0].Outcome != outcomeRecorded {
			t.Fatalf("status = %d reply = %+v", response.Code, reply)
		}
	})
}

func TestObservationBodyIsStrictAndBounded(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	token := expectedInternalServiceToken(observeRootSecret, observeOrg)
	good := observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")

	for _, c := range []struct {
		name string
		body string
		want int
	}{
		{"malformed", `{`, http.StatusBadRequest},
		{"empty batch", `{"observations":[]}`, http.StatusBadRequest},
		{"unknown top-level field", `{"observations":[` + good + `],"org_id":"org-b"}`, http.StatusBadRequest},
		{"a description is not accepted", strings.Replace(observationBody(good), `"digest"`, `"description":"Sends mail","digest"`, 1), http.StatusBadRequest},
		{"a schema is not accepted", strings.Replace(observationBody(good), `"digest"`, `"input_schema":{"type":"object"},"digest"`, 1), http.StatusBadRequest},
		{"a title is not accepted", strings.Replace(observationBody(good), `"read_only_hint"`, `"title":"x","read_only_hint"`, 1), http.StatusBadRequest},
		{"two documents", observationBody(good) + observationBody(good), http.StatusBadRequest},
		{"over the batch bound", observationBody(strings.Split(strings.Repeat(good+"~", maxObservationsPerBatch+1), "~")[:maxObservationsPerBatch+1]...), http.StatusRequestEntityTooLarge},
		{"over the body bound", `{"observations":[` + good + `],"pad":"` + strings.Repeat("a", maxObservationBodyBytes) + `"}`, http.StatusBadRequest},
	} {
		t.Run(c.name, func(t *testing.T) {
			before := len(registry.applies)
			response, _ := postObservation(t, mcp, observeOrg, token, c.body)
			if response.Code != c.want {
				t.Fatalf("status = %d, want %d (%s)", response.Code, c.want, response.Body.String())
			}
			if len(registry.applies) != before {
				t.Fatal("a refused batch wrote")
			}
		})
	}
}

func TestInvalidObservationsAreRejectedPerToolAndTheRestStillRecorded(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	bad := strings.Replace(observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""), observeServerID, "bad__id", 1)
	reply := observe(t, mcp,
		bad,
		observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
	)
	if len(reply.Results) != 2 {
		t.Fatalf("results = %+v", reply.Results)
	}
	if reply.Results[0].Outcome != outcomeRejected || reply.Results[0].Reason == "" || reply.Results[0].QualifiedName != "" {
		t.Fatalf("the invalid tool: %+v", reply.Results[0])
	}
	if reply.Results[1].Outcome != outcomeRecorded {
		t.Fatalf("the valid tool: %+v", reply.Results[1])
	}
	if len(registry.applies) != 1 {
		t.Fatalf("writes = %d, want only the valid tool's", len(registry.applies))
	}
}

func TestFixtureServerObservedFirstTimeYieldsThreeRowsWithTheExpectedClasses(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read", "create_item", "drop_all")
	mcp := observationHandler(t, registry, observeRootSecret)

	reply := observe(t, mcp,
		observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, `{"read_only_hint":true,"destructive_hint":null,"idempotent_hint":null,"open_world_hint":null}`),
		observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, `{"read_only_hint":false,"destructive_hint":null,"idempotent_hint":null,"open_world_hint":null}`),
		observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
	)
	want := map[string]struct{ class, floor string }{
		"echo_read":   {models.SideEffectExternalWrite, models.ApprovalFloorNone}, // untrusted read-only claim
		"create_item": {models.SideEffectDestructive, models.ApprovalFloorNone},
		"drop_all":    {models.SideEffectUnclassified, models.ApprovalFloorAsk},
	}
	for _, result := range reply.Results {
		if result.Outcome != outcomeRecorded || result.ReviewState != reviewApproved {
			t.Fatalf("result = %+v", result)
		}
	}
	for tool, expected := range want {
		row := registry.tools[tool]
		if row == nil || row.class != expected.class || row.floor != expected.floor || row.state != reviewApproved {
			t.Fatalf("%s row = %+v, want %+v approved", tool, row, expected)
		}
		capability := registry.caps[row.capabilityID]
		if !capability.enabled || capability.risk != models.RiskHigh {
			t.Fatalf("%s capability = %+v, want enabled and high risk", tool, capability)
		}
	}
	if got := strings.Join(registry.audits, ","); got != strings.Repeat(auditActionToolObserved+",", 2)+auditActionToolObserved {
		t.Fatalf("audit = %s, want three %s", got, auditActionToolObserved)
	}
}

func TestATrustedServersReadOnlyToolDerivesReadAndLowRisk(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", true, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if result.SideEffectClass != models.SideEffectRead {
		t.Fatalf("class = %s, want read", result.SideEffectClass)
	}
	row := registry.tools["echo_read"]
	if capability := registry.caps[row.capabilityID]; capability.risk != models.RiskLow || !capability.enabled {
		t.Fatalf("capability = %+v", capability)
	}
}

func TestDriftThenApprovalRepinsAndReenablesTheTool(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)

	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	original := registry.tools["echo_read"]
	pinnedVersion := registry.caps[original.capabilityID].version

	// The server changes the tool's description: the digest moves.
	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, "")))
	if result.ReviewState != reviewDrifted {
		t.Fatalf("result = %+v", result)
	}
	row := registry.tools["echo_read"]
	capability := registry.caps[row.capabilityID]
	if row.pinned != knownAnswerToolDigest || row.observed != changedToolDigest || row.class != models.SideEffectExternalWrite {
		t.Fatalf("row after drift = %+v", row)
	}
	if capability.enabled || capability.reasonCode != models.ReasonToolDefinitionChanged || capability.version != pinnedVersion {
		t.Fatalf("capability after drift = %+v", capability)
	}
	if registry.audits[len(registry.audits)-1] != auditActionToolDrifted {
		t.Fatalf("audit = %v", registry.audits)
	}

	// Observing the same drifted digest again changes nothing and adds no audit row.
	auditsBefore := len(registry.audits)
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))
	if len(registry.audits) != auditsBefore {
		t.Fatalf("a repeated drifted digest was audited again: %v", registry.audits)
	}

	// An admin approves the new definition.
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	response := approve(t, handler, sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR), observeServerID, "echo_read")
	if response.Code != http.StatusOK {
		t.Fatalf("approve status = %d, body=%s", response.Code, response.Body.String())
	}
	row = registry.tools["echo_read"]
	capability = registry.caps[row.capabilityID]
	if row.state != reviewApproved || row.pinned != changedToolDigest {
		t.Fatalf("row after approval = %+v", row)
	}
	if !capability.enabled || capability.reasonCode != "" || capability.version == pinnedVersion {
		t.Fatalf("capability after approval = %+v (old version %s)", capability, pinnedVersion)
	}
	if registry.audits[len(registry.audits)-1] != auditActionToolApproved {
		t.Fatalf("audit = %v", registry.audits)
	}

	// The approved digest is now what the next observation matches.
	result = onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, "")))
	if result.ReviewState != reviewApproved {
		t.Fatalf("result after approval = %+v", result)
	}
}

func TestObservationOfAnUnregisteredServerRecordsPendingAndNothingIsCallable(t *testing.T) {
	registry := newToolRegistry() // no server registered
	mcp := observationHandler(t, registry, observeRootSecret)
	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if result.ReviewState != reviewPending {
		t.Fatalf("result = %+v", result)
	}
	capability := registry.caps[registry.tools["echo_read"].capabilityID]
	if capability.enabled || capability.reasonCode != models.ReasonToolNotApproved {
		t.Fatalf("capability = %+v", capability)
	}

	// Once the server exists with the tool allowlisted, the same digest approves.
	registry.addServer(observeServerID, "org", false, "echo_read")
	result = onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if result.ReviewState != reviewApproved || !registry.caps[registry.tools["echo_read"].capabilityID].enabled {
		t.Fatalf("result = %+v", result)
	}
}

func TestAnObservationThatLosesTheRaceIsReportedContendedNotLost(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	registry.contend = true
	mcp := observationHandler(t, registry, observeRootSecret)
	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if result.Outcome != outcomeContended {
		t.Fatalf("result = %+v", result)
	}
	if len(registry.applies) != observationRetries {
		t.Fatalf("attempts = %d, want %d", len(registry.applies), observationRetries)
	}
}

func TestObservationDatabaseFailureIsAGenericServerError(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	registry.failApply = errors.New("postgres connection refused")
	mcp := observationHandler(t, registry, observeRootSecret)
	response, _ := postObservation(t, mcp, observeOrg, expectedInternalServiceToken(observeRootSecret, observeOrg),
		observationBody(observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, "")))
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status = %d, want 500", response.Code)
	}
	if strings.Contains(response.Body.String(), "postgres connection refused") {
		t.Fatalf("response leaked the database error: %s", response.Body.String())
	}
}

func TestAnObservationCannotRewriteARowAnOperatorOverrode(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	registry.tools["echo_read"].source = "override"
	registry.tools["echo_read"].class = models.SideEffectRead

	result := onlyResult(t, observe(t, mcp, observationJSON("echo_read", models.DeclaredDestructive, changedToolDigest, "")))
	if result.Outcome != outcomeContended {
		t.Fatalf("result = %+v: an overridden row must not be written by observation", result)
	}
	if registry.tools["echo_read"].class != models.SideEffectRead || registry.tools["echo_read"].source != "override" {
		t.Fatalf("row = %+v", registry.tools["echo_read"])
	}
}

// ---------------------------------------------------------------------------
// Approval and list
// ---------------------------------------------------------------------------

func approvalHandler(t *testing.T, mcp *MCPHandler) (http.Handler, mcpTokenSigner) {
	t.Helper()
	mux := http.NewServeMux()
	mcp.Register(mux)
	return mcpAuthenticatedHandlerWithSigner(t, mux)
}

// approve approves the drifted definition driftedRegistry leaves behind
// (changedToolDigest), the one the person is assumed to have just reviewed.
func approve(t *testing.T, handler http.Handler, token, serverID, tool string) *httptest.ResponseRecorder {
	t.Helper()
	return approveBody(t, handler, token, serverID, tool, `{"approved_digest":"`+changedToolDigest+`"}`)
}

func approveBody(t *testing.T, handler http.Handler, token, serverID, tool, body string) *httptest.ResponseRecorder {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp/"+serverID+"/tools/"+tool+"/approve", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func driftedRegistry(t *testing.T, scope string, trusted bool) (*toolRegistry, *MCPHandler) {
	t.Helper()
	registry := newToolRegistry()
	registry.addServer(observeServerID, scope, trusted, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, changedToolDigest, ""))
	if registry.tools["echo_read"].state != reviewDrifted {
		t.Fatalf("setup: tool is %s, want drifted", registry.tools["echo_read"].state)
	}
	return registry, mcp
}

func TestApprovalAuthority(t *testing.T) {
	nonZDR, zdr := false, true
	for _, c := range []struct {
		name   string
		scope  string
		scopes []string
		zdr    *bool
		want   int
	}{
		{"org server, admin with write", "org", []string{authz.WriteScope, "org:admin"}, &nonZDR, http.StatusOK},
		{"org server, write but not admin", "org", []string{authz.WriteScope}, &nonZDR, http.StatusForbidden},
		{"org server, admin but read only", "org", []string{authz.ReadScope, "org:admin"}, &nonZDR, http.StatusForbidden},
		{"workspace server is treated as shared", "workspace", []string{authz.WriteScope}, &nonZDR, http.StatusForbidden},
		{"user server, its owner", "user", []string{authz.WriteScope}, &nonZDR, http.StatusOK},
		{"org server, a ZDR principal", "org", []string{authz.WriteScope, "org:admin"}, &zdr, http.StatusForbidden},
	} {
		t.Run(c.name, func(t *testing.T) {
			registry, mcp := driftedRegistry(t, c.scope, false)
			handler, sign := approvalHandler(t, mcp)
			before := registry.tools["echo_read"].state
			response := approve(t, handler, sign("user", c.scopes, c.zdr), observeServerID, "echo_read")
			if response.Code != c.want {
				t.Fatalf("status = %d, want %d (%s)", response.Code, c.want, response.Body.String())
			}
			after := registry.tools["echo_read"].state
			if c.want != http.StatusOK && after != before {
				t.Fatalf("a refused approval changed the tool: %s -> %s", before, after)
			}
			if c.want == http.StatusOK && after != reviewApproved {
				t.Fatalf("tool is %s after approval", after)
			}
		})
	}
}

func TestApprovalOfAUserServerByAnotherUserIsRefused(t *testing.T) {
	registry, mcp := driftedRegistry(t, "user", false)
	server := registry.servers[observeServerID]
	server.config["owner_user_id"] = "someone-else"
	registry.servers[observeServerID] = server
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	response := approve(t, handler, sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR), observeServerID, "echo_read")
	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403: an org admin is not the owner of a user's server", response.Code)
	}
}

func TestApprovalOfUnknownThingsIsNotFoundAndAnUnobservedToolIsAConflict(t *testing.T) {
	registry, mcp := driftedRegistry(t, "org", false)
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	token := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)

	if response := approve(t, handler, token, "no-such-server", "echo_read"); response.Code != http.StatusNotFound {
		t.Fatalf("unknown server status = %d, want 404", response.Code)
	}
	if response := approve(t, handler, token, observeServerID, "no_such_tool"); response.Code != http.StatusNotFound {
		t.Fatalf("unknown tool status = %d, want 404", response.Code)
	}
	registry.tools["echo_read"].observed = ""
	if response := approve(t, handler, token, observeServerID, "echo_read"); response.Code != http.StatusConflict {
		t.Fatalf("unobserved tool status = %d, want 409", response.Code)
	}
	registry.tools["echo_read"].observed = changedToolDigest
	registry.tools["echo_read"].source = "override"
	if response := approve(t, handler, token, observeServerID, "echo_read"); response.Code != http.StatusConflict {
		t.Fatalf("operator-set tool status = %d, want 409", response.Code)
	}
}

func TestApprovingAnAlreadyApprovedToolIsANoOp(t *testing.T) {
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	audits := len(registry.audits)
	token := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)
	response := approveBody(t, handler, token, observeServerID, "echo_read", `{"approved_digest":"`+knownAnswerToolDigest+`"}`)
	if response.Code != http.StatusOK || len(registry.audits) != audits {
		t.Fatalf("status = %d audits %d -> %d", response.Code, audits, len(registry.audits))
	}
	// A digest that is not the observed one is not a no-op: the person reviewed something else.
	if response := approve(t, handler, token, observeServerID, "echo_read"); response.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409 for a digest that is not the observed one", response.Code)
	}
}

func TestApprovalNeedsTheDigestThePersonReviewed(t *testing.T) {
	registry, mcp := driftedRegistry(t, "org", false)
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	token := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)

	for name, body := range map[string]string{
		"no body":         ``,
		"empty object":    `{}`,
		"not json":        `approve`,
		"not a digest":    `{"approved_digest":"changed"}`,
		"upper case":      `{"approved_digest":"` + strings.ToUpper(knownAnswerToolDigest) + `"}`,
		"too short":       `{"approved_digest":"` + changedToolDigest[:63] + `"}`,
		"unknown field":   `{"approved_digest":"` + changedToolDigest + `","force":true}`,
		"two json values": `{"approved_digest":"` + changedToolDigest + `"}{}`,
	} {
		if response := approveBody(t, handler, token, observeServerID, "echo_read", body); response.Code != http.StatusBadRequest {
			t.Errorf("%s: status = %d, want 400 (%s)", name, response.Code, response.Body.String())
		}
	}
	if registry.tools["echo_read"].state != reviewDrifted || registry.tools["echo_read"].pinned != knownAnswerToolDigest {
		t.Fatalf("a refused approval changed the tool: %+v", registry.tools["echo_read"])
	}
}

// The bait and switch: a person reviews definition B, the server then serves C,
// and the next listing records C while the tool stays drifted. Their approval
// of B must not pin C.
func TestApprovalOfADigestTheServerNoLongerServesPinsNothing(t *testing.T) {
	registry, mcp := driftedRegistry(t, "org", false)
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	token := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)

	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, anotherChangedToolDigest, ""))
	if row := registry.tools["echo_read"]; row.state != reviewDrifted || row.observed != anotherChangedToolDigest {
		t.Fatalf("setup: row = %+v", row)
	}
	audits := len(registry.audits)

	response := approve(t, handler, token, observeServerID, "echo_read") // the reviewed one: changedToolDigest
	if response.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409 (%s)", response.Code, response.Body.String())
	}
	row := registry.tools["echo_read"]
	if row.state != reviewDrifted || row.pinned != knownAnswerToolDigest || len(registry.audits) != audits {
		t.Fatalf("a stale approval changed the tool: %+v, audits %v", row, registry.audits)
	}
	if registry.caps[row.capabilityID].enabled {
		t.Fatal("a stale approval re-enabled the tool")
	}

	// Approving what is observed now is what works.
	response = approveBody(t, handler, token, observeServerID, "echo_read", `{"approved_digest":"`+anotherChangedToolDigest+`"}`)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (%s)", response.Code, response.Body.String())
	}
	if row := registry.tools["echo_read"]; row.state != reviewApproved || row.pinned != anotherChangedToolDigest {
		t.Fatalf("row after approving the observed digest = %+v", row)
	}
}

func TestApprovalIsRefusedWhenTheToolMovedAfterThePersonLookedAtIt(t *testing.T) {
	registry, mcp := driftedRegistry(t, "org", false)
	registry.contend = true
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	response := approve(t, handler, sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR), observeServerID, "echo_read")
	if response.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409", response.Code)
	}
	if registry.tools["echo_read"].state != reviewDrifted {
		t.Fatal("a lost approval still re-pinned the tool")
	}
}

func TestApprovalRederivesTheClassFromWhatTheServerDeclaredAndHowFarItIsTrusted(t *testing.T) {
	// Untrusted: the derived class is the external write it already had.
	registry, mcp := driftedRegistry(t, "org", false)
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	admin := sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)
	if response := approve(t, handler, admin, observeServerID, "echo_read"); response.Code != http.StatusOK {
		t.Fatalf("status = %d", response.Code)
	}
	if registry.tools["echo_read"].class != models.SideEffectExternalWrite {
		t.Fatalf("class = %s", registry.tools["echo_read"].class)
	}

	// Trusted after the fact: a person with authority looks and approves, and
	// that is the one place the class may move down to what the server declared.
	registry, mcp = driftedRegistry(t, "org", false)
	server := registry.servers[observeServerID]
	server.config["annotation_trust"] = true
	registry.servers[observeServerID] = server
	handler, sign = approvalHandler(t, mcp)
	admin = sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR)
	if response := approve(t, handler, admin, observeServerID, "echo_read"); response.Code != http.StatusOK {
		t.Fatalf("status = %d", response.Code)
	}
	if registry.tools["echo_read"].class != models.SideEffectRead {
		t.Fatalf("class = %s, want read after an admin approved a trusted server's tool", registry.tools["echo_read"].class)
	}
}

func TestApprovePathParsing(t *testing.T) {
	for path, want := range map[string][2]string{
		"srv/tools/echo_read/approve": {"srv", "echo_read"},
		"srv/tools/a.b:c-d/approve":   {"srv", "a.b:c-d"},
	} {
		server, tool, ok := parseMCPToolApprovePath(path)
		if !ok || server != want[0] || tool != want[1] {
			t.Errorf("%q -> (%q, %q, %v)", path, server, tool, ok)
		}
	}
	for _, path := range []string{"srv", "srv/tools", "srv/tools/x", "srv/tools/x/approve/more", "srv/other/x/approve", "/tools/x/approve", "srv/tools//approve"} {
		if _, _, ok := parseMCPToolApprovePath(path); ok {
			t.Errorf("%q parsed as an approve path", path)
		}
	}
}

func TestListToolsIsTenantPinnedAndAnEmptyListIsAnArray(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, readToken, _ := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodGet, "/api/v1/mcp/srv/tools", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusOK || strings.TrimSpace(response.Body.String()) != "[]" {
		t.Fatalf("status = %d body = %q, want 200 and []", response.Code, response.Body.String())
	}
	call := database.queries[len(database.queries)-1]
	if len(call.args) != 2 || call.args[0] != "org-a" || call.args[1] != "srv" {
		t.Fatalf("list args = %v, want the signed org then the server", call.args)
	}
	if !strings.Contains(call.query, "org_id=$1") {
		t.Fatalf("list query is not tenant scoped: %s", call.query)
	}

	request = httptest.NewRequest(http.MethodPost, "/api/v1/mcp/srv/tools", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response = httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden && response.Code != http.StatusNotFound {
		t.Fatalf("a write to the list route status = %d, want 403 or 404", response.Code)
	}
}

// ---------------------------------------------------------------------------
// Server registration and the cascade
// ---------------------------------------------------------------------------

func TestAnnotationTrustNeedsAnAdminForASharedServerAndNotForAUsersOwn(t *testing.T) {
	register := func(t *testing.T, scopes []string, body string, created bool) (*httptest.ResponseRecorder, *recordingDatabase) {
		t.Helper()
		database := &recordingDatabase{}
		mcp := NewMCPHandler(nil)
		mcp.pool = database
		mcp.resolver = publicMCPResolver{}
		mux := http.NewServeMux()
		mcp.Register(mux)
		handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
		if created {
			database.expectUpsertReturningID(t, "mcp_new")
		}
		nonZDR := false
		request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+sign("user", scopes, &nonZDR))
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		return response, database
	}
	shared := `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","config_json":{"tool_allowlist":["a"],"annotation_trust":true}}`
	own := `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"user","config_json":{"tool_allowlist":["a"],"owner_user_id":"user-a","annotation_trust":true}}`
	untrusted := `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","config_json":{"tool_allowlist":["a"],"annotation_trust":false}}`
	unstated := `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","config_json":{"tool_allowlist":["a"]}}`

	if response, database := register(t, []string{authz.WriteScope}, shared, false); response.Code != http.StatusForbidden || database.databaseCalls() != 0 {
		t.Fatalf("non-admin trusting a shared server: status = %d calls = %d", response.Code, database.databaseCalls())
	}
	if response, _ := register(t, []string{authz.WriteScope, "org:admin"}, shared, true); response.Code != http.StatusCreated {
		t.Fatalf("admin trusting a shared server: status = %d, body=%s", response.Code, response.Body.String())
	}
	if response, _ := register(t, []string{authz.WriteScope}, own, true); response.Code != http.StatusCreated {
		t.Fatalf("owner trusting their own server: status = %d, body=%s", response.Code, response.Body.String())
	}
	if response, _ := register(t, []string{authz.WriteScope}, untrusted, true); response.Code != http.StatusCreated {
		t.Fatalf("anyone may state untrusted: status = %d", response.Code)
	}
	response, database := register(t, []string{authz.WriteScope}, unstated, true)
	if response.Code != http.StatusCreated {
		t.Fatalf("unstated trust: status = %d", response.Code)
	}
	call := database.queryRows[len(database.queryRows)-1]
	stored, _ := call.args[7].([]byte)
	if strings.Contains(string(stored), "annotation_trust") {
		t.Fatalf("an unstated trust was written as a choice: %s", stored)
	}
	// The upsert keeps a stored choice only while the endpoint and the scope are
	// unchanged: a user-scoped server widened to a shared scope must not carry
	// the owner's trust past the admin check.
	if !strings.Contains(call.query, "mcp_servers.endpoint_url = $5") ||
		!strings.Contains(call.query, "mcp_servers.scope = $9") ||
		!strings.Contains(call.query, "annotation_trust") {
		t.Fatalf("registration upsert does not preserve a stored trust choice per endpoint and scope: %s", call.query)
	}
}

func TestDisablingOrRemovingAServerSwitchesItsToolsOffInTheSameStatement(t *testing.T) {
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	patch := func(body string) databaseCall {
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/srv", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("patch %s status = %d, body=%s", body, response.Code, response.Body.String())
		}
		return database.execs[len(database.execs)-1]
	}
	for _, body := range []string{`{"enabled":false}`, `{"rollout_state":"quarantine"}`, `{"rollout_state":"deprecated"}`} {
		call := patch(body)
		if !strings.Contains(call.query, "cascade AS") || !strings.Contains(call.query, "kind='mcp_tool'") {
			t.Fatalf("%s did not cascade to the tool rows: %s", body, call.query)
		}
		if len(call.args) != 6 || call.args[5] != "org-a" {
			t.Fatalf("%s cascade is not tenant scoped: %v", body, call.args)
		}
		if !strings.Contains(call.query, "org_id=$6") || !strings.Contains(call.query, "server_id=$5") {
			t.Fatalf("%s cascade is not bound to the signed org and the server: %s", body, call.query)
		}
	}

	request := httptest.NewRequest(http.MethodDelete, "/api/v1/mcp/srv", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	call := database.execs[len(database.execs)-1]
	if response.Code != http.StatusNoContent || !strings.Contains(call.query, "cascade AS") {
		t.Fatalf("delete status = %d, query = %s", response.Code, call.query)
	}
	if len(call.args) != 3 || call.args[2] != "org-a" || !strings.Contains(call.query, "server_id=$2") {
		t.Fatalf("delete cascade is not scoped: %v / %s", call.args, call.query)
	}

	// A sharing change disables nothing.
	database.nextRows = append(database.nextRows, mcpConfigRow{
		config: map[string]any{"tool_allowlist": []string{"a"}, "owner_user_id": "user-a"}, scope: "user", authKind: "none",
	})
	call = patch(`{"shared_with":["user-b"]}`)
	if strings.Contains(call.query, "cascade AS") {
		t.Fatalf("a sharing change cascaded: %s", call.query)
	}

	// A registration that leaves the server disabled cascades in its one statement.
	database.expectUpsertReturningID(t, "mcp_new")
	request = httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(validMCPRegistrationJSON("org-a")))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response = httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	registration := database.queryRows[len(database.queryRows)-1]
	if response.Code != http.StatusCreated || !strings.Contains(registration.query, "cascade AS") || !strings.Contains(registration.query, "NOT $10::boolean") {
		t.Fatalf("registration status = %d query = %s", response.Code, registration.query)
	}
}

func TestEveryMCPRegistryStatementBindsTheTenantFromTheSignedOrgOnly(t *testing.T) {
	// The observation write is keyed by the org the credential was derived for and
	// by nothing in the body: a body cannot carry an org at all.
	registry := newToolRegistry()
	registry.addServer(observeServerID, "org", false, "echo_read")
	mcp := observationHandler(t, registry, observeRootSecret)
	observe(t, mcp, observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""))
	args := registry.applies[0].args
	if args[0] != observeOrg {
		t.Fatalf("org argument = %v", args[0])
	}
	if args[1] != mcpToolCapabilityID(observeOrg, "mcp__srv__echo_read") {
		t.Fatalf("capability id argument = %v", args[1])
	}
	for _, call := range registry.queryRows {
		if strings.Contains(call.query, "mcp_servers") && call.args[1] != observeOrg {
			t.Fatalf("server lookup org = %v", call.args[1])
		}
	}
}

func TestObservationAvailabilityWindowAndReasonCodesReachThePolicyGate(t *testing.T) {
	// The columns observation writes make a tool available inside the observation
	// window (not the five-minute rule) and name the cause when it is off. This
	// pins the values the SQL statement is given, which availability then reads.
	enabledState, reason, mode, cost := availabilityFor(true, "")
	if enabledState != "available" || reason != "" || mode != models.ExecutionAgentic || cost != models.CostVariable {
		t.Fatalf("enabled availability = %s %q %s %s", enabledState, reason, mode, cost)
	}
	for _, code := range []string{models.ReasonToolDefinitionChanged, models.ReasonToolNotApproved, "capability_disabled"} {
		state, text, mode, _ := availabilityFor(false, code)
		if state != "unavailable" || text == "" || mode != models.ExecutionUnavailable {
			t.Fatalf("%s: availability = %s %q %s", code, state, text, mode)
		}
	}
	checked := time.Now().UTC().Add(-30 * time.Minute)
	capability := &models.Capability{
		ID: "cap.mcp_tool.x", Kind: models.KindMCPTool, Enabled: true, RiskLevel: models.RiskHigh,
		AvailabilityState: "available", ExecutionMode: models.ExecutionAgentic, CostClass: models.CostVariable,
		HealthCheckedAt: &checked,
	}
	if got := models.DeriveAvailability(capability); got.State != models.AvailabilityApprovalRequired {
		t.Fatalf("a tool seen 30 minutes ago: %+v", got)
	}
}

// ---------------------------------------------------------------------------
// The reviewer's round: a disable mid-batch, who may share a trusted server,
// registration cascading to removed tools, and the one attester of a tool row
// ---------------------------------------------------------------------------

// A PATCH enabled=false, a DELETE or a quarantine lands between the handler's one
// read of the server for the batch and a later write of the same batch. The
// write must not carry the stale enabled=true back onto the capability row.
func TestAServerTakenOutMidBatchIsNotOverwrittenByTheRestOfTheBatch(t *testing.T) {
	for name, takeOut := range map[string]func(r *toolRegistry){
		"disabled": func(r *toolRegistry) {
			s := r.servers[observeServerID]
			s.enabled = false
			r.servers[observeServerID] = s
		},
		"deleted": func(r *toolRegistry) { delete(r.servers, observeServerID) },
		"quarantined": func(r *toolRegistry) {
			s := r.servers[observeServerID]
			s.rollout = "quarantine"
			r.servers[observeServerID] = s
		},
		"tool removed from the allowlist": func(r *toolRegistry) {
			s := r.servers[observeServerID]
			s.config = map[string]any{"tool_allowlist": []string{"echo_read"}}
			r.servers[observeServerID] = s
		},
	} {
		t.Run(name, func(t *testing.T) {
			registry := newToolRegistry()
			registry.addServer(observeServerID, "org", false, "echo_read", "create_item", "drop_all")
			mcp := observationHandler(t, registry, observeRootSecret)
			registry.beforeApply = func(n int) {
				if n != 2 {
					return
				}
				takeOut(registry)
				// The mutation's own cascade switches off what was written so far.
				if name != "tool removed from the allowlist" {
					for id, capability := range registry.caps {
						capability.enabled, capability.reasonCode = false, "capability_disabled"
						registry.caps[id] = capability
					}
				}
			}
			reply := observe(t, mcp,
				observationJSON("echo_read", models.DeclaredReadOnly, knownAnswerToolDigest, ""),
				observationJSON("create_item", models.DeclaredDestructive, changedToolDigest, ""),
				observationJSON("drop_all", models.DeclaredUndeclared, anotherChangedToolDigest, ""),
			)
			for _, result := range reply.Results {
				if result.Outcome != outcomeRecorded {
					t.Fatalf("result = %+v", result)
				}
			}
			// The tool that was written after the mutation is off, with the reason of a
			// server that no longer lists it; its registry row (the pinned digest) is
			// still recorded, so a later re-enable finds it.
			for _, tool := range []string{"create_item", "drop_all"} {
				row := registry.tools[tool]
				capability := registry.caps[row.capabilityID]
				if capability.enabled || capability.reasonCode != "capability_disabled" {
					t.Fatalf("%s capability = %+v: a write after the server was taken out re-enabled it", tool, capability)
				}
				if row.state != reviewApproved || row.pinned == "" {
					t.Fatalf("%s row = %+v", tool, row)
				}
			}
		})
	}
}

func TestAnApprovalCannotReenableAToolWhoseServerWasTakenOutAfterItWasRead(t *testing.T) {
	registry, mcp := driftedRegistry(t, "org", false)
	registry.beforeApprove = func() {
		server := registry.servers[observeServerID]
		server.enabled = false
		registry.servers[observeServerID] = server
	}
	handler, sign := approvalHandler(t, mcp)
	nonZDR := false
	response := approve(t, handler, sign("user", []string{authz.WriteScope, "org:admin"}, &nonZDR), observeServerID, "echo_read")
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	row := registry.tools["echo_read"]
	capability := registry.caps[row.capabilityID]
	if row.state != reviewApproved || row.pinned != changedToolDigest {
		t.Fatalf("the person's approval was not recorded: %+v", row)
	}
	if capability.enabled || capability.reasonCode != "capability_disabled" {
		t.Fatalf("capability = %+v: an approval re-enabled a tool of a disabled server", capability)
	}
}

// What makes the two tests above true on a real database is the srv CTE of the
// two write statements; the fake registry only mirrors it. This pins its parts:
// the live server row, selected FOR SHARE so the cascade's UPDATE of that row
// serializes with it, and a capability row whose enabled and availability columns
// come from it rather than from the caller's stale flag.
func TestToolWriteStatementsDecideEnabledAgainstTheServerRowAsItIsNow(t *testing.T) {
	for name, c := range map[string]struct{ sql, serverParam, enabledParam, bareEnabled string }{
		"observation": {applyObservationSQL, "$6", "$22", "'workspace', false, $22::boolean"},
		"approval":    {approveToolSQL, "$22", "$12", "enabled = $12"},
	} {
		t.Run(name, func(t *testing.T) {
			for _, part := range []string{
				"WITH srv AS (",
				"FROM mcp_servers",
				"id = " + c.serverParam,
				"(org_id = $1 OR org_id = 'global')",
				"deleted_at IS NULL",
				"AND enabled AND rollout_state IN ('stable', 'canary')",
				"config_json->'tool_allowlist' ? $3::text",
				"FOR SHARE",
				c.enabledParam + "::boolean AND EXISTS (SELECT 1 FROM srv)",
				c.enabledParam + "::boolean AND NOT EXISTS (SELECT 1 FROM srv)",
				"(SELECT callable FROM eff)",
				"THEN 'capability_disabled' ELSE",
			} {
				if !strings.Contains(c.sql, part) {
					t.Errorf("the statement lost %q", part)
				}
			}
			if strings.Contains(c.sql, c.bareEnabled) {
				t.Errorf("the capability's enabled flag is taken from the caller's stale value again (%q)", c.bareEnabled)
			}
		})
	}
	// The cascade it serializes with really does update the server row.
	cascade := withMCPToolCascade(`UPDATE mcp_servers SET enabled=false WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`, "$2", "$1")
	if !strings.Contains(cascade, "UPDATE mcp_servers") {
		t.Fatalf("the cascade no longer updates the server row it must serialize on: %s", cascade)
	}
}

func TestHealthAttestationCannotTouchAnMCPToolRow(t *testing.T) {
	const body = `{"id":"cap.mcp_tool.abc","version":"1","state":"available","reason_code":"runtime_healthy","execution_mode":"agentic","cost_class":"variable"}`
	t.Run("tenant lane, called directly", func(t *testing.T) {
		store := &fakeAvailabilityStore{
			row:     &registry.CapabilityRow{ID: "cap.mcp_tool.abc", Kind: models.KindMCPTool, Version: "1", Enabled: true, RiskLevel: models.RiskHigh},
			updated: true,
		}
		recorder := availabilityRecorder(t, http.MethodPost, body, store)
		if recorder.Code != http.StatusForbidden {
			t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
		}
		if store.auditAction != "" || !store.gotUpdate.HealthCheckedAt.IsZero() {
			t.Fatalf("an attestation of an MCP tool row reached persistence: %q %+v", store.auditAction, store.gotUpdate)
		}
		// The same attestation of an ordinary row of the tenant goes through, so the
		// refusal above is about the kind and nothing else.
		ordinary := &fakeAvailabilityStore{
			row:     &registry.CapabilityRow{ID: "cap.mcp_tool.abc", Kind: "tool", Version: "1", Enabled: true, RiskLevel: models.RiskLow},
			updated: true,
		}
		if recorder := availabilityRecorder(t, http.MethodPost, body, ordinary); recorder.Code != http.StatusOK || ordinary.auditAction == "" {
			t.Fatalf("control: status = %d, body = %s", recorder.Code, recorder.Body.String())
		}
	})
	t.Run("tenant lane, through the authorizer", func(t *testing.T) {
		store := &fakeAvailabilityStore{
			row:     &registry.CapabilityRow{ID: "cap.mcp_tool.abc", OrgID: "org-a", Kind: models.KindMCPTool, Version: "1", Enabled: true, RiskLevel: models.RiskHigh},
			updated: true,
		}
		recorder := availabilityAttestationRecorder(t, store, "cap.mcp_tool.abc", "org-a", "tenant-health", authz.HealthWriteScope)
		if recorder.Code != http.StatusForbidden || store.auditAction != "" {
			t.Fatalf("status = %d audit = %q, body = %s", recorder.Code, store.auditAction, recorder.Body.String())
		}
	})
}

// ---- annotation trust and sharing ------------------------------------------

func registerMCPServer(t *testing.T, scopes []string, body string, created bool) (*httptest.ResponseRecorder, *recordingDatabase) {
	t.Helper()
	database := &recordingDatabase{}
	mcp := NewMCPHandler(nil)
	mcp.pool = database
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	if created {
		database.expectUpsertReturningID(t, "mcp_new")
	}
	nonZDR := false
	request := httptest.NewRequest(http.MethodPost, "/api/v1/mcp", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+sign("user", scopes, &nonZDR))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response, database
}

func userServerBody(sharedWith, annotationTrust string) string {
	config := `"tool_allowlist":["a"],"owner_user_id":"user-a"`
	if sharedWith != "" {
		config += `,"shared_with":` + sharedWith
	}
	if annotationTrust != "" {
		config += `,"annotation_trust":` + annotationTrust
	}
	return `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"user","config_json":{` + config + `}}`
}

func TestAUserServerSharedWithOthersNeedsAnAdminToTrustItsAnnotations(t *testing.T) {
	nonAdmin, admin := []string{authz.WriteScope}, []string{authz.WriteScope, "org:admin"}

	if response, database := registerMCPServer(t, nonAdmin, userServerBody(`["user-b"]`, "true"), false); response.Code != http.StatusForbidden || database.databaseCalls() != 0 {
		t.Fatalf("non-admin trusting a user server shared with someone: status = %d calls = %d", response.Code, database.databaseCalls())
	}
	if response, _ := registerMCPServer(t, admin, userServerBody(`["user-b"]`, "true"), true); response.Code != http.StatusCreated {
		t.Fatalf("admin trusting a user server shared with someone: status = %d, body=%s", response.Code, response.Body.String())
	}
	// Alone, or sharing with nobody, it is still the owner's own risk.
	if response, _ := registerMCPServer(t, nonAdmin, userServerBody(``, "true"), true); response.Code != http.StatusCreated {
		t.Fatalf("owner trusting their own server: status = %d", response.Code)
	}
	if response, _ := registerMCPServer(t, nonAdmin, userServerBody(`[]`, "true"), true); response.Code != http.StatusCreated {
		t.Fatalf("owner trusting their own server, shared with nobody: status = %d", response.Code)
	}
	// Anyone may share a server that is not trusted.
	if response, _ := registerMCPServer(t, nonAdmin, userServerBody(`["user-b"]`, ""), true); response.Code != http.StatusCreated {
		t.Fatalf("sharing an untrusted server: status = %d", response.Code)
	}
}

func TestSharingAUserServerRegistrationDoesNotCarryTheStoredTrustOverForANonAdmin(t *testing.T) {
	// The registration does not mention annotation_trust, so the upsert may carry a
	// stored choice over; $15 says whether this caller is a non-admin who is
	// sharing a user-scoped server, and the SQL refuses the carry-over for that
	// unless the stored row was already shared.
	nonAdmin, admin := []string{authz.WriteScope}, []string{authz.WriteScope, "org:admin"}
	cases := []struct {
		name   string
		scopes []string
		body   string
		want   bool
	}{
		{"non-admin shares a user server", nonAdmin, userServerBody(`["user-b"]`, ""), true},
		{"non-admin, shared with nobody", nonAdmin, userServerBody(`[]`, ""), false},
		{"non-admin, not shared", nonAdmin, userServerBody(``, ""), false},
		{"admin shares a user server", admin, userServerBody(`["user-b"]`, ""), false},
		{"non-admin, org scope", nonAdmin, `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","config_json":{"tool_allowlist":["a"]}}`, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			response, database := registerMCPServer(t, c.scopes, c.body, true)
			if response.Code != http.StatusCreated {
				t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
			}
			call := database.queryRows[len(database.queryRows)-1]
			if len(call.args) != 16 || call.args[14] != c.want {
				t.Fatalf("carry-over gate ($15) = %v, want %v (args %d)", call.args[14], c.want, len(call.args))
			}
			for _, part := range []string{"NOT $15::boolean", "jsonb_array_length(mcp_servers.config_json->'shared_with')", "mcp_servers.endpoint_url = $5", "mcp_servers.scope = $9"} {
				if !strings.Contains(call.query, part) {
					t.Fatalf("the carry-over lost %q: %s", part, call.query)
				}
			}
		})
	}
}

func TestWideningSharedWithFromEmptyByANonAdminLeavesTheServerUntrusted(t *testing.T) {
	nonAdmin, admin := []string{authz.WriteScope}, []string{authz.WriteScope, "org:admin"}
	cases := []struct {
		name        string
		scopes      []string
		stored      []string
		patch       string
		wantTrusted bool
	}{
		{"non-admin shares an unshared trusted server", nonAdmin, nil, `["user-b"]`, false},
		{"admin shares it", admin, nil, `["user-b"]`, true},
		{"non-admin adds to a server already shared", nonAdmin, []string{"user-c"}, `["user-c","user-b"]`, true},
		{"non-admin stops sharing", nonAdmin, []string{"user-c"}, `[]`, true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			database := &recordingDatabase{}
			mcp := NewMCPHandler(nil)
			mcp.pool = database
			mcp.resolver = publicMCPResolver{}
			mux := http.NewServeMux()
			mcp.Register(mux)
			handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
			config := map[string]any{"tool_allowlist": []string{"a"}, "owner_user_id": "user-a", "annotation_trust": true}
			if c.stored != nil {
				config["shared_with"] = c.stored
			}
			database.nextRows = append(database.nextRows, mcpConfigRow{config: config, scope: "user", authKind: "none"})
			nonZDR := false
			request := httptest.NewRequest(http.MethodPatch, "/api/v1/mcp/srv", strings.NewReader(`{"shared_with":`+c.patch+`}`))
			request.Header.Set("Authorization", "Bearer "+sign("user", c.scopes, &nonZDR))
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
			}
			stored, _ := database.execs[len(database.execs)-1].args[2].([]byte)
			if trusted := strings.Contains(string(stored), `"annotation_trust":true`); trusted != c.wantTrusted {
				t.Fatalf("trusted = %v, want %v after the change: %s", trusted, c.wantTrusted, stored)
			}
			if !strings.Contains(string(stored), `"tool_allowlist"`) {
				t.Fatalf("the patch dropped the rest of the config: %s", stored)
			}
		})
	}
}

// ---- registration cascades to what the registration no longer allows ---------

func TestARegistrationSwitchesOffTheToolsItNoLongerListsAndThoseOfAQuarantinedServer(t *testing.T) {
	body := `{"name":"s","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"none","scope":"org","enabled":true,"rollout_state":"quarantine","config_json":{"tool_allowlist":["a","b"]}}`
	response, database := registerMCPServer(t, []string{authz.WriteScope}, body, true)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 1 || len(database.execs) != 0 {
		t.Fatalf("the registration and its cascade are not one statement: %d queries, %d execs", len(database.queryRows), len(database.execs))
	}
	call := database.queryRows[0]
	for _, part := range []string{
		"cascade AS",
		"NOT $10::boolean",
		"$11::text NOT IN ('stable', 'canary')",
		"NOT (ct.tool_name = ANY($16::text[]))",
		"ct.org_id=$2",
		"server_id=(SELECT id FROM registered)",
	} {
		if !strings.Contains(call.query, part) {
			t.Fatalf("the registration cascade lost %q: %s", part, call.query)
		}
	}
	if call.args[9] != true || call.args[10] != "quarantine" {
		t.Fatalf("enabled/rollout arguments = %v / %v", call.args[9], call.args[10])
	}
	allowlist, _ := call.args[15].([]string)
	if len(allowlist) != 2 || allowlist[0] != "a" || allowlist[1] != "b" {
		t.Fatalf("the allowlist argument ($16) = %v, want the registered one", call.args[15])
	}
}
