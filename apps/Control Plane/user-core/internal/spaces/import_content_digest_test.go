package spaces

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// DEPTH-WS14-S3-1: an import execution decision binds the one body the worker
// is about to write when the worker supplies its digest. Control copies a
// well-formed digest into the signed decision unchanged, refuses a malformed
// one, and leaves every decision without one in the shape it always had.
var importBodyDigest = "sha256:" + strings.Repeat("a", 64)

func TestAnImportExecutionDecisionCarriesTheSuppliedContentDigest(t *testing.T) {
	now := time.Date(2026, 11, 24, 12, 0, 0, 0, time.UTC)
	personal := validPersonalImportEvidence()
	personalIntent := PersonalImportExecutionIntent{
		OrgID: personal.Membership.OrgID, SpaceRef: personal.Membership.SpaceRef,
		SubjectID: personal.Membership.SubjectID, ActionSchemaHash: personalImportSchema,
		PayloadDigest: "sha256:durable-import-payload", IdempotencyKey: "import-1", SourceType: "notion",
		ContentDigest: importBodyDigest,
	}
	decision, err := IssuePersonalImportExecutionDecision(personal, personalIntent, "execution-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("personal: %v", err)
	}
	if decision.ContentDigest != importBodyDigest {
		t.Fatalf("personal decision content_digest = %q", decision.ContentDigest)
	}

	shared := validSharedImportEvidence()
	sharedIntent := sharedImportIntent(shared)
	sharedIntent.ContentDigest = importBodyDigest
	sharedDecision, err := IssueSharedImportExecutionDecision(shared, sharedIntent, "execution-1", "nonce-2", sharedImportNow)
	if err != nil {
		t.Fatalf("shared: %v", err)
	}
	if sharedDecision.ContentDigest != importBodyDigest {
		t.Fatalf("shared decision content_digest = %q", sharedDecision.ContentDigest)
	}
	raw, err := json.Marshal(sharedDecision)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `"content_digest":"`+importBodyDigest+`"`) {
		t.Fatalf("the digest is not on the signed wire shape: %s", raw)
	}
}

func TestAMalformedContentDigestIsRefused(t *testing.T) {
	shared := validSharedImportEvidence()
	for _, digest := range []string{
		"sha256:short",
		"sha256:" + strings.Repeat("A", 64),
		"md5:" + strings.Repeat("a", 64),
		strings.Repeat("a", 64),
		"sha256:" + strings.Repeat("a", 64) + "\n",
	} {
		intent := sharedImportIntent(shared)
		intent.ContentDigest = digest
		if _, err := IssueSharedImportExecutionDecision(shared, intent, "execution-1", "nonce-2", sharedImportNow); err == nil {
			t.Errorf("content_digest %q was accepted", digest)
		}
	}
}

func TestADecisionWithoutAContentDigestKeepsItsShape(t *testing.T) {
	shared := validSharedImportEvidence()
	decision, err := IssueSharedImportExecutionDecision(shared, sharedImportIntent(shared), "execution-1", "nonce-2", sharedImportNow)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(raw), "content_digest") {
		t.Fatalf("a decision with no digest grew the field: %s", raw)
	}
}
