package spaces

import (
	"strings"
	"testing"
	"time"
)

// withRunActionTargets replaces the admitted table for one test and restores it after.
func withRunActionTargets(t *testing.T, rows []runActionTargetRow) {
	t.Helper()
	previous := runActionTargets
	runActionTargets = rows
	t.Cleanup(func() { runActionTargets = previous })
}

// The table is the projection of the golden registry (registry/space-self-api-registry.json):
// its eight rows in sorted id order. The tickets.create row is the row the table was
// seeded with before the registry existed, unchanged.
func TestRunActionTargetTableSeedsTicketsCreateOnly(t *testing.T) {
	want := []runActionTargetRow{
		{ActionID: "space.agent.pause_self", ServiceAudience: "application-plane-convex-core", Permission: "space-action:execute",
			Class: "workspace_write", HashBasis: "parameters_json", SchemaHash: "sha256:8c2d8043ec52d70a1bc7c90615f64d8a501f1635d6a445417a3a8b672a42ce39"},
		{ActionID: "space.canvas.read", ServiceAudience: "data-plane-canvas", Permission: "canvas:read",
			Class: "read_only", HashBasis: "parameters_json", SchemaHash: "sha256:7760a5a5ce90f1c50ad52b3bb68cfcb409a0d348b55eb2f066cd92e85474cac1"},
		{ActionID: "space.canvas.write", ServiceAudience: "data-plane-canvas", Permission: "canvas:write",
			Class: "workspace_write", HashBasis: "parameters_json", SchemaHash: "sha256:9f0b5e21a35e9daf7b00b5cda023b71b598b169f28c4e112208f73b66a7d19e0"},
		{ActionID: "space.schedule.follow_up", ServiceAudience: "model-plane-capability-core", Permission: "space-action:execute",
			Class: "write_approval", HashBasis: "parameters_json", SchemaHash: "sha256:5e33241382f8fd3a93fcc22bb8d47f87beb794e65a4a2bf0d3d9f46ab967b3c2"},
		{ActionID: "space.thread.pin", ServiceAudience: "model-plane-session-core", Permission: "space-action:execute",
			Class: "workspace_write", HashBasis: "parameters_json", SchemaHash: "sha256:bbd2580ffd5c0ba20dcf692314d797a89897603052202b8d00a98c1a94638294"},
		{ActionID: "space.thread.post_card", ServiceAudience: "model-plane-session-core", Permission: "space-action:execute",
			Class: "workspace_write", HashBasis: "parameters_json", SchemaHash: "sha256:0c1cd8f2e7cb368e7ea5a7369ca19fa2723754c1ea38ad173ef98c8ba72f42e3"},
		{ActionID: "space.thread.rename", ServiceAudience: "model-plane-session-core", Permission: "space-action:execute",
			Class: "workspace_write", HashBasis: "parameters_json", SchemaHash: "sha256:817e5676063235350559652663e961ba67ac2e2c260b43b544446a3b73c98543"},
		{
			ActionID:        "tickets.create",
			ServiceAudience: "application-plane-conversation-core",
			Permission:      "owner-action:execute",
			Class:           "write_approval",
			HashBasis:       "owner_contract",
			SchemaHash:      ticketsCreateActionSchemaSHA256,
		},
	}
	if len(runActionTargets) != len(want) {
		t.Fatalf("run action target table has %d rows; the golden projection has %d", len(runActionTargets), len(want))
	}
	for index, row := range want {
		if got := runActionTargets[index]; got != row {
			t.Fatalf("row %d = %#v, want %#v", index, got, row)
		}
	}
	if _, found := runActionTargetFor("users.delete"); found {
		t.Fatal("an unseeded action resolved to a target row")
	}
}

func TestRunActionTargetsModeIsStrict(t *testing.T) {
	for value, want := range map[string]RunActionTargetsMode{
		"table":        RunActionTargetsTable,
		"tickets_only": RunActionTargetsTicketsOnly,
		"TABLE":        RunActionTargetsTicketsOnly,
		" table":       RunActionTargetsTicketsOnly,
		"true":         RunActionTargetsTicketsOnly,
		"":             RunActionTargetsTicketsOnly,
	} {
		got := RunActionTargetsModeFromLookup(func(name string) string {
			if name != runActionTargetsEnv {
				t.Fatalf("read %q, want %q", name, runActionTargetsEnv)
			}
			return value
		})
		if got != want {
			t.Fatalf("RUN_ACTION_TARGETS=%q mode = %q, want %q", value, got, want)
		}
	}
	if got := RunActionTargetsModeFromLookup(nil); got != RunActionTargetsTicketsOnly {
		t.Fatalf("nil lookup mode = %q, want %q", got, RunActionTargetsTicketsOnly)
	}
}

// Decisions read the golden registry through runActionSpec (WS18-S1). An action
// outside the golden is refused, and a row injected into the target table alone
// never reaches a decision, even when the operator allowlist names it: the table
// serves the views and the owner reservation, not the decision path.
func TestRunActionDecisionRejectsActionOutsideTable(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	request := validRunActionRequest()
	request.ActionID = "users.delete"

	if err := request.Validate(); err == nil {
		t.Fatal("a request for an action outside the table validated")
	}
	if _, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now); err == nil {
		t.Fatal("a decision for an action outside the table was issued")
	}

	injected := runActionTargetRow{
		ActionID:        "chat.submit_feedback",
		ServiceAudience: "application-plane-conversation-core",
		Permission:      "owner-action:feedback",
		Class:           "write_approval",
		HashBasis:       "catalog_entry",
		SchemaHash:      "sha256:" + strings.Repeat("c", 64),
	}
	withRunActionTargets(t, append(append([]runActionTargetRow{}, runActionTargets...), injected))
	t.Setenv(spaceSelfAPIActionsEnv, injected.ActionID)
	if _, found := runActionTargetFor(injected.ActionID); !found {
		t.Fatal("the injected row is not in the table")
	}
	request.ActionID = injected.ActionID
	request.ActionSchemaHash = injected.SchemaHash
	if err := request.Validate(); err == nil {
		t.Fatal("a request for a row only the table names validated")
	}
	if _, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now); err == nil {
		t.Fatal("a row injected into the table reached a decision")
	}

	// A decision rewritten for the injected row does not validate either.
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision tickets.create: %v", err)
	}
	forged := decision
	forged.ActionID, forged.ServiceAudience = injected.ActionID, injected.ServiceAudience
	forged.ActionSchemaHash, forged.Permissions = injected.SchemaHash, []string{injected.Permission}
	if err := forged.Validate(); err == nil {
		t.Fatal("a decision for a row only the table names validated")
	}
	if err := ValidateCurrentRunActionDecision(evidence, forged); err == nil {
		t.Fatal("the effect-time check accepted a decision for a row only the table names")
	}
}
