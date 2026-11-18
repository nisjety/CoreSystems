package spaces

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// WS19-S2: the room canvas decision (data.canvas.read, data.canvas.write).
// Pure tier: request shape and the closed operation set, the role floor
// (viewer reads, editor-and-above writes), kind handling (personal, room,
// project and case allowed; dm and group_dm refused), canvas_entitled and Zero
// Data Retention, and the four golden payload digests shared with wiki-store
// (WS19-S3) in testdata/space-canvas-decision.golden.json.
//
// W3-FIX-5: canvasPayloadDigest hashes the decision ref, the recipient
// audience ref and hash, the privacy policy ref and the resource
// authorization ref, and the idempotency key, all trimmed exactly as
// newEvidenceDecision writes them into the signed decision, so a caller's
// incidental surrounding whitespace on any of those must not change the
// digest. That trim is already in canvasPayloadDigest (canvas_decision.go);
// this file pins it and proves the golden vectors are unaffected.
//
// The live tier (gated on TEST_DATABASE_URL via roomLiveFixture, skipped
// without it) covers what only the repository join can answer: a member
// outside the current recipient audience resolves nothing, and an archived
// room resolves a canvas read but refuses a canvas write.

func validSharedCanvasEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.CanvasEntitled = true
	e.ResourceAuthorizationRef = "control:space-room:canvas:7"
	return e
}

func personalCanvasEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Role = role
	e.CanvasEntitled = true
	e.ResourceAuthorizationRef = "control:space-personal:canvas:7"
	return e
}

func canvasValidDigest() string { return "sha256:" + strings.Repeat("a", 64) }

func canvasReadRequest() CanvasDecisionRequest {
	return CanvasDecisionRequest{DecisionRef: "decision-1", Op: CanvasOpRead, IdempotencyKey: "canvas-read-1", Nonce: "nonce-1"}
}

func canvasCreateRequest() CanvasDecisionRequest {
	return CanvasDecisionRequest{
		DecisionRef: "decision-1", Op: CanvasOpCreate, ContentDigest: canvasValidDigest(),
		IdempotencyKey: "canvas-create-1", Nonce: "nonce-1",
	}
}

func canvasSaveRequest() CanvasDecisionRequest {
	return CanvasDecisionRequest{
		DecisionRef: "decision-1", Op: CanvasOpSave, CanvasID: "canvas-1", ExpectedVersionID: "version-2",
		ContentDigest: canvasValidDigest(), IdempotencyKey: "canvas-save-1", Nonce: "nonce-1",
	}
}

func canvasRestoreRequest() CanvasDecisionRequest {
	return CanvasDecisionRequest{
		DecisionRef: "decision-1", Op: CanvasOpRestore, CanvasID: "canvas-1", ExpectedVersionID: "version-2",
		RestoreFromVersionID: "version-1", IdempotencyKey: "canvas-restore-1", Nonce: "nonce-1",
	}
}

var canvasTestNow = time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)

func TestCanvasDecisionRequestValidateClosedOperationSet(t *testing.T) {
	digest := canvasValidDigest()
	accepted := map[string]CanvasDecisionRequest{
		"read":                     {DecisionRef: "d", Op: CanvasOpRead, IdempotencyKey: "k", Nonce: "n"},
		"read with a canvas id":    {DecisionRef: "d", Op: CanvasOpRead, CanvasID: "canvas-1", IdempotencyKey: "k", Nonce: "n"},
		"create":                   {DecisionRef: "d", Op: CanvasOpCreate, ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"save":                     {DecisionRef: "d", Op: CanvasOpSave, ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"save with a canvas id":    {DecisionRef: "d", Op: CanvasOpSave, CanvasID: "canvas-1", ExpectedVersionID: "version-2", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"restore":                  {DecisionRef: "d", Op: CanvasOpRestore, RestoreFromVersionID: "version-1", IdempotencyKey: "k", Nonce: "n"},
		"restore with a canvas id": {DecisionRef: "d", Op: CanvasOpRestore, CanvasID: "canvas-1", ExpectedVersionID: "version-2", RestoreFromVersionID: "version-1", IdempotencyKey: "k", Nonce: "n"},
	}
	for name, request := range accepted {
		if err := request.Validate(); err != nil {
			t.Errorf("%s refused: %v", name, err)
		}
	}

	refused := map[string]CanvasDecisionRequest{
		"unknown op":                     {DecisionRef: "d", Op: "update", IdempotencyKey: "k", Nonce: "n"},
		"op case":                        {DecisionRef: "d", Op: "READ", IdempotencyKey: "k", Nonce: "n"},
		"empty op":                       {DecisionRef: "d", IdempotencyKey: "k", Nonce: "n"},
		"read with content digest":       {DecisionRef: "d", Op: CanvasOpRead, ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"read with expected version":     {DecisionRef: "d", Op: CanvasOpRead, ExpectedVersionID: "version-2", IdempotencyKey: "k", Nonce: "n"},
		"read with restore source":       {DecisionRef: "d", Op: CanvasOpRead, RestoreFromVersionID: "version-1", IdempotencyKey: "k", Nonce: "n"},
		"create with a canvas id":        {DecisionRef: "d", Op: CanvasOpCreate, CanvasID: "canvas-1", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"create with expected version":   {DecisionRef: "d", Op: CanvasOpCreate, ExpectedVersionID: "version-2", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"create with restore source":     {DecisionRef: "d", Op: CanvasOpCreate, RestoreFromVersionID: "version-1", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"create without digest":          {DecisionRef: "d", Op: CanvasOpCreate, IdempotencyKey: "k", Nonce: "n"},
		"save with restore source":       {DecisionRef: "d", Op: CanvasOpSave, RestoreFromVersionID: "version-1", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"save without digest":            {DecisionRef: "d", Op: CanvasOpSave, IdempotencyKey: "k", Nonce: "n"},
		"restore with content digest":    {DecisionRef: "d", Op: CanvasOpRestore, RestoreFromVersionID: "version-1", ContentDigest: digest, IdempotencyKey: "k", Nonce: "n"},
		"restore without restore source": {DecisionRef: "d", Op: CanvasOpRestore, IdempotencyKey: "k", Nonce: "n"},
		"no decision ref":                {Op: CanvasOpRead, IdempotencyKey: "k", Nonce: "n"},
		"no idempotency key":             {DecisionRef: "d", Op: CanvasOpRead, Nonce: "n"},
		"no nonce":                       {DecisionRef: "d", Op: CanvasOpRead, IdempotencyKey: "k"},
		"idempotency key padded":         {DecisionRef: "d", Op: CanvasOpRead, IdempotencyKey: " k", Nonce: "n"},
		"idempotency key too long":       {DecisionRef: "d", Op: CanvasOpRead, IdempotencyKey: strings.Repeat("k", canvasMaxIdempotencyKeyBytes+1), Nonce: "n"},
		"canvas id padded":               {DecisionRef: "d", Op: CanvasOpRead, CanvasID: " canvas-1", IdempotencyKey: "k", Nonce: "n"},
		"canvas id too long":             {DecisionRef: "d", Op: CanvasOpRead, CanvasID: strings.Repeat("c", canvasMaxIDBytes+1), IdempotencyKey: "k", Nonce: "n"},
		"malformed content digest":       {DecisionRef: "d", Op: CanvasOpCreate, ContentDigest: "sha256:not-hex-not-hex-not-hex-not-hex-not-hex-not-hex-not-hex-not-h", IdempotencyKey: "k", Nonce: "n"},
		"content digest missing prefix":  {DecisionRef: "d", Op: CanvasOpCreate, ContentDigest: strings.Repeat("a", 64), IdempotencyKey: "k", Nonce: "n"},
	}
	for name, request := range refused {
		if err := request.Validate(); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

func TestIssueCanvasDecisionRoleFloorIsViewerToReadAndEditorToWrite(t *testing.T) {
	for _, role := range []string{RoleViewer, RoleEditor, RoleManager, RoleOwner} {
		if _, err := IssueCanvasDecision(validSharedCanvasEvidence(role), canvasReadRequest(), canvasTestNow); err != nil {
			t.Errorf("shared %s: read refused: %v", role, err)
		}
		if _, err := IssueCanvasDecision(personalCanvasEvidence(role), canvasReadRequest(), canvasTestNow); err != nil {
			t.Errorf("personal %s: read refused: %v", role, err)
		}
	}
	for _, role := range []string{RoleEditor, RoleManager, RoleOwner} {
		if _, err := IssueCanvasDecision(validSharedCanvasEvidence(role), canvasCreateRequest(), canvasTestNow); err != nil {
			t.Errorf("shared %s: write refused: %v", role, err)
		}
		if _, err := IssueCanvasDecision(personalCanvasEvidence(role), canvasCreateRequest(), canvasTestNow); err != nil {
			t.Errorf("personal %s: write refused: %v", role, err)
		}
	}
	_, err := IssueCanvasDecision(validSharedCanvasEvidence(RoleViewer), canvasCreateRequest(), canvasTestNow)
	if denial := wantDenial(t, err, DenialRoleBelowFloor); strings.Join(denial.RequiredRoles, ",") != "editor,manager,owner" {
		t.Errorf("required roles = %v, want editor, manager and owner", denial.RequiredRoles)
	}
	_, err = IssueCanvasDecision(personalCanvasEvidence(RoleViewer), canvasCreateRequest(), canvasTestNow)
	wantDenial(t, err, DenialRoleBelowFloor)
}

func TestIssueCanvasDecisionRefusesANonRecipientForBothKinds(t *testing.T) {
	personal := personalCanvasEvidence(RoleOwner)
	personal.RecipientSubjectID = "user-2"
	for _, request := range []CanvasDecisionRequest{canvasReadRequest(), canvasCreateRequest()} {
		_, err := IssueCanvasDecision(personal, request, canvasTestNow)
		wantDenial(t, err, DenialAuthorityIncomplete)
	}

	sharedNoRef := validSharedCanvasEvidence(RoleEditor)
	sharedNoRef.RecipientAudienceRef = ""
	sharedNoHash := validSharedCanvasEvidence(RoleEditor)
	sharedNoHash.RecipientAudienceHash = " "
	for name, evidence := range map[string]PersonalThreadDecisionEvidence{"no audience ref": sharedNoRef, "no audience hash": sharedNoHash} {
		for opName, request := range map[string]CanvasDecisionRequest{"read": canvasReadRequest(), "write": canvasCreateRequest()} {
			t.Run(name+"/"+opName, func(t *testing.T) {
				_, err := IssueCanvasDecision(evidence, request, canvasTestNow)
				wantDenial(t, err, DenialAuthorityIncomplete)
			})
		}
	}
}

func TestIssueCanvasDecisionKindHandling(t *testing.T) {
	for _, kind := range []Kind{KindRoom, KindProject, KindCase} {
		evidence := validSharedCanvasEvidence(RoleEditor)
		evidence.Membership.Kind = kind
		for opName, request := range map[string]CanvasDecisionRequest{"read": canvasReadRequest(), "write": canvasCreateRequest()} {
			if _, err := IssueCanvasDecision(evidence, request, canvasTestNow); err != nil {
				t.Errorf("%s/%s: refused: %v", kind, opName, err)
			}
		}
	}
	if _, err := IssueCanvasDecision(personalCanvasEvidence(RoleOwner), canvasReadRequest(), canvasTestNow); err != nil {
		t.Errorf("personal: refused: %v", err)
	}
	if _, err := IssueCanvasDecision(personalCanvasEvidence(RoleOwner), canvasCreateRequest(), canvasTestNow); err != nil {
		t.Errorf("personal: write refused: %v", err)
	}
	for _, kind := range []Kind{KindDM, KindGroupDM} {
		evidence := validSharedCanvasEvidence(RoleEditor)
		evidence.Membership.Kind = kind
		for opName, request := range map[string]CanvasDecisionRequest{"read": canvasReadRequest(), "write": canvasCreateRequest()} {
			_, err := IssueCanvasDecision(evidence, request, canvasTestNow)
			if denial := wantDenial(t, err, DenialKindUnsupported); denial.Kind != string(kind) {
				t.Errorf("%s/%s: kind = %q, want %q", kind, opName, denial.Kind, kind)
			}
		}
	}
}

func TestIssueCanvasDecisionRequiresCanvasEntitled(t *testing.T) {
	for name, build := range map[string]func() PersonalThreadDecisionEvidence{
		"personal": func() PersonalThreadDecisionEvidence { return personalCanvasEvidence(RoleOwner) },
		"shared":   func() PersonalThreadDecisionEvidence { return validSharedCanvasEvidence(RoleEditor) },
	} {
		evidence := build()
		evidence.CanvasEntitled = false
		for opName, request := range map[string]CanvasDecisionRequest{"read": canvasReadRequest(), "write": canvasCreateRequest()} {
			t.Run(name+"/"+opName, func(t *testing.T) {
				_, err := IssueCanvasDecision(evidence, request, canvasTestNow)
				if denial := wantDenial(t, err, DenialEntitlementOff); denial.Entitlement != "canvas" {
					t.Errorf("entitlement = %q, want canvas", denial.Entitlement)
				}
			})
		}
	}
}

func TestIssueCanvasDecisionRefusesZeroDataRetentionForReadAndWrite(t *testing.T) {
	for name, build := range map[string]func() PersonalThreadDecisionEvidence{
		"personal": func() PersonalThreadDecisionEvidence { return personalCanvasEvidence(RoleOwner) },
		"shared":   func() PersonalThreadDecisionEvidence { return validSharedCanvasEvidence(RoleEditor) },
	} {
		evidence := build()
		evidence.Privacy.ZeroDataRetention = true
		for opName, request := range map[string]CanvasDecisionRequest{"read": canvasReadRequest(), "write": canvasCreateRequest()} {
			t.Run(name+"/"+opName, func(t *testing.T) {
				_, err := IssueCanvasDecision(evidence, request, canvasTestNow)
				if denial := wantDenial(t, err, DenialZDRForbidsEffect); denial.Effect != "canvas" {
					t.Errorf("effect = %q, want canvas", denial.Effect)
				}
			})
		}
	}
}

func TestIssueCanvasDecisionRequiresAnIssuanceTime(t *testing.T) {
	if _, err := IssueCanvasDecision(validSharedCanvasEvidence(RoleEditor), canvasReadRequest(), time.Time{}); err == nil {
		t.Error("a zero issuance time was accepted")
	}
}

func TestIssueCanvasDecisionBindsTheOperationToTheActionAndSchema(t *testing.T) {
	for opName, request := range map[string]CanvasDecisionRequest{
		"read": canvasReadRequest(), "create": canvasCreateRequest(),
		"save": canvasSaveRequest(), "restore": canvasRestoreRequest(),
	} {
		decision, err := IssueCanvasDecision(validSharedCanvasEvidence(RoleEditor), request, canvasTestNow)
		if err != nil {
			t.Fatalf("%s: %v", opName, err)
		}
		wantAction, wantSchema, wantPermission := canvasWriteAction, canvasWriteSchema, canvasWritePermission
		if request.Op == CanvasOpRead {
			wantAction, wantSchema, wantPermission = canvasReadAction, canvasReadSchema, canvasReadPermission
		}
		if decision.ActionID != wantAction || decision.ActionSchemaHash != wantSchema ||
			len(decision.Permissions) != 1 || decision.Permissions[0] != wantPermission {
			t.Fatalf("%s: decision = %+v", opName, decision)
		}
		if decision.ServiceAudience != canvasAudience {
			t.Fatalf("%s: audience = %q, want %q", opName, decision.ServiceAudience, canvasAudience)
		}
		if err := decision.Validate(); err != nil {
			t.Fatalf("%s: issued decision does not validate: %v", opName, err)
		}
	}
}

// W3-FIX-5: canvasPayloadDigest trims the decision ref, the recipient audience
// ref and hash, the privacy policy ref, the resource authorization ref and
// the idempotency key before hashing, so a caller's incidental surrounding
// whitespace on any of them must not change the digest.
func TestCanvasPayloadDigestTrimsReferencesAndIdempotencyKeyBeforeHashing(t *testing.T) {
	evidence := validSharedCanvasEvidence(RoleEditor)
	request := canvasReadRequest()
	base := canvasPayloadDigest(evidence, request, canvasReadSchema)

	padded := evidence
	padded.RecipientAudienceRef = "  " + evidence.RecipientAudienceRef + "\t"
	padded.RecipientAudienceHash = "\n" + evidence.RecipientAudienceHash + " "
	padded.Privacy.PolicyRef = " " + evidence.Privacy.PolicyRef + " "
	padded.ResourceAuthorizationRef = "\t" + evidence.ResourceAuthorizationRef + "\t"
	paddedRequest := request
	paddedRequest.DecisionRef = "  " + request.DecisionRef + "  "
	paddedRequest.IdempotencyKey = " " + request.IdempotencyKey + " "

	if got := canvasPayloadDigest(padded, paddedRequest, canvasReadSchema); got != base {
		t.Fatalf("padded digest = %s, want the trimmed digest %s (the digest must trim before hashing)", got, base)
	}

	// Sanity: an actual content change still changes the digest.
	changed := request
	changed.IdempotencyKey = "canvas-read-2"
	if canvasPayloadDigest(evidence, changed, canvasReadSchema) == base {
		t.Fatal("the digest did not change for a different idempotency key")
	}
}

// Validate() refuses a padded idempotency key outright (canvasTokenOK requires
// exact trim equality), but accepts a padded decision_ref (only non-empty after
// trim is required there), which IssueCanvasDecision then trims consistently in
// both the stored decision field and the payload digest.
func TestIssueCanvasDecisionAcceptsAPaddedDecisionRefAndTrimsItEverywhere(t *testing.T) {
	evidence := validSharedCanvasEvidence(RoleEditor)
	trimmedRequest := canvasReadRequest()
	trimmed, err := IssueCanvasDecision(evidence, trimmedRequest, canvasTestNow)
	if err != nil {
		t.Fatalf("trimmed: %v", err)
	}
	paddedRequest := trimmedRequest
	paddedRequest.DecisionRef = "  " + trimmedRequest.DecisionRef + "  "
	padded, err := IssueCanvasDecision(evidence, paddedRequest, canvasTestNow)
	if err != nil {
		t.Fatalf("padded decision_ref refused: %v", err)
	}
	if padded.PayloadDigest != trimmed.PayloadDigest || padded.DecisionRef != trimmed.DecisionRef {
		t.Fatalf("padded decision = %+v, trimmed = %+v", padded, trimmed)
	}

	paddedKeyRequest := trimmedRequest
	paddedKeyRequest.IdempotencyKey = " " + trimmedRequest.IdempotencyKey + " "
	if _, err := IssueCanvasDecision(evidence, paddedKeyRequest, canvasTestNow); err == nil {
		t.Error("a padded idempotency_key was accepted")
	}
}

// The golden vectors are shared with wiki-store (WS19-S3), which recomputes
// the same bytes; change the digests only on an intended wire change, on both
// sides. The vectors were computed independently of this package (see the
// file's comment and W3-FIX-5's depth note), so this is the drift check.
func TestCanvasPayloadDigestMatchesTheGoldenVectors(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "space-canvas-decision.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden struct {
		Input struct {
			OrgID                     string `json:"org_id"`
			UserID                    string `json:"user_id"`
			SpaceID                   string `json:"space_id"`
			SpaceDecisionRef          string `json:"space_decision_ref"`
			RecipientAudienceRef      string `json:"recipient_audience_ref"`
			RecipientAudienceHash     string `json:"recipient_audience_hash"`
			PrivacyPolicyRef          string `json:"privacy_policy_ref"`
			ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
			AuthorityRevision         int64  `json:"authority_revision"`
			RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
		} `json:"input"`
		Vectors []struct {
			Name                 string `json:"name"`
			Op                   string `json:"op"`
			ActionSchemaHash     string `json:"action_schema_hash"`
			IdempotencyKey       string `json:"idempotency_key"`
			CanvasID             string `json:"canvas_id"`
			ExpectedVersionID    string `json:"expected_version_id"`
			RestoreFromVersionID string `json:"restore_from_version_id"`
			ContentDigest        string `json:"content_digest"`
			PayloadDigest        string `json:"payload_digest"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if len(golden.Vectors) != 4 {
		t.Fatalf("golden vectors = %d, want 4", len(golden.Vectors))
	}
	in := golden.Input

	evidence := validSharedCanvasEvidence(RoleEditor)
	evidence.Membership.OrgID = in.OrgID
	evidence.Membership.SubjectID = in.UserID
	evidence.Membership.SpaceRef = in.SpaceID
	evidence.Membership.Revisions.Authority = in.AuthorityRevision
	evidence.Membership.Revisions.RecipientAudience = in.RecipientAudienceRevision
	evidence.RecipientAudienceRef = in.RecipientAudienceRef
	evidence.RecipientAudienceHash = in.RecipientAudienceHash
	evidence.Privacy.PolicyRef = in.PrivacyPolicyRef
	evidence.ResourceAuthorizationRef = in.ResourceAuthorizationRef

	for _, v := range golden.Vectors {
		t.Run(v.Name, func(t *testing.T) {
			request := CanvasDecisionRequest{
				DecisionRef: in.SpaceDecisionRef, Op: v.Op, CanvasID: v.CanvasID,
				ExpectedVersionID: v.ExpectedVersionID, RestoreFromVersionID: v.RestoreFromVersionID,
				ContentDigest: v.ContentDigest, IdempotencyKey: v.IdempotencyKey, Nonce: "nonce-1",
			}
			decision, err := IssueCanvasDecision(evidence, request, canvasTestNow)
			if err != nil {
				t.Fatalf("IssueCanvasDecision: %v", err)
			}
			if decision.PayloadDigest != v.PayloadDigest {
				t.Errorf("digest = %s, want %s (change only on an intended wire change, on both sides)", decision.PayloadDigest, v.PayloadDigest)
			}
			if decision.ActionSchemaHash != v.ActionSchemaHash {
				t.Errorf("schema = %s, want %s", decision.ActionSchemaHash, v.ActionSchemaHash)
			}
			wantAction := canvasReadAction
			if v.Op != CanvasOpRead {
				wantAction = canvasWriteAction
			}
			if decision.ActionID != wantAction {
				t.Errorf("action = %s, want %s", decision.ActionID, wantAction)
			}
			if err := decision.Validate(); err != nil {
				t.Errorf("issued decision does not validate: %v", err)
			}
		})
	}
}

// Live tier: what only the repository join can answer. Gated on
// TEST_DATABASE_URL via roomLiveFixture (skipped without it).

// TestResolveSharedCanvasDecisionEvidenceLive proves the shared canvas
// resolver's current-recipient-audience join, its own canvas_entitled bit and
// resource reference, and WS04-S7's read/write split: a read resolves an
// archived room and a write resolves only an active one.
func TestResolveSharedCanvasDecisionEvidenceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "canvaslive")
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)

	// An editor in the audience resolves both operations, with the canvas
	// resource ref and the entitlement bit on.
	for _, write := range []bool{false, true} {
		evidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, write)
		if err != nil {
			t.Fatalf("write=%v: %v", write, err)
		}
		if evidence.Membership.Role != "editor" || evidence.Membership.Kind != KindRoom || !evidence.CanvasEntitled ||
			!strings.Contains(evidence.ResourceAuthorizationRef, ":canvas:") {
			t.Fatalf("write=%v evidence = %+v", write, evidence)
		}
	}
	// A viewer resolves a read but not a write; the role floor is the issuer's.
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.viewer, false); err != nil {
		t.Fatalf("viewer read: %v", err)
	}
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.viewer, true); err == nil {
		t.Fatal("a viewer resolved a canvas write")
	}

	// A member outside the published recipient audience resolves nothing, for
	// either operation.
	late := f.user(t, "canvaslive-late", f.org)
	f.member(t, s.ref, late, "editor", SourceMemberOps)
	for _, write := range []bool{false, true} {
		if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, late, write); !errors.Is(err, ErrNoCurrentMembership) {
			t.Fatalf("non-recipient write=%v: err = %v, want ErrNoCurrentMembership", write, err)
		}
	}

	// Archiving the room: a read still resolves, a write is refused.
	if _, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner)); err != nil {
		t.Fatalf("archive: %v", err)
	}
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false); err != nil {
		t.Fatalf("archived room canvas read: %v", err)
	}
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("archived room canvas write: err = %v, want ErrNoCurrentMembership", err)
	}

	// Unarchiving restores the write.
	if _, err := f.repo.UnarchiveSpace(ctx, f.archiveReq(s.ref, s.manager)); err != nil {
		t.Fatalf("unarchive: %v", err)
	}
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true); err != nil {
		t.Fatalf("restored room canvas write: %v", err)
	}

	// The entitlement off makes the resolver fail with the typed refusal.
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=FALSE WHERE org_id=$1`, f.org)
	_, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false)
	wantDenial(t, err, DenialEntitlementOff)
}

func TestResolveSharedCanvasDecisionEvidenceRefusesAPersonalSpaceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "canvaslivep")
	f.exec(t, `UPDATE registered_spaces SET space_kind='personal' WHERE space_ref=$1`, s.ref)
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.owner, false); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("personal Space: err = %v, want ErrNoCurrentMembership", err)
	}
}
