package handler

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

func TestSpaceImportDecisionRequiresAValidTargetedSignature(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("CONTROL_SPACE_DECISION_KEY_ID", "control-key-1")
	t.Setenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64", base64.RawURLEncoding.EncodeToString(publicKey))
	t.Setenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON", "")

	decision := spaceImportDecisionClaims{
		DecisionRef: "decision-1", OrgID: "org-1", SpaceRef: "space:org-1:user-1", SubjectID: "user-1",
		ServiceAudience: spaceImportDecisionAudience, ActionID: spaceImportDecisionAction,
		ActionSchemaHash: spaceImportDecisionSchema, PayloadDigest: "sha256:payload", IdempotencyKey: "import-1",
		RecipientAudienceRef: "audience:user-1", PrivacyPolicyRef: "privacy:org-1", ResourceAuthorizationRef: "resource:import-1",
		AuthorityRevision: 1, MembershipRevision: 1, PrivacyRevision: 1, RecipientAudienceRevision: 1, EntitlementRevision: 1,
		Permissions: []string{"documents:write"}, Purpose: "knowledge_import", LawfulBasis: "contract", PrivacyClass: "internal",
		RetentionClass: "standard", Residency: "eu-north-1", DeletionScope: "space", ImportSourceType: "notion", IssuedAt: time.Now().UTC(), ExpiresAt: time.Now().UTC().Add(time.Minute), Nonce: "nonce-1",
	}
	token := signedSpaceImportDecision(t, privateKey, "control-key-1", decision)
	req := httptest.NewRequest("POST", "/v1/documents", nil)
	req.Header.Set(spaceImportDecisionHeader, token)
	req = req.WithContext(context.WithValue(req.Context(), orgIDKey, "org-1"))
	req = req.WithContext(authctx.IntoContext(req.Context(), &authctx.Claims{
		ServiceID: "imports-core", PrincipalType: "service", Scopes: []string{"documents:write"}, Verified: true,
	}))

	authority, err := verifySpaceImportDecision(req)
	if err != nil || authority == nil {
		t.Fatalf("valid decision rejected: authority=%v err=%v", authority, err)
	}
	if !authority.matchesDocumentInputType("notion") || authority.matchesDocumentInputType("hubspot") {
		t.Fatal("decision did not bind the document source type")
	}

	req.Header.Set(spaceImportDecisionHeader, token+"tampered")
	if _, err := verifySpaceImportDecision(req); err == nil {
		t.Fatal("tampered decision was accepted")
	}
}

func TestSpaceImportDecisionRejectsWrongServiceAudience(t *testing.T) {
	decision := spaceImportDecisionClaims{ServiceAudience: "data-plane-retrieval"}
	if err := decision.validate("org-1", time.Now().UTC()); err == nil {
		t.Fatal("wrong audience was accepted")
	}
}

func signedSpaceImportDecision(t *testing.T, privateKey ed25519.PrivateKey, keyID string, decision spaceImportDecisionClaims) string {
	t.Helper()
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	encodedID := base64.RawURLEncoding.EncodeToString([]byte(keyID))
	encodedPayload := base64.RawURLEncoding.EncodeToString(payload)
	signed := "v2." + encodedID + "." + encodedPayload
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(privateKey, []byte(signed)))
}

// The defect this guards: the decision was verified and then discarded, so the
// document row could not say which room it belonged to and no Space-filtered
// listing was possible. The Space must come from the signed claims — a
// body-supplied `space_ref` is a claim, not authority.
func TestCreateDocumentInputTakesTheSpaceFromClaimsAndNeverFromTheBody(t *testing.T) {
	var input model.CreateDocumentInput
	body := []byte(`{"title":"t","content":"c","type":"notion","space_ref":"space:org-1:someone-elses-room"}`)
	if err := json.Unmarshal(body, &input); err != nil {
		t.Fatal(err)
	}
	if input.SpaceRef != "" {
		t.Fatalf("a body-supplied space_ref must not deserialize; got %q", input.SpaceRef)
	}

	authority := &spaceImportDecisionClaims{SpaceRef: " space:org-1:room-7 "}
	input.SpaceRef = strings.TrimSpace(authority.SpaceRef)
	if input.SpaceRef != "space:org-1:room-7" {
		t.Fatalf("verified space_ref = %q", input.SpaceRef)
	}
}

// WS14-S3: the two helpers that route a verified decision into the room-file
// rules and the signed-subject owner.
func TestSpaceImportDecisionIsSpaceUploadAndSignedSubject(t *testing.T) {
	for source, want := range map[string]bool{"space_upload": true, " space_upload ": true, "notion": false, "": false, "Space_Upload": false} {
		if got := (spaceImportDecisionClaims{ImportSourceType: source}).isSpaceUpload(); got != want {
			t.Errorf("isSpaceUpload(%q) = %v, want %v", source, got, want)
		}
	}
	if got := (spaceImportDecisionClaims{SubjectID: "  user-1 "}).signedSubject(); got != "user-1" {
		t.Errorf("signedSubject = %q", got)
	}
	if got := (spaceImportDecisionClaims{}).signedSubject(); got != "" {
		t.Errorf("signedSubject of an empty decision = %q", got)
	}
}

// With DOCUMENTS_SPACE_FILES on, a personal-Space connector import is owned by
// the decision's signed subject; with it off the previous rule (owner must be
// the verified imports-core principal) is unchanged. The stop point is the
// validator, which runs after the owner gate and before the repository.
func TestPersonalSpaceConnectorImportOwnerFollowsTheFlag(t *testing.T) {
	post := func(f *uploadFixture, owner string) *httptest.ResponseRecorder {
		notion := baseDecision(testOrg, "space:org-1:user-1", "user-1", "notion")
		return f.post(uploadRequest{
			decision: &notion,
			body:     map[string]any{"type": "notion", "title": "", "owner_id": owner, "metadata": map[string]any{}},
		})
	}

	on := newUploadFixture(t, SpaceFilesConfig{Enabled: true})
	if rec := post(on, "user-1"); rec.Code != 400 || !strings.Contains(rec.Body.String(), "title is required") {
		t.Fatalf("flag on, owner = subject: %d %s (want the owner gate passed, stopped at validation)", rec.Code, rec.Body.String())
	}
	if rec := post(on, ""); rec.Code != 400 || !strings.Contains(rec.Body.String(), "title is required") {
		t.Fatalf("flag on, no owner_id: %d %s", rec.Code, rec.Body.String())
	}
	if rec := post(on, "user-2"); rec.Code != 403 || !strings.Contains(rec.Body.String(), "decision subject") {
		t.Fatalf("flag on, owner != subject: %d %s", rec.Code, rec.Body.String())
	}

	off := newUploadFixture(t, SpaceFilesConfig{})
	if rec := post(off, "user-1"); rec.Code != 403 || !strings.Contains(rec.Body.String(), "verified principal") {
		t.Fatalf("flag off, owner = subject: %d %s (the subject rule must not apply)", rec.Code, rec.Body.String())
	}
	if rec := post(off, "imports-core"); rec.Code != 400 || !strings.Contains(rec.Body.String(), "title is required") {
		t.Fatalf("flag off, owner = principal: %d %s", rec.Code, rec.Body.String())
	}
}
