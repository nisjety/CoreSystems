package spaces

import "testing"

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
	if err := valid.Validate(); err != nil {
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
			if err := candidate.Validate(); err == nil {
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
