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

func TestRunActionTargetTableSeedsTicketsCreateOnly(t *testing.T) {
	if len(runActionTargets) != 1 {
		t.Fatalf("run action target table has %d rows; only tickets.create is seeded", len(runActionTargets))
	}
	want := runActionTargetRow{
		ActionID:        "tickets.create",
		ServiceAudience: "application-plane-conversation-core",
		Permission:      "owner-action:execute",
		Class:           "write_approval",
		HashBasis:       "owner_contract",
		SchemaHash:      ticketsCreateActionSchemaSHA256,
	}
	if got := runActionTargets[0]; got != want {
		t.Fatalf("seeded row = %#v, want %#v", got, want)
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

	// A row added to the table is decidable only for its own audience and permission.
	withRunActionTargets(t, append(append([]runActionTargetRow{}, runActionTargets...), runActionTargetRow{
		ActionID:        "chat.submit_feedback",
		ServiceAudience: "application-plane-conversation-core",
		Permission:      "owner-action:feedback",
		Class:           "write_approval",
		HashBasis:       "catalog_entry",
		SchemaHash:      "sha256:" + strings.Repeat("c", 64),
	}))
	request.ActionID = "chat.submit_feedback"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision for an added row: %v", err)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "owner-action:feedback" {
		t.Fatalf("decision permissions = %#v; want the row's own permission", decision.Permissions)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("decision for an added row did not validate: %v", err)
	}
}
