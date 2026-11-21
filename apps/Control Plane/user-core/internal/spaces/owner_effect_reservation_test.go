package spaces

import (
	"testing"
	"time"
)

func TestOwnerEffectReservationCommitmentRequiresExactContentFreeBinding(t *testing.T) {
	valid := OwnerEffectReservationCommitment{
		OperationID:      "ticket_operation_1",
		ActionID:         "tickets.create",
		ActionSchemaHash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
		PayloadDigest:    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
		IdempotencyKey:   "ticket-agent-1",
		DecisionRef:      "decision_1",
		GrantRef:         "grant_1",
	}
	if err := valid.Validate(RunActionTargetsTicketsOnly); err != nil {
		t.Fatalf("valid commitment rejected: %v", err)
	}

	for name, mutate := range map[string]func(*OwnerEffectReservationCommitment){
		"missing operation": func(value *OwnerEffectReservationCommitment) { value.OperationID = "" },
		"unapproved action": func(value *OwnerEffectReservationCommitment) { value.ActionID = "tickets.delete" },
		"invalid schema":    func(value *OwnerEffectReservationCommitment) { value.ActionSchemaHash = "sha256:bad" },
		"invalid payload":   func(value *OwnerEffectReservationCommitment) { value.PayloadDigest = "" },
		"missing grant":     func(value *OwnerEffectReservationCommitment) { value.GrantRef = "" },
	} {
		t.Run(name, func(t *testing.T) {
			candidate := valid
			mutate(&candidate)
			if err := candidate.Validate(RunActionTargetsTicketsOnly); err == nil {
				t.Fatal("invalid commitment accepted")
			}
		})
	}
}

func TestOwnerEffectReservationCommitmentMatchesAllImmutableFacts(t *testing.T) {
	left := OwnerEffectReservationCommitment{
		OperationID:      "ticket_operation_1",
		ActionID:         "tickets.create",
		ActionSchemaHash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
		PayloadDigest:    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
		IdempotencyKey:   "ticket-agent-1",
		DecisionRef:      "decision_1",
		GrantRef:         "grant_1",
	}
	if !left.Matches(left) {
		t.Fatal("identical commitment did not match")
	}
	right := left
	right.PayloadDigest = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
	if left.Matches(right) {
		t.Fatal("changed payload digest matched")
	}
}

func TestReservationCommitRequiresTheOriginalDecisionScope(t *testing.T) {
	reservation := &OwnerEffectReservation{
		Commitment: OwnerEffectReservationCommitment{
			ActionID: "tickets.create", ActionSchemaHash: "schema", PayloadDigest: "payload",
			IdempotencyKey: "key", DecisionRef: "decision",
		},
		OrgID: "org_1", SpaceRef: "space_1", SubjectID: "user_1", RunID: "run_1", ThreadID: "thread_1",
		AudienceRef: "audience_1", AudienceHash: "hash_1", AudienceRev: 2, PrivacyRef: "privacy_1", AuthorityRev: 7,
	}
	decision := RunActionDecision{
		ActionID: "tickets.create", ActionSchemaHash: "schema", PayloadDigest: "payload", IdempotencyKey: "key", DecisionRef: "decision",
		OrgID: "org_1", SpaceRef: "space_1", SubjectID: "user_1", RunID: "run_1", ThreadID: "thread_1",
		RecipientAudienceRef: "audience_1", RecipientAudienceHash: "hash_1", RecipientAudienceRevision: 2,
		PrivacyPolicyRef: "privacy_1", AuthorityRevision: 7,
	}
	if !reservationMatchesDecision(reservation, decision) {
		t.Fatal("exact original decision did not match reservation")
	}
	decision.OrgID = "org_2"
	if reservationMatchesDecision(reservation, decision) {
		t.Fatal("wrong-org decision matched reservation")
	}
}

// A second admitted row carries its own audience, not the tickets.create one.
// Its reservation commitment is admitted in table mode against that row, and is
// refused in tickets_only. tickets.create stays admitted in both modes, and an
// action outside the table is refused in both.
func TestOwnerEffectReservationCommitmentAdmitsASecondRowByItsOwnAudienceOnlyInTableMode(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	second := secondRunActionRow()
	second.ServiceAudience = "application-plane-feedback-core"
	withRunActionTargets(t, append(append([]runActionTargetRow{}, runActionTargets...), second))
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	request := validRunActionRequest()
	request.ActionID = second.ActionID
	request.ActionSchemaHash = second.SchemaHash
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now, RunActionTargetsTable)
	if err != nil {
		t.Fatalf("IssueRunActionDecision for the second row in table mode: %v", err)
	}
	if decision.ServiceAudience != second.ServiceAudience {
		t.Fatalf("decision audience = %q, want the row's own audience %q", decision.ServiceAudience, second.ServiceAudience)
	}
	commitment := OwnerEffectReservationCommitment{
		OperationID:      "operation-second-row-1",
		ActionID:         decision.ActionID,
		ActionSchemaHash: decision.ActionSchemaHash,
		PayloadDigest:    decision.PayloadDigest,
		IdempotencyKey:   decision.IdempotencyKey,
		DecisionRef:      decision.DecisionRef,
		GrantRef:         "grant-second-row-1",
	}
	if err := decision.Validate(RunActionTargetsTable); err != nil {
		t.Fatalf("table mode refused the second row's decision: %v", err)
	}
	if err := commitment.Validate(RunActionTargetsTable); err != nil {
		t.Fatalf("table mode refused the second row's reservation commitment: %v", err)
	}
	if err := commitment.Validate(RunActionTargetsTicketsOnly); err == nil {
		t.Fatal("tickets_only admitted the second row's reservation commitment")
	}

	ticket := commitment
	ticket.OperationID = "operation-tickets-1"
	ticket.ActionID = ticketsCreateActionID
	ticket.ActionSchemaHash = ticketsCreateActionSchemaSHA256
	for _, mode := range []RunActionTargetsMode{RunActionTargetsTicketsOnly, RunActionTargetsTable} {
		if err := ticket.Validate(mode); err != nil {
			t.Fatalf("mode %q refused the tickets.create reservation commitment: %v", mode, err)
		}
	}

	unknown := commitment
	unknown.ActionID = "users.delete"
	for _, mode := range []RunActionTargetsMode{RunActionTargetsTicketsOnly, RunActionTargetsTable} {
		if err := unknown.Validate(mode); err == nil {
			t.Fatalf("mode %q admitted an action outside the table", mode)
		}
	}
}
