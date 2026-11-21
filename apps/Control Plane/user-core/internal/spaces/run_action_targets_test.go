package spaces

import (
	"crypto/ed25519"
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

	for _, mode := range []RunActionTargetsMode{RunActionTargetsTicketsOnly, RunActionTargetsTable} {
		if err := request.Validate(mode); err == nil {
			t.Fatalf("a request for an action outside the table validated in mode %q", mode)
		}
		if _, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now, mode); err == nil {
			t.Fatalf("a decision for an action outside the table was issued in mode %q", mode)
		}
	}
}

// secondRunActionRow is a row added to the table for one test: a read-class action with its own audience,
// permission and schema hash, which the seeded table does not hold.
func secondRunActionRow() runActionTargetRow {
	return runActionTargetRow{
		ActionID:        "chat.submit_feedback",
		ServiceAudience: "application-plane-conversation-core",
		Permission:      "owner-action:feedback",
		Class:           "write_approval",
		HashBasis:       "catalog_entry",
		SchemaHash:      "sha256:" + strings.Repeat("c", 64),
	}
}

// A second row is decidable only in table mode, and only for its own audience, permission and schema hash.
// tickets_only admits tickets.create alone, whatever the table holds, at every step of the decision lane.
func TestRunActionDecisionAdmitsASecondRowOnlyInTableMode(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	second := secondRunActionRow()
	withRunActionTargets(t, append(append([]runActionTargetRow{}, runActionTargets...), second))
	request := validRunActionRequest()
	request.ActionID = second.ActionID
	request.ActionSchemaHash = second.SchemaHash
	authority := validRunActionAuthority()
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}

	if err := request.Validate(RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only admitted a second row at the request")
	}
	if _, err := IssueRunActionDecision(evidence, authority, request, now, RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only issued a decision for a second row")
	}

	decision, err := IssueRunActionDecision(evidence, authority, request, now, RunActionTargetsTable)
	if err != nil {
		t.Fatalf("IssueRunActionDecision for an added row in table mode: %v", err)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != second.Permission {
		t.Fatalf("decision permissions = %#v; want the row's own permission", decision.Permissions)
	}
	if err := decision.Validate(RunActionTargetsTable); err != nil {
		t.Fatalf("decision for an added row did not validate in table mode: %v", err)
	}
	if err := decision.Validate(RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only validated a decision for a second row")
	}
	if err := ValidateCurrentRunActionDecision(evidence, decision, RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only passed the current-authority check for a second row")
	}
	if err := ValidateCurrentRunActionDecision(evidence, decision, RunActionTargetsTable); err != nil {
		t.Fatalf("table mode failed the current-authority check: %v", err)
	}
	if _, err := SignRunActionDecision(key, decision, RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only signed a decision for a second row")
	}
	token, err := SignRunActionDecision(key, decision, RunActionTargetsTable)
	if err != nil {
		t.Fatalf("SignRunActionDecision in table mode: %v", err)
	}
	if _, err := VerifyRunActionDecision(key, token, now, RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only verified a decision for a second row")
	}
	if _, err := VerifyRunActionDecision(key, token, now, RunActionTargetsTable); err != nil {
		t.Fatalf("VerifyRunActionDecision in table mode: %v", err)
	}
}

// The schema a decision binds is the one the table holds, not the one the caller names. A different hash is
// refused at the request and at every later check that reads the decision, in both modes. Surrounding space on
// the table's own hash is trimmed, so it still binds.
func TestRunActionDecisionBindsTheTablesSchemaHash(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()
	other := "sha256:" + strings.Repeat("e", 64)
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}

	for _, mode := range []RunActionTargetsMode{RunActionTargetsTicketsOnly, RunActionTargetsTable} {
		request := validRunActionRequest()
		request.ActionSchemaHash = other
		if err := request.Validate(mode); err == nil {
			t.Fatalf("mode %q: a request naming another schema validated", mode)
		}
		if _, err := IssueRunActionDecision(evidence, authority, request, now, mode); err == nil {
			t.Fatalf("mode %q: a decision naming another schema was issued", mode)
		}

		request.ActionSchemaHash = " " + ticketsCreateActionSchemaSHA256 + " "
		decision, err := IssueRunActionDecision(evidence, authority, request, now, mode)
		if err != nil {
			t.Fatalf("mode %q: the table's own schema hash must issue: %v", mode, err)
		}
		if decision.ActionSchemaHash != ticketsCreateActionSchemaSHA256 {
			t.Fatalf("mode %q: the decision carries %q, want the table's hash", mode, decision.ActionSchemaHash)
		}

		decision.ActionSchemaHash = other
		if err := decision.Validate(mode); err == nil {
			t.Fatalf("mode %q: a decision carrying another schema validated", mode)
		}
		if err := ValidateCurrentRunActionDecision(evidence, decision, mode); err == nil {
			t.Fatalf("mode %q: a decision carrying another schema passed the current-authority check", mode)
		}
		if _, err := SignRunActionDecision(key, decision, mode); err == nil {
			t.Fatalf("mode %q: a decision carrying another schema was signed", mode)
		}
	}
}
