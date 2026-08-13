package spaces

import (
	"strings"
	"testing"
	"time"
)

func validPersonalImportEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.ImportWriteEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:ingestion-import:7"
	return evidence
}

func TestIssuePersonalImportDecisionIsTargetBoundAndDistinctFromRetrieval(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	decision, err := IssuePersonalImportDecision(validPersonalImportEvidence(), PersonalImportDecisionRequest{
		DecisionRef: "import-decision-1", IdempotencyKey: "import-1", SourceType: "notion", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssuePersonalImportDecision: %v", err)
	}
	if decision.ActionID != personalImportAction || decision.ServiceAudience != personalImportAudience || decision.ActionSchemaHash != personalImportSchema {
		t.Fatalf("incorrect import target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "ingestion:import" {
		t.Fatalf("import permission = %#v", decision.Permissions)
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":ingestion-import:") || strings.Contains(decision.ResourceAuthorizationRef, "retrieval-read") {
		t.Fatalf("import resource ref widened or reused retrieval authority: %q", decision.ResourceAuthorizationRef)
	}
	if !strings.HasPrefix(decision.PayloadDigest, "sha256:") {
		t.Fatalf("import payload digest missing: %q", decision.PayloadDigest)
	}
	if decision.ImportSourceType != "notion" {
		t.Fatalf("import source type was not bound: %q", decision.ImportSourceType)
	}
}

func TestIssuePersonalImportDecisionDeniesOtherEntitlements(t *testing.T) {
	evidence := validPersonalImportEvidence()
	evidence.ImportWriteEntitled = false
	if _, err := IssuePersonalImportDecision(evidence, PersonalImportDecisionRequest{
		DecisionRef: "import-decision-1", IdempotencyKey: "import-1", SourceType: "notion", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "import entitlement") {
		t.Fatalf("non-import policy unexpectedly issued import decision: %v", err)
	}
}

func TestIssuePersonalImportExecutionDecisionRefreshesTheTargetWithoutChangingIntent(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalImportEvidence()
	intent := PersonalImportExecutionIntent{
		OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef,
		SubjectID: evidence.Membership.SubjectID, ActionSchemaHash: personalImportSchema,
		PayloadDigest: "sha256:durable-import-payload", IdempotencyKey: "import-1", SourceType: "notion",
	}
	decision, err := IssuePersonalImportExecutionDecision(evidence, intent, "execution-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssuePersonalImportExecutionDecision: %v", err)
	}
	if decision.ServiceAudience != personalImportExecutionAudience || decision.ActionID != personalImportAction {
		t.Fatalf("wrong execution target: %+v", decision)
	}
	if decision.PayloadDigest != intent.PayloadDigest || decision.IdempotencyKey != intent.IdempotencyKey {
		t.Fatalf("execution decision changed durable intent: %+v", decision)
	}
	if decision.ImportSourceType != intent.SourceType {
		t.Fatalf("execution decision changed source type: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "documents:write" {
		t.Fatalf("execution permissions = %#v", decision.Permissions)
	}
}

func TestIssuePersonalImportDecisionRejectsZeroRetention(t *testing.T) {
	evidence := validPersonalImportEvidence()
	evidence.Privacy.ZeroDataRetention = true
	if _, err := IssuePersonalImportDecision(evidence, PersonalImportDecisionRequest{
		DecisionRef: "import-decision-1", IdempotencyKey: "import-1", SourceType: "notion", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "zero data retention") {
		t.Fatalf("ZDR unexpectedly issued durable import decision: %v", err)
	}
}
