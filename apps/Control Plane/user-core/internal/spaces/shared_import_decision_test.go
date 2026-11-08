package spaces

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// WS14-S1: shared-Space import decisions share the personal import action,
// schema and digest; only who may obtain one differs.

func validSharedImportEvidence() PersonalThreadDecisionEvidence {
	e := validPersonalImportEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = "editor"
	e.RecipientAudienceRef = "audience:space-room:2"
	e.ResourceAuthorizationRef = "control:space-room:ingestion-import:7"
	return e
}

func sharedImportRequest(source string) PersonalImportDecisionRequest {
	return PersonalImportDecisionRequest{DecisionRef: "import-decision-1", IdempotencyKey: "import-1", SourceType: source, Nonce: "nonce-1"}
}

var sharedImportNow = time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)

func TestIssueSharedImportDecisionIsTheOrdinaryImportActionForARoomUpload(t *testing.T) {
	decision, err := IssueSharedImportDecision(validSharedImportEvidence(), sharedImportRequest(SpaceUploadSourceType), sharedImportNow)
	if err != nil {
		t.Fatalf("IssueSharedImportDecision: %v", err)
	}
	if decision.ActionID != personalImportAction || decision.ServiceAudience != "ingestion-plane-import" ||
		decision.ActionSchemaHash != personalImportSchema {
		t.Fatalf("wrong target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "ingestion:import" {
		t.Fatalf("permissions = %v", decision.Permissions)
	}
	if decision.ImportSourceType != "space_upload" || decision.SpaceRef != "space-room" || decision.SubjectID != "user-1" {
		t.Fatalf("bindings = %+v", decision)
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":ingestion-import:") {
		t.Fatalf("resource ref = %q", decision.ResourceAuthorizationRef)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued decision does not validate: %v", err)
	}
}

// The digest function is shared with the personal issuer, so the same evidence
// and request produce the same payload digest and downstream verifiers need no change.
func TestSharedImportDecisionDigestMatchesThePersonalIssuerForTheSameEvidence(t *testing.T) {
	evidence := validSharedImportEvidence()
	request := sharedImportRequest(SpaceUploadSourceType)
	shared, err := IssueSharedImportDecision(evidence, request, sharedImportNow)
	if err != nil {
		t.Fatal(err)
	}
	if want := personalImportPayloadDigest(evidence, request); shared.PayloadDigest != want {
		t.Fatalf("digest = %s, want the personal digest %s", shared.PayloadDigest, want)
	}
	// And against a personal decision built from the same fields.
	personal := evidence
	personal.Membership.Kind = KindPersonal
	personalDecision, err := IssuePersonalImportDecision(personal, request, sharedImportNow)
	if err != nil {
		t.Fatalf("personal issuer: %v", err)
	}
	if personalDecision.PayloadDigest != shared.PayloadDigest ||
		personalDecision.ActionID != shared.ActionID || personalDecision.ActionSchemaHash != shared.ActionSchemaHash ||
		personalDecision.ServiceAudience != shared.ServiceAudience {
		t.Fatalf("personal and shared decisions diverge:\n personal %+v\n shared   %+v", personalDecision, shared)
	}
}

func TestIssueSharedImportDecisionMatrix(t *testing.T) {
	for name, tc := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		code   DenialCode
	}{
		"viewer":              {func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor},
		"personal Space":      {func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindPersonal }, DenialKindUnsupported},
		"entitlement off":     {func(e *PersonalThreadDecisionEvidence) { e.ImportWriteEntitled = false }, DenialEntitlementOff},
		"zero data retention": {func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, DenialZDRForbidsEffect},
		"empty audience ref":  {func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef = "" }, DenialAuthorityIncomplete},
		"empty audience hash": {func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceHash = " " }, DenialAuthorityIncomplete},
		"no resource ref":     {func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = "" }, DenialAuthorityIncomplete},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := validSharedImportEvidence()
			tc.mutate(&evidence)
			_, err := IssueSharedImportDecision(evidence, sharedImportRequest(SpaceUploadSourceType), sharedImportNow)
			wantDenial(t, err, tc.code)
		})
	}
	// Editor, manager and owner all pass.
	for _, role := range []string{"editor", "manager", "owner"} {
		e := validSharedImportEvidence()
		e.Membership.Role = role
		if _, err := IssueSharedImportDecision(e, sharedImportRequest(SpaceUploadSourceType), sharedImportNow); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
	}
	// ZDR is a typed refusal that names the import effect.
	zdr := validSharedImportEvidence()
	zdr.Privacy.ZeroDataRetention = true
	_, err := IssueSharedImportDecision(zdr, sharedImportRequest(SpaceUploadSourceType), sharedImportNow)
	if d := wantDenial(t, err, DenialZDRForbidsEffect); d.Effect != "import_write" {
		t.Errorf("zdr effect = %q, want import_write", d.Effect)
	}
}

func TestIssueSharedImportDecisionOnlyAcceptsTheRoomUploadSourceType(t *testing.T) {
	for _, source := range []string{"notion", "gmail", "drive", "space_upload2", "upload"} {
		if _, err := IssueSharedImportDecision(validSharedImportEvidence(), sharedImportRequest(source), sharedImportNow); !errors.Is(err, ErrSharedImportSourceType) {
			t.Errorf("source %q: err = %v, want ErrSharedImportSourceType", source, err)
		}
	}
	// A malformed source type is a plain validation error, not the typed sentinel.
	for _, malformed := range []string{"has space", "SPACE_UPLOAD"} {
		_, err := IssueSharedImportDecision(validSharedImportEvidence(), sharedImportRequest(malformed), sharedImportNow)
		if err == nil || errors.Is(err, ErrSharedImportSourceType) {
			t.Errorf("malformed source type %q: err = %v", malformed, err)
		}
	}
	for _, bad := range []PersonalImportDecisionRequest{
		{IdempotencyKey: "k", SourceType: SpaceUploadSourceType, Nonce: "n"},
		{DecisionRef: "d", SourceType: SpaceUploadSourceType, Nonce: "n"},
		{DecisionRef: "d", IdempotencyKey: "k", SourceType: SpaceUploadSourceType},
	} {
		if _, err := IssueSharedImportDecision(validSharedImportEvidence(), bad, sharedImportNow); err == nil {
			t.Errorf("incomplete request %+v was accepted", bad)
		}
	}
	if _, err := IssueSharedImportDecision(validSharedImportEvidence(), sharedImportRequest(SpaceUploadSourceType), time.Time{}); err == nil {
		t.Error("a zero issuance time was accepted")
	}
	if !SharedImportSourceTypeAllowed(" space_upload ") || SharedImportSourceTypeAllowed("notion") || SharedImportSourceTypeAllowed("") {
		t.Error("SharedImportSourceTypeAllowed allow-list is wrong")
	}
}

func sharedImportIntent(evidence PersonalThreadDecisionEvidence) PersonalImportExecutionIntent {
	return PersonalImportExecutionIntent{
		OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef, SubjectID: evidence.Membership.SubjectID,
		ActionSchemaHash: personalImportSchema, PayloadDigest: "sha256:durable-import-payload",
		IdempotencyKey: "import-1", SourceType: SpaceUploadSourceType,
	}
}

func TestIssueSharedImportExecutionDecisionKeepsTheOriginalIntent(t *testing.T) {
	evidence := validSharedImportEvidence()
	intent := sharedImportIntent(evidence)
	decision, err := IssueSharedImportExecutionDecision(evidence, intent, "execution-1", "nonce-2", sharedImportNow)
	if err != nil {
		t.Fatalf("IssueSharedImportExecutionDecision: %v", err)
	}
	if decision.ServiceAudience != "data-plane-import" || decision.ActionID != personalImportAction ||
		decision.PayloadDigest != intent.PayloadDigest || decision.IdempotencyKey != intent.IdempotencyKey ||
		decision.ImportSourceType != SpaceUploadSourceType {
		t.Fatalf("execution decision = %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "documents:write" {
		t.Fatalf("permissions = %v", decision.Permissions)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("does not validate: %v", err)
	}
}

func TestIssueSharedImportExecutionDecisionRefusesAChangedIntentOrAuthority(t *testing.T) {
	evidence := validSharedImportEvidence()
	for name, mutate := range map[string]func(*PersonalImportExecutionIntent){
		"other org":     func(i *PersonalImportExecutionIntent) { i.OrgID = "org-2" },
		"other space":   func(i *PersonalImportExecutionIntent) { i.SpaceRef = "space-other" },
		"other subject": func(i *PersonalImportExecutionIntent) { i.SubjectID = "user-2" },
	} {
		t.Run(name, func(t *testing.T) {
			intent := sharedImportIntent(evidence)
			mutate(&intent)
			if _, err := IssueSharedImportExecutionDecision(evidence, intent, "execution-1", "nonce-2", sharedImportNow); err == nil {
				t.Fatal("an intent for another actor or Space was accepted")
			}
		})
	}
	connector := sharedImportIntent(evidence)
	connector.SourceType = "notion"
	if _, err := IssueSharedImportExecutionDecision(evidence, connector, "execution-1", "nonce-2", sharedImportNow); !errors.Is(err, ErrSharedImportSourceType) {
		t.Errorf("connector intent: err = %v, want ErrSharedImportSourceType", err)
	}
	bad := sharedImportIntent(evidence)
	bad.ActionSchemaHash = "sha256:other"
	if _, err := IssueSharedImportExecutionDecision(evidence, bad, "execution-1", "nonce-2", sharedImportNow); err == nil {
		t.Error("an intent with another action schema was accepted")
	}
	if _, err := IssueSharedImportExecutionDecision(evidence, sharedImportIntent(evidence), "", "nonce-2", sharedImportNow); err == nil {
		t.Error("an empty decision ref was accepted")
	}
	// Current authority is re-checked: a member who lost the entitlement or went ZDR gets nothing.
	off := validSharedImportEvidence()
	off.ImportWriteEntitled = false
	_, err := IssueSharedImportExecutionDecision(off, sharedImportIntent(off), "execution-1", "nonce-2", sharedImportNow)
	wantDenial(t, err, DenialEntitlementOff)
	zdr := validSharedImportEvidence()
	zdr.Privacy.ZeroDataRetention = true
	_, err = IssueSharedImportExecutionDecision(zdr, sharedImportIntent(zdr), "execution-1", "nonce-2", sharedImportNow)
	wantDenial(t, err, DenialZDRForbidsEffect)
}
