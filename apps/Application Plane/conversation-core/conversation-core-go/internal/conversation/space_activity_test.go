package conversation

import (
	"context"
	"errors"
	"testing"
	"time"
)

func spaceActivityService(t *testing.T) (*Service, *fakeRepository) {
	t.Helper()
	repository := newFakeRepository()
	return NewService(repository, nil), repository
}

func TestSpaceActivityEvidenceRequiresAnOrgAndABoundedSpace(t *testing.T) {
	service, _ := spaceActivityService(t)
	for name, spaceRef := range map[string]string{
		"empty":     "  ",
		"unbounded": string(make([]byte, 201)),
	} {
		if _, err := service.SpaceActivityEvidence(context.Background(), "org-1", spaceRef); !errors.Is(err, ErrInvalidInput) {
			t.Fatalf("%s space_ref: err = %v; want ErrInvalidInput", name, err)
		}
	}
	if _, err := service.SpaceActivityEvidence(context.Background(), " ", "space-1"); !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("empty org: err = %v; want ErrInvalidInput", err)
	}
}

// Both halves are always answered. A room with grants and no operations means
// "an agent may act here and has not yet"; one with operations and no live
// grant means "it acted, and that authority is gone". A caller that cannot
// tell those apart cannot explain a refusal.
func TestSpaceActivityEvidenceAnswersBothHalvesIndependently(t *testing.T) {
	service, repository := spaceActivityService(t)
	now := time.Now().UTC()
	repository.spaceAuthorityEvents["org-1:space-1"] = []SpaceAuthorityEvent{{
		GrantID: "grant-1", ActionID: "tickets.create", SubjectID: "agent-7",
		ConversationID: "conv-1", CreatedByUserID: "user-1", CreatedAt: now,
	}}

	evidence, err := service.SpaceActivityEvidence(context.Background(), "org-1", "space-1")
	if err != nil {
		t.Fatalf("evidence: %v", err)
	}
	if len(evidence.Authority) != 1 || evidence.Authority[0].GrantID != "grant-1" {
		t.Fatalf("authority = %+v", evidence.Authority)
	}
	if evidence.Operations == nil {
		t.Fatal("operations must be an empty list, never nil: a caller has to be able to say 'nothing yet'")
	}
	if len(evidence.Operations) != 0 {
		t.Fatalf("operations = %+v; want empty", evidence.Operations)
	}
}

// `unknown` is a real terminal answer: the effect may have landed and the
// receipt did not come back. It must survive to the caller intact rather than
// being smoothed into a failure or dropped.
func TestSpaceActivityEvidenceKeepsAnUnknownOutcome(t *testing.T) {
	service, repository := spaceActivityService(t)
	repository.spaceOperationReceipts["org-1:space-1"] = []SpaceOperationReceipt{{
		OperationID: "op-1", ActionID: "tickets.create", Status: "unknown",
		SubjectID: "agent-7", GrantedByUserID: "user-1", ConversationID: "conv-1",
		TerminalReason: "owner receipt never returned",
	}}

	evidence, err := service.SpaceActivityEvidence(context.Background(), "org-1", "space-1")
	if err != nil {
		t.Fatalf("evidence: %v", err)
	}
	if len(evidence.Operations) != 1 || evidence.Operations[0].Status != "unknown" {
		t.Fatalf("operations = %+v", evidence.Operations)
	}
	if evidence.Operations[0].TicketID != "" {
		t.Fatal("an unknown outcome must not name a ticket it cannot prove exists")
	}
}

// The one thing this surface must never render: a receipt claiming an effect
// landed without naming it. The ledger's CHECK constraint already forbids it,
// and the service refuses rather than trusting the read.
func TestSpaceActivityEvidenceRefusesACompletedReceiptMissingItsProof(t *testing.T) {
	for name, receipt := range map[string]SpaceOperationReceipt{
		"no ticket": {OperationID: "op-1", Status: "completed", AuditEventID: "audit-1"},
		"no audit":  {OperationID: "op-1", Status: "completed", TicketID: "ticket-1"},
	} {
		service, repository := spaceActivityService(t)
		repository.spaceOperationReceipts["org-1:space-1"] = []SpaceOperationReceipt{receipt}
		if _, err := service.SpaceActivityEvidence(context.Background(), "org-1", "space-1"); err == nil {
			t.Fatalf("%s: a completed receipt without its owner proof was accepted", name)
		}
	}
}

func TestSpaceActivityEvidenceScopesToTheRequestedSpace(t *testing.T) {
	service, repository := spaceActivityService(t)
	repository.spaceOperationReceipts["org-1:space-other"] = []SpaceOperationReceipt{{
		OperationID: "op-elsewhere", ActionID: "tickets.create", Status: "completed",
		TicketID: "ticket-1", AuditEventID: "audit-1",
	}}

	evidence, err := service.SpaceActivityEvidence(context.Background(), "org-1", "space-1")
	if err != nil {
		t.Fatalf("evidence: %v", err)
	}
	if len(evidence.Operations) != 0 {
		t.Fatalf("another Space's operations leaked: %+v", evidence.Operations)
	}
}
