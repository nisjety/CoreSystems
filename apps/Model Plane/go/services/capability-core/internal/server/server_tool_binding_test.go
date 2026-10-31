package server

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// bindingRow is one result row of toolBindingSQL, in column order.
type bindingRow struct {
	capabilityID, toolName, qualifiedName, class, floor, digest, reviewState, version string
}

type bindingRows struct {
	rows []bindingRow
	next int
}

func (r *bindingRows) Close()                                       {}
func (r *bindingRows) Err() error                                   { return nil }
func (r *bindingRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (r *bindingRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (r *bindingRows) Values() ([]any, error)                       { return nil, nil }
func (r *bindingRows) RawValues() [][]byte                          { return nil }
func (r *bindingRows) Conn() *pgx.Conn                              { return nil }
func (r *bindingRows) Next() bool                                   { r.next++; return r.next <= len(r.rows) }
func (r *bindingRows) Scan(dest ...any) error {
	row := r.rows[r.next-1]
	values := []string{row.capabilityID, row.toolName, row.qualifiedName, row.class, row.floor, row.digest, row.reviewState, row.version}
	for i := range dest {
		*(dest[i].(*string)) = values[i]
	}
	return nil
}

// bindingDB answers every lookup with the same rows and records what was asked.
type bindingDB struct {
	rows     []bindingRow
	err      error
	gotOrg   string
	gotNames []string
	queries  int
}

func (db *bindingDB) Query(_ context.Context, _ string, args ...any) (pgx.Rows, error) {
	db.queries++
	db.gotOrg, _ = args[0].(string)
	db.gotNames, _ = args[1].([]string)
	if db.err != nil {
		return nil, db.err
	}
	return &bindingRows{rows: db.rows}, nil
}

// capabilityRowsStore serves several capability rows by id.
type capabilityRowsStore struct {
	rows map[string]*registry.CapabilityRow
}

func (s *capabilityRowsStore) GetForOrg(_ context.Context, id, _ string) (*registry.CapabilityRow, error) {
	row, ok := s.rows[id]
	if !ok {
		return nil, domain.ErrCapabilityNotFound
	}
	copy := *row
	return &copy, nil
}

func (s *capabilityRowsStore) RankedList(context.Context, string, string, []string, int) ([]registry.ScoredCapability, error) {
	return nil, nil
}

func availableCapabilityRow(id, kind, risk, class, floor string) *registry.CapabilityRow {
	now := time.Now().UTC()
	executionMode := models.ExecutionDirectRead
	if risk == models.RiskHigh {
		executionMode = models.ExecutionAgentic
	}
	return &registry.CapabilityRow{
		ID: id, OrgID: "org-a", Name: id, Kind: kind, Version: "1.0.0", RiskLevel: risk, Scope: "global",
		Enabled: true, EnabledForScopes: []string{"global"}, RolloutState: "stable",
		AvailabilityState: string(models.AvailabilityAvailable), ExecutionMode: executionMode,
		CostClass: models.CostBounded, HealthCheckedAt: &now, SideEffectClass: class, ApprovalFloor: floor,
	}
}

func toolBindingServer(t *testing.T, db *bindingDB, rows ...*registry.CapabilityRow) (*Server, *DecisionProofSigner) {
	t.Helper()
	store := &capabilityRowsStore{rows: map[string]*registry.CapabilityRow{}}
	for _, row := range rows {
		store.rows[row.ID] = row
	}
	signer := NewDecisionProofSignerForTest()
	server := newTestServer().WithDecisionProofSigner(signer)
	server.store = store
	if db != nil {
		server.WithToolBindingDatabase(db)
	}
	return server, signer
}

func evidenceClaims(t *testing.T, signer *DecisionProofSigner, proof string) map[string]any {
	t.Helper()
	parts := strings.Split(proof, ".")
	if len(parts) != 3 {
		t.Fatalf("evidence has %d segments", len(parts))
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || !ed25519.Verify(signer.PublicKey(), []byte(parts[0]+"."+parts[1]), signature) {
		t.Fatal("evidence does not verify")
	}
	payload, _ := base64.RawURLEncoding.DecodeString(parts[1])
	var claims map[string]any
	if err := json.Unmarshal(payload, &claims); err != nil {
		t.Fatalf("decode claims: %v", err)
	}
	return claims
}

func evaluateTool(t *testing.T, server *Server, capabilityID, tool, user string) *mpv1.EvaluatePolicyResponse {
	t.Helper()
	response, err := server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), &mpv1.EvaluatePolicyRequest{
		CapabilityId: capabilityID, RunId: "run-a", AgentId: "execution-core", Scope: "global", ToolName: tool, UserId: user,
	})
	if err != nil {
		t.Fatalf("EvaluatePolicy: %v", err)
	}
	return response
}

const testDigest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

func TestEvaluatePolicyBindsTheNamedToolIntoTheSignedDecision(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	db := &bindingDB{rows: []bindingRow{{capabilityID: "cap.tool.http", toolName: "web_fetch", class: "read", floor: "none", version: "1.0.0"}}}
	server, signer := toolBindingServer(t, db, http)

	response := evaluateTool(t, server, "cap.tool.http", "web_fetch", "user-a")
	if response.Decision != policy.DecisionAllow {
		t.Fatalf("decision = %+v", response)
	}
	if response.SideEffectClass != "read" || response.ApprovalFloor != "none" || response.PolicySource != policy.PolicySourceRiskLevel || response.ToolDigest != "" {
		t.Fatalf("response class fields = %+v", response)
	}
	claims := evidenceClaims(t, signer, response.DecisionEvidence)
	for key, want := range map[string]string{
		"v": "2", "tool_name": "web_fetch", "user_id": "user-a", "side_effect_class": "read", "approval_floor": "none",
		"policy_source": "risk_level", "tool_digest": "", "capability_id": "cap.tool.http", "decision": "allow",
	} {
		if claims[key] != want {
			t.Errorf("claim %s = %v, want %q", key, claims[key], want)
		}
	}
	if db.gotOrg != "org-a" || len(db.gotNames) != 1 || db.gotNames[0] != "web_fetch" {
		t.Fatalf("lookup args = %q %v", db.gotOrg, db.gotNames)
	}

	// The audit id tells two tools of one capability, and two people, apart.
	other := evaluateTool(t, server, "cap.tool.http", "web_search", "user-a")
	otherUser := evaluateTool(t, server, "cap.tool.http", "web_fetch", "user-b")
	if other.DecisionId == response.DecisionId || otherUser.DecisionId == response.DecisionId {
		t.Fatalf("decision ids collide: %q %q %q", response.DecisionId, other.DecisionId, otherUser.DecisionId)
	}
	again := evaluateTool(t, server, "cap.tool.http", "web_fetch", "user-a")
	if again.DecisionId != response.DecisionId {
		t.Fatalf("decision id is not stable: %q != %q", again.DecisionId, response.DecisionId)
	}
}

// A request that names no tool and no user is decided and identified exactly as
// before the tool binding existed.
func TestEvaluatePolicyWithoutAToolKeepsTheCapabilityLevelDecisionAndID(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	db := &bindingDB{}
	server, signer := toolBindingServer(t, db, http)

	response := evaluateTool(t, server, "cap.tool.http", "", "")
	if response.Decision != policy.DecisionAllow || db.queries != 0 {
		t.Fatalf("decision = %+v, queries = %d", response, db.queries)
	}
	if response.SideEffectClass != "read" || response.ApprovalFloor != "none" {
		t.Fatalf("a capability-level decision reports its ceiling: %+v", response)
	}
	claims := evidenceClaims(t, signer, response.DecisionEvidence)
	if claims["tool_name"] != "" || claims["user_id"] != "" {
		t.Fatalf("claims bind a tool or user that was not named: %v", claims)
	}
}

func TestEvaluatePolicyAppliesTheToolsClassFloorAndReviewState(t *testing.T) {
	lineage := availableCapabilityRow("cap.agent.lineage.read", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorAsk)
	mcp := availableCapabilityRow("cap.mcp_tool.a", models.KindMCPTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	plain := availableCapabilityRow("cap.tool.plain", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	tests := []struct {
		name         string
		capability   *registry.CapabilityRow
		tool         string
		rows         []bindingRow
		wantDecision string
		wantReason   string
		wantSource   string
		wantDigest   string
	}{
		{
			name: "a floor of ask pauses a low-risk read", capability: lineage, tool: "read_subagent_result",
			rows:         []bindingRow{{capabilityID: lineage.ID, toolName: "read_subagent_result", class: "read", floor: "ask", version: "1.0.0"}},
			wantDecision: policy.DecisionAsk, wantSource: policy.PolicySourceApprovalFloor,
		},
		{
			name: "a sibling tool without the floor is allowed", capability: lineage, tool: "list_subagent_results",
			rows:         []bindingRow{{capabilityID: lineage.ID, toolName: "list_subagent_results", class: "read", floor: "none", version: "1.0.0"}},
			wantDecision: policy.DecisionAllow, wantSource: policy.PolicySourceRiskLevel,
		},
		{
			name: "an approved MCP read is allowed and carries its pinned digest", capability: mcp, tool: "mcp__srv__echo",
			rows:         []bindingRow{{capabilityID: mcp.ID, toolName: "echo", qualifiedName: "mcp__srv__echo", class: "read", floor: "none", digest: testDigest, reviewState: "approved", version: "1.0.0-0123456789ab"}},
			wantDecision: policy.DecisionAllow, wantSource: policy.PolicySourceRiskLevel, wantDigest: testDigest,
		},
		{
			name: "a drifted MCP tool is denied", capability: mcp, tool: "mcp__srv__echo",
			rows:         []bindingRow{{capabilityID: mcp.ID, toolName: "echo", qualifiedName: "mcp__srv__echo", class: "read", floor: "none", digest: testDigest, reviewState: "drifted", version: "1.0.0-0123456789ab"}},
			wantDecision: policy.DecisionDeny, wantReason: "tool_definition_changed", wantSource: policy.PolicySourceToolReview, wantDigest: testDigest,
		},
		{
			name: "an unclassified MCP tool is an ask", capability: mcp, tool: "mcp__srv__mystery",
			rows:         []bindingRow{{capabilityID: mcp.ID, toolName: "mystery", qualifiedName: "mcp__srv__mystery", class: "unclassified", floor: "ask", digest: testDigest, reviewState: "approved", version: "1.0.0-0123456789ab"}},
			wantDecision: policy.DecisionAsk, wantSource: policy.PolicySourceApprovalFloor, wantDigest: testDigest,
		},
		{
			name: "an mcp name the registry does not hold is denied", capability: mcp, tool: "mcp__srv__forged",
			wantDecision: policy.DecisionDeny, wantReason: "tool_not_bound", wantSource: policy.PolicySourceToolReview,
		},
		{
			name: "an mcp name under a built-in capability is denied", capability: lineage, tool: "mcp__srv__echo",
			rows:         []bindingRow{{capabilityID: mcp.ID, toolName: "echo", qualifiedName: "mcp__srv__echo", class: "read", floor: "none", digest: testDigest, reviewState: "approved", version: "1"}},
			wantDecision: policy.DecisionDeny, wantReason: "tool_not_bound", wantSource: policy.PolicySourceToolReview,
		},
		{
			name: "a built-in name evaluated under an MCP capability is denied", capability: mcp, tool: "web_fetch",
			rows:         []bindingRow{{capabilityID: "cap.tool.http", toolName: "web_fetch", class: "read", floor: "none", version: "1"}},
			wantDecision: policy.DecisionDeny, wantReason: "tool_not_bound", wantSource: policy.PolicySourceToolReview,
		},
		{
			name: "an unregistered built-in name falls back to the capability ceiling", capability: plain, tool: "any_tool",
			wantDecision: policy.DecisionAllow, wantSource: policy.PolicySourceRiskLevel,
		},
		{
			name: "an unregistered built-in name is held to the ceiling's ask floor", capability: lineage, tool: "list_subagent_results",
			wantDecision: policy.DecisionAsk, wantSource: policy.PolicySourceApprovalFloor,
		},
		{
			name: "a built-in name the registry binds to another capability is denied, not signed under this one", capability: plain, tool: "shell",
			rows:         []bindingRow{{capabilityID: "cap.command.shell", toolName: "shell", class: "destructive", floor: "none", version: "1"}},
			wantDecision: policy.DecisionDeny, wantReason: "tool_not_bound", wantSource: policy.PolicySourceToolReview,
		},
		{
			name: "a name bound under this capability and another uses this capability's row", capability: plain, tool: "web_fetch",
			rows: []bindingRow{
				{capabilityID: "cap.tool.http", toolName: "web_fetch", class: "read", floor: "none", version: "1"},
				{capabilityID: plain.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1"},
			},
			wantDecision: policy.DecisionAllow, wantSource: policy.PolicySourceRiskLevel,
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			server, signer := toolBindingServer(t, &bindingDB{rows: tc.rows}, tc.capability)
			response := evaluateTool(t, server, tc.capability.ID, tc.tool, "user-a")
			if response.Decision != tc.wantDecision {
				t.Fatalf("decision = %q (%s), want %q", response.Decision, response.Reason, tc.wantDecision)
			}
			if tc.wantReason != "" && response.Reason != tc.wantReason {
				t.Fatalf("reason = %q, want %q", response.Reason, tc.wantReason)
			}
			if response.PolicySource != tc.wantSource {
				t.Fatalf("policy source = %q, want %q", response.PolicySource, tc.wantSource)
			}
			if response.ToolDigest != tc.wantDigest {
				t.Fatalf("tool digest = %q, want %q", response.ToolDigest, tc.wantDigest)
			}
			claims := evidenceClaims(t, signer, response.DecisionEvidence)
			if claims["decision"] != tc.wantDecision || claims["tool_name"] != tc.tool || claims["policy_source"] != tc.wantSource || claims["tool_digest"] != tc.wantDigest {
				t.Fatalf("evidence disagrees with the response: %v", claims)
			}
		})
	}
}

// With no database the lookup cannot resolve an mcp name, so it is denied, and a
// built-in tool is held to its capability's ceiling rather than loosened.
func TestEvaluatePolicyWithoutABindingDatabaseFailsClosedForMCPAndUsesTheCeilingForBuiltIns(t *testing.T) {
	shell := availableCapabilityRow("cap.command.shell", models.KindTool, models.RiskHigh, models.SideEffectDestructive, models.ApprovalFloorNone)
	server, _ := toolBindingServer(t, nil, shell)

	forged := evaluateTool(t, server, "cap.command.shell", "mcp__srv__echo", "")
	if forged.Decision != policy.DecisionDeny || forged.Reason != "tool_not_bound" {
		t.Fatalf("an mcp name resolved without a registry: %+v", forged)
	}
	builtIn := evaluateTool(t, server, "cap.command.shell", "shell", "")
	if builtIn.Decision != policy.DecisionAsk || builtIn.SideEffectClass != "destructive" {
		t.Fatalf("a built-in tool is not held to its ceiling: %+v", builtIn)
	}
}

// An owner_effect capability with a low risk level is an allow on its risk level
// alone. When the tool is named but the registry has no row for it (no database
// attached, or no capability_tools row) the ceiling's floor must still tighten
// that to an ask, and the evidence must say so, rather than sign a plain allow
// that carries a durable_owner floor nobody enforces.
func TestEvaluatePolicyHoldsAnUnregisteredToolToTheCeilingsFloor(t *testing.T) {
	for _, tc := range []struct {
		name string
		db   *bindingDB
	}{
		{"no binding database", nil},
		{"a database with no row for the tool", &bindingDB{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ticket := availableCapabilityRow("cap.tool.ticket.create", models.KindTool, models.RiskLow, models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner)
			server, signer := toolBindingServer(t, tc.db, ticket)

			response := evaluateTool(t, server, ticket.ID, "tickets.create", "user-a")
			if response.Decision != policy.DecisionAsk || response.PolicySource != policy.PolicySourceApprovalFloor {
				t.Fatalf("an owner effect with no registry row was not held to its floor: %+v", response)
			}
			if response.SideEffectClass != "owner_effect" || response.ApprovalFloor != "durable_owner" || response.ToolDigest != "" {
				t.Fatalf("response class fields = %+v", response)
			}
			claims := evidenceClaims(t, signer, response.DecisionEvidence)
			for key, want := range map[string]string{
				"decision": "ask", "policy_source": "approval_floor", "approval_floor": "durable_owner",
				"side_effect_class": "owner_effect", "tool_name": "tickets.create", "tool_digest": "",
			} {
				if claims[key] != want {
					t.Errorf("claim %s = %v, want %q", key, claims[key], want)
				}
			}

			// A tool-less evaluation names nothing, so it is still the capability
			// level decision it always was.
			capabilityLevel := evaluateTool(t, server, ticket.ID, "", "")
			if capabilityLevel.Decision != policy.DecisionAllow {
				t.Fatalf("a tool-less evaluation changed: %+v", capabilityLevel)
			}
		})
	}
}

func TestEvaluatePolicyReportsAnUnavailableBindingLookup(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	server, _ := toolBindingServer(t, &bindingDB{err: errors.New("connection reset")}, http)
	_, err := server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), &mpv1.EvaluatePolicyRequest{
		CapabilityId: "cap.tool.http", RunId: "run-a", AgentId: "execution-core", Scope: "global", ToolName: "web_fetch",
	})
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("status = %v, want Unavailable", status.Code(err))
	}
	if strings.Contains(err.Error(), "connection reset") {
		t.Fatalf("the database error leaked to the caller: %v", err)
	}
}

// A tenant row and the global seed for the same tool resolve to the stricter of
// the two, so a tenant can never loosen a seeded class.
func TestLookupToolBindingsTakesTheStricterOfTenantAndSeedRows(t *testing.T) {
	db := &bindingDB{rows: []bindingRow{
		{capabilityID: "cap.command.shell", toolName: "shell", class: "destructive", floor: "none", version: "1"},
		{capabilityID: "cap.command.shell", toolName: "shell", class: "read", floor: "ask", version: "1"},
	}}
	server, _ := toolBindingServer(t, db)
	found, err := server.lookupToolBindings(context.Background(), "org-a", []string{"shell"})
	if err != nil {
		t.Fatalf("lookup: %v", err)
	}
	binding := found.forCapability("shell", "cap.command.shell")
	if binding == nil || binding.SideEffectClass != "destructive" || binding.ApprovalFloor != "ask" {
		t.Fatalf("binding = %+v", binding)
	}
}

func TestResolveToolBindingsReturnsRegistryBindingsAndOmitsUnknownNames(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	mcp := availableCapabilityRow("cap.mcp_tool.a", models.KindMCPTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	drifted := availableCapabilityRow("cap.mcp_tool.b", models.KindMCPTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	drifted.Enabled = false
	drifted.AvailabilityState = string(models.AvailabilityUnavailable)
	db := &bindingDB{rows: []bindingRow{
		{capabilityID: http.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1.0.0"},
		{capabilityID: mcp.ID, toolName: "echo", qualifiedName: "mcp__srv__echo", class: "read", floor: "none", digest: testDigest, reviewState: "approved", version: "1.0.0-0123456789ab"},
		{capabilityID: drifted.ID, toolName: "gone", qualifiedName: "mcp__srv__gone", class: "read", floor: "none", digest: testDigest, reviewState: "drifted", version: "1.0.0-0123456789ab"},
		{capabilityID: "cap.deleted", toolName: "orphan", class: "read", floor: "none", version: "1"},
	}}
	server, _ := toolBindingServer(t, db, http, mcp, drifted)

	response, err := server.ResolveToolBindings(verifiedGRPCContext(t, "org-a"), &mpv1.ResolveToolBindingsRequest{
		RunId: "run-a", UserId: "user-a",
		ToolNames: []string{"web_fetch", "mcp__srv__echo", "mcp__srv__gone", "orphan", "mcp__srv__forged", "web_fetch", "", strings.Repeat("x", 300)},
	})
	if err != nil {
		t.Fatalf("ResolveToolBindings: %v", err)
	}
	if db.gotOrg != "org-a" || len(db.gotNames) != 5 {
		t.Fatalf("lookup args = %q %v (the duplicate, empty and oversized names must not reach the query)", db.gotOrg, db.gotNames)
	}
	got := map[string]*mpv1.ToolBinding{}
	for _, binding := range response.Bindings {
		got[binding.ToolName] = binding
	}
	if len(response.Bindings) != 4 || got["mcp__srv__forged"] != nil {
		t.Fatalf("bindings = %v", response.Bindings)
	}
	if b := got["web_fetch"]; b == nil || b.CapabilityId != http.ID || b.SideEffectClass != "read" || b.ApprovalFloor != "none" || b.ToolDigest != "" || b.EffectivePolicy != policy.DecisionAllow {
		t.Fatalf("web_fetch binding = %+v", b)
	}
	if b := got["mcp__srv__echo"]; b == nil || b.CapabilityId != mcp.ID || b.ToolDigest != testDigest || b.ReviewState != "approved" ||
		b.CapabilityVersion != "1.0.0-0123456789ab" || b.EffectivePolicy != policy.DecisionAllow {
		t.Fatalf("echo binding = %+v", b)
	}
	if b := got["mcp__srv__gone"]; b == nil || b.ReviewState != "drifted" || b.EffectivePolicy != policy.DecisionDeny {
		t.Fatalf("a drifted tool must resolve and be a denial: %+v", b)
	}
	if b := got["orphan"]; b == nil || b.EffectivePolicy != policy.DecisionDeny {
		t.Fatalf("a binding whose capability is gone must be a denial: %+v", b)
	}
}

func TestResolveToolBindingsOmitsANameTwoCapabilitiesClaim(t *testing.T) {
	db := &bindingDB{rows: []bindingRow{
		{capabilityID: "cap.a", toolName: "dup", class: "read", floor: "none", version: "1"},
		{capabilityID: "cap.b", toolName: "dup", class: "read", floor: "none", version: "1"},
	}}
	server, _ := toolBindingServer(t, db,
		availableCapabilityRow("cap.a", models.KindTool, models.RiskLow, "read", "none"),
		availableCapabilityRow("cap.b", models.KindTool, models.RiskLow, "read", "none"))
	response, err := server.ResolveToolBindings(verifiedGRPCContext(t, "org-a"), &mpv1.ResolveToolBindingsRequest{ToolNames: []string{"dup"}})
	if err != nil {
		t.Fatalf("ResolveToolBindings: %v", err)
	}
	if len(response.Bindings) != 0 {
		t.Fatalf("an ambiguous name resolved: %v", response.Bindings)
	}
}

func TestResolveToolBindingsRefusesWhatItCannotAnswerSafely(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	server, _ := toolBindingServer(t, &bindingDB{}, http)
	ctx := verifiedGRPCContext(t, "org-a")

	tooMany := make([]string, maxToolBindingNames+1)
	for i := range tooMany {
		tooMany[i] = "tool"
	}
	if _, err := server.ResolveToolBindings(ctx, &mpv1.ResolveToolBindingsRequest{ToolNames: tooMany}); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("an oversized request status = %v", status.Code(err))
	}
	if _, err := server.ResolveToolBindings(ctx, &mpv1.ResolveToolBindingsRequest{OrgId: "org-b", ToolNames: []string{"x"}}); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("a foreign tenant status = %v", status.Code(err))
	}
	if _, err := server.ResolveToolBindings(ctx, nil); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("a nil request status = %v", status.Code(err))
	}
	if _, err := server.ResolveToolBindings(context.Background(), &mpv1.ResolveToolBindingsRequest{}); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("an unauthenticated request status = %v", status.Code(err))
	}

	noDB, _ := toolBindingServer(t, nil, http)
	if _, err := noDB.ResolveToolBindings(ctx, &mpv1.ResolveToolBindingsRequest{ToolNames: []string{"x"}}); status.Code(err) != codes.Unavailable {
		t.Fatalf("no database status = %v", status.Code(err))
	}
	failing, _ := toolBindingServer(t, &bindingDB{err: errors.New("down")}, http)
	if _, err := failing.ResolveToolBindings(ctx, &mpv1.ResolveToolBindingsRequest{ToolNames: []string{"x"}}); status.Code(err) != codes.Unavailable {
		t.Fatalf("a failing database status = %v", status.Code(err))
	}
}

// A person asks only for themselves; a service (execution-core) asserts the user
// its run acts for. The handler repeats the check the interceptor makes.
func TestResolveToolBindingsPinsTheAssertedUserForPeople(t *testing.T) {
	http := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	db := &bindingDB{rows: []bindingRow{{capabilityID: http.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1"}}}
	server, _ := toolBindingServer(t, db, http)
	person := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"})

	if _, err := server.ResolveToolBindings(person, &mpv1.ResolveToolBindingsRequest{UserId: "user-b", ToolNames: []string{"web_fetch"}}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("a person resolving for someone else: status = %v", status.Code(err))
	}
	for _, user := range []string{"", "user-a"} {
		response, err := server.ResolveToolBindings(person, &mpv1.ResolveToolBindingsRequest{UserId: user, ToolNames: []string{"web_fetch"}})
		if err != nil || len(response.Bindings) != 1 {
			t.Fatalf("a person resolving for themselves (%q): %v, %v", user, response, err)
		}
	}
	if _, err := server.ResolveToolBindings(verifiedGRPCContext(t, "org-a"), &mpv1.ResolveToolBindingsRequest{UserId: "user-b", ToolNames: []string{"web_fetch"}}); err != nil {
		t.Fatalf("a service asserting a user: %v", err)
	}
}
