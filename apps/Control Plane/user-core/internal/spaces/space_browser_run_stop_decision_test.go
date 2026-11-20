package spaces

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// D21h: the starter Stop decision for a room browser run
// (model.space.browser_run_stop).

func browserRunStopRoomEvidence() PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room-1"
	e.Membership.Role = RoleViewer
	e.RecipientAudienceRef = "audience:space-room-1:4"
	e.RecipientAudienceHash = "sha256:audience-hash"
	e.Membership.Revisions.RecipientAudience = 4
	e.ResourceAuthorizationRef = "control:space-room-1:browser-run-stop:7"
	// No entitlement bit is read: every one is off here on purpose.
	e.ThreadCreateEntitled = false
	e.BrowserEntitled = false
	e.RunControlEntitled = false
	return e
}

func validBrowserRunStopRequest() BrowserRunStopDecisionRequest {
	return BrowserRunStopDecisionRequest{
		DecisionRef: "stop-decision-1", RunID: "run-browser-1",
		IdempotencyKey: "stop-1", Nonce: "nonce-1",
	}
}

func TestBrowserRunStopDecisionAdmitsEveryRoomRoleWithNoEntitlement(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	tests := []struct {
		kind Kind
		role string
	}{
		{KindRoom, RoleViewer},
		{KindRoom, RoleEditor},
		{KindRoom, RoleManager},
		{KindRoom, RoleOwner},
		{KindProject, RoleViewer},
		{KindCase, RoleEditor},
	}
	for _, test := range tests {
		t.Run(string(test.kind)+"/"+test.role, func(t *testing.T) {
			evidence := browserRunStopRoomEvidence()
			evidence.Membership.Kind = test.kind
			evidence.Membership.Role = test.role
			decision, claims, err := IssueBrowserRunStopDecision(evidence, validBrowserRunStopRequest(), now)
			if err != nil {
				t.Fatalf("IssueBrowserRunStopDecision: %v", err)
			}
			if decision.ActionID != "model.space.browser_run_stop" ||
				decision.ServiceAudience != "model-plane-model-gateway" ||
				decision.ActionSchemaHash != "sha256:space-browser-run-stop-v1" {
				t.Fatalf("decision binds action=%q audience=%q schema=%q", decision.ActionID, decision.ServiceAudience, decision.ActionSchemaHash)
			}
			if !reflect.DeepEqual(decision.Permissions, []string{"space:browser:stop"}) {
				t.Fatalf("permissions = %v, want exactly space:browser:stop", decision.Permissions)
			}
			if decision.SubjectID != "user-1" || decision.SpaceRef != "space-room-1" || decision.IdempotencyKey != "stop-1" {
				t.Fatalf("decision binds subject=%q space=%q key=%q", decision.SubjectID, decision.SpaceRef, decision.IdempotencyKey)
			}
			if claims.RunID != "run-browser-1" {
				t.Fatalf("claims.run_id = %q", claims.RunID)
			}
			if decision.PayloadDigest != browserRunStopPayloadDigest(evidence, validBrowserRunStopRequest(), claims) {
				t.Fatal("decision digest must be the stop payload digest")
			}
			if decision.ZeroDataRetention {
				t.Fatal("a non-ZDR Space must not sign a ZDR claim")
			}
		})
	}
}

func TestBrowserRunStopDecisionRefusals(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	tests := []struct {
		name   string
		mutate func(*PersonalThreadDecisionEvidence)
		want   DenialCode
	}{
		{"personal Space", func(e *PersonalThreadDecisionEvidence) {
			e.Membership.Kind = KindPersonal
			e.Membership.Role = RoleOwner
		}, DenialKindUnsupported},
		{"direct message", func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindDM }, DenialKindUnsupported},
		{"group direct message", func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindGroupDM }, DenialKindUnsupported},
		{"zero data retention", func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, DenialZDRForbidsEffect},
		{"no recipient audience", func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceHash = "" }, DenialAuthorityIncomplete},
		{"no resource authorization", func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = " " }, DenialAuthorityIncomplete},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			evidence := browserRunStopRoomEvidence()
			test.mutate(&evidence)
			_, _, err := IssueBrowserRunStopDecision(evidence, validBrowserRunStopRequest(), now)
			denial := wantDenial(t, err, test.want)
			if test.want == DenialZDRForbidsEffect && denial.Effect != "browser_run_stop" {
				t.Fatalf("effect = %q, want browser_run_stop", denial.Effect)
			}
		})
	}
}

func TestBrowserRunStopRequestValidation(t *testing.T) {
	tests := []struct {
		name   string
		mutate func(*BrowserRunStopDecisionRequest)
		ok     bool
	}{
		{"valid", func(*BrowserRunStopDecisionRequest) {}, true},
		{"run id at the limit", func(r *BrowserRunStopDecisionRequest) { r.RunID = strings.Repeat("r", 128) }, true},
		{"missing run id", func(r *BrowserRunStopDecisionRequest) { r.RunID = "" }, false},
		{"run id too long", func(r *BrowserRunStopDecisionRequest) { r.RunID = strings.Repeat("r", 129) }, false},
		{"padded run id", func(r *BrowserRunStopDecisionRequest) { r.RunID = " run-browser-1" }, false},
		{"run id with NUL", func(r *BrowserRunStopDecisionRequest) { r.RunID = "run\x00x" }, false},
		{"invalid UTF-8 run id", func(r *BrowserRunStopDecisionRequest) { r.RunID = "run-\xff" }, false},
		{"missing idempotency key", func(r *BrowserRunStopDecisionRequest) { r.IdempotencyKey = "" }, false},
		{"padded idempotency key", func(r *BrowserRunStopDecisionRequest) { r.IdempotencyKey = "stop-1 " }, false},
		{"missing decision ref", func(r *BrowserRunStopDecisionRequest) { r.DecisionRef = "" }, false},
		{"missing nonce", func(r *BrowserRunStopDecisionRequest) { r.Nonce = "" }, false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := validBrowserRunStopRequest()
			test.mutate(&request)
			if err := request.Validate(); (err == nil) != test.ok {
				t.Fatalf("Validate() = %v, want ok=%v", err, test.ok)
			}
		})
	}
	unknownRole := browserRunStopRoomEvidence()
	unknownRole.Membership.Role = "guest"
	if _, _, err := IssueBrowserRunStopDecision(unknownRole, validBrowserRunStopRequest(), time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)); err == nil {
		t.Fatal("an unknown room role must be refused")
	}
	if _, _, err := IssueBrowserRunStopDecision(browserRunStopRoomEvidence(), validBrowserRunStopRequest(), time.Time{}); err == nil {
		t.Fatal("a zero issuance time must be refused")
	}
}

// A stop token must not be usable as a run-control or browser-run token, and the
// digest must bind the run, the caller and the idempotency key.
func TestBrowserRunStopDecisionIsDisjointAndBound(t *testing.T) {
	if browserRunStopAction == browserRunAction || browserRunStopAction == runControlAction {
		t.Fatal("the stop action must differ from the browser run and run-control actions")
	}
	if browserRunStopSchema == browserRunSchema || browserRunStopSchema == runControlSchema {
		t.Fatal("the stop schema must differ from the browser run and run-control schemas")
	}
	if BrowserRunStopPermission == browserPermissionRun || BrowserRunStopPermission == RunControlPermission {
		t.Fatal("the stop permission must differ from space:browser:run and run:cancel")
	}
	evidence := browserRunStopRoomEvidence()
	request := validBrowserRunStopRequest()
	base := browserRunStopPayloadDigest(evidence, request, BrowserRunStopClaims{RunID: request.RunID})
	if other := browserRunStopPayloadDigest(evidence, request, BrowserRunStopClaims{RunID: "run-browser-2"}); other == base {
		t.Fatal("the digest must bind the run id")
	}
	otherSubject := evidence
	otherSubject.Membership.SubjectID = "user-9"
	if other := browserRunStopPayloadDigest(otherSubject, request, BrowserRunStopClaims{RunID: request.RunID}); other == base {
		t.Fatal("the digest must bind the caller")
	}
	otherKey := request
	otherKey.IdempotencyKey = "stop-2"
	if other := browserRunStopPayloadDigest(evidence, otherKey, BrowserRunStopClaims{RunID: request.RunID}); other == base {
		t.Fatal("the digest must bind the idempotency key")
	}
	padded := evidence
	padded.RecipientAudienceRef = " " + evidence.RecipientAudienceRef + "\t"
	padded.ResourceAuthorizationRef = "\n" + evidence.ResourceAuthorizationRef
	if got := browserRunStopPayloadDigest(padded, request, BrowserRunStopClaims{RunID: request.RunID}); got != base {
		t.Fatal("padded references must hash as their trimmed values, as the decision carries them")
	}
}

// D21h: the shared Control/Rust golden. The digests were computed by a
// standalone script, independent of this package; Go must reproduce them, and
// the model-gateway verifier must pin the same bytes.
func TestBrowserRunStopDigestGolden(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "space-browser-run-stop-decision.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden struct {
		Action     string `json:"action"`
		Audience   string `json:"audience"`
		Schema     string `json:"schema"`
		Permission string `json:"permission"`
		Input      struct {
			OrgID                     string `json:"org_id"`
			SpaceID                   string `json:"space_id"`
			RecipientAudienceRef      string `json:"recipient_audience_ref"`
			RecipientAudienceHash     string `json:"recipient_audience_hash"`
			PrivacyPolicyRef          string `json:"privacy_policy_ref"`
			ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
			AuthorityRevision         int64  `json:"authority_revision"`
			MembershipRevision        int64  `json:"membership_revision"`
			PrivacyRevision           int64  `json:"privacy_revision"`
			RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
			EntitlementRevision       int64  `json:"entitlement_revision"`
		} `json:"input"`
		Vectors []struct {
			Name           string `json:"name"`
			UserID         string `json:"user_id"`
			RunID          string `json:"run_id"`
			IdempotencyKey string `json:"idempotency_key"`
			PayloadDigest  string `json:"payload_digest"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if golden.Action != browserRunStopAction || golden.Audience != browserRunStopAudience ||
		golden.Schema != browserRunStopSchema || golden.Permission != BrowserRunStopPermission {
		t.Fatalf("golden constants %q/%q/%q/%q differ from the issuer", golden.Action, golden.Audience, golden.Schema, golden.Permission)
	}
	if len(golden.Vectors) < 4 {
		t.Fatalf("golden carries %d vectors, want at least 4", len(golden.Vectors))
	}
	in := golden.Input
	seen := map[string]string{}
	for _, vector := range golden.Vectors {
		vector := vector
		t.Run(vector.Name, func(t *testing.T) {
			evidence := PersonalThreadDecisionEvidence{
				Membership: CurrentMembership{
					OrgID: in.OrgID, SubjectID: vector.UserID, SpaceRef: in.SpaceID,
					Revisions: AuthorityRevision{
						Authority: in.AuthorityRevision, Membership: in.MembershipRevision,
						Privacy: in.PrivacyRevision, RecipientAudience: in.RecipientAudienceRevision,
						Entitlement: in.EntitlementRevision,
					},
				},
				RecipientAudienceRef:     in.RecipientAudienceRef,
				RecipientAudienceHash:    in.RecipientAudienceHash,
				ResourceAuthorizationRef: in.ResourceAuthorizationRef,
				Privacy:                  PrivacyPolicySnapshot{PolicyRef: in.PrivacyPolicyRef},
			}
			got := browserRunStopPayloadDigest(evidence,
				BrowserRunStopDecisionRequest{IdempotencyKey: vector.IdempotencyKey},
				BrowserRunStopClaims{RunID: vector.RunID})
			if got != vector.PayloadDigest {
				t.Fatalf("browserRunStopPayloadDigest = %s, golden %s (change only on an intended wire change, on both sides)", got, vector.PayloadDigest)
			}
			if previous, dup := seen[got]; dup {
				t.Fatalf("vector %s shares its digest with %s", vector.Name, previous)
			}
			seen[got] = vector.Name
		})
	}
}
