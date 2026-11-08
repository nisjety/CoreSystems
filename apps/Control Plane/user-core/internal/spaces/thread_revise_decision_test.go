package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"reflect"
	"strings"
	"testing"
	"time"
)

// WS12-S5 pure legs of the model.thread.revise decision.

func reviseContentDigest(text string) string {
	sum := sha256.Sum256([]byte(text))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// validSharedReviseEvidence is a room editor with message_revise on and the
// default (purge) original-text rule.
func validSharedReviseEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.Role = "editor"
	evidence.Membership.Revisions.RecipientAudience = 4
	evidence.RecipientAudienceRef = "space:room-1:recipient-audience:4"
	evidence.RecipientAudienceHash = "sha256:shared-audience"
	evidence.ThreadCreateEntitled = false
	evidence.MessageReviseEntitled = true
	evidence.OriginalRetention = OriginalRetentionPurge
	evidence.ResourceAuthorizationRef = "control:room-1:thread-revise:7"
	return evidence
}

func validReviseRequest() ThreadReviseDecisionRequest {
	return ThreadReviseDecisionRequest{
		DecisionRef: "decision-1", ThreadID: "thread-1", MessageID: "msg-1", Action: ThreadReviseActionEdit,
		ContentDigest: reviseContentDigest("edited text"), IdempotencyKey: "revise-1", Nonce: "nonce-1",
	}
}

// The golden below is the byte layout documented in
// apps/Model Plane/rust/crates/mp-contracts/src/message_revision.rs and
// recomputed by session-core thread_revise_payload_digest. It was produced by
// an independent implementation of that layout (not by this function), and the
// session-core test thread_revise_digest_matches_control_golden asserts the
// same hex against the same inputs. Change both together or neither.
const threadReviseGoldenDigest = "sha256:450775ecf96b23929bd99d5cbaa3ef4c7047b247d6c3351c1d69ed5cfab3ccee"

func TestThreadReviseDigestGolden(t *testing.T) {
	if got := reviseContentDigest("edited text"); got != "sha256:bc72610fa4011f57aa4b3dc6a5f0d1861ef6516d89522441582fb4e3b3450d3b" {
		t.Fatalf("golden input drifted: %s", got)
	}
	got := ThreadRevisePayloadDigest(validSharedReviseEvidence(), validReviseRequest())
	if got != threadReviseGoldenDigest {
		t.Fatalf("revise payload digest = %s, want golden %s", got, threadReviseGoldenDigest)
	}
	decision, err := IssueThreadReviseDecision(validSharedReviseEvidence(), validReviseRequest(), time.Now().UTC())
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if decision.PayloadDigest != threadReviseGoldenDigest {
		t.Fatalf("decision digest = %s", decision.PayloadDigest)
	}
	if decision.ActionID != "model.thread.revise" || decision.ActionSchemaHash != "sha256:thread-revise-v1" || decision.ServiceAudience != "model-plane" {
		t.Fatalf("decision identity: %+v", decision)
	}
}

func TestThreadReviseDigestBindsEveryOperationField(t *testing.T) {
	evidence := validSharedReviseEvidence()
	base := ThreadRevisePayloadDigest(evidence, validReviseRequest())
	for name, mutate := range map[string]func(*ThreadReviseDecisionRequest){
		"thread":      func(r *ThreadReviseDecisionRequest) { r.ThreadID = "thread-2" },
		"message":     func(r *ThreadReviseDecisionRequest) { r.MessageID = "msg-2" },
		"action":      func(r *ThreadReviseDecisionRequest) { r.Action = ThreadReviseActionDelete; r.ContentDigest = emptyContentDigest },
		"content":     func(r *ThreadReviseDecisionRequest) { r.ContentDigest = reviseContentDigest("other") },
		"idempotency": func(r *ThreadReviseDecisionRequest) { r.IdempotencyKey = "revise-2" },
		"decision":    func(r *ThreadReviseDecisionRequest) { r.DecisionRef = "decision-2" },
	} {
		request := validReviseRequest()
		mutate(&request)
		if ThreadRevisePayloadDigest(evidence, request) == base {
			t.Errorf("changing %s must change the digest", name)
		}
	}
	// The nonce is not part of the effect.
	request := validReviseRequest()
	request.Nonce = "another"
	if ThreadRevisePayloadDigest(evidence, request) != base {
		t.Error("the nonce must not change the digest")
	}
	moved := validSharedReviseEvidence()
	moved.Membership.Revisions.Authority++
	if ThreadRevisePayloadDigest(moved, validReviseRequest()) == base {
		t.Error("the authority revision must be bound")
	}
}

func TestThreadReviseIssuedPermissionsMatrix(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	issue := func(mutate func(*PersonalThreadDecisionEvidence)) ([]string, error) {
		evidence := validSharedReviseEvidence()
		mutate(&evidence)
		decision, err := IssueThreadReviseDecision(evidence, validReviseRequest(), now)
		return decision.Permissions, err
	}
	wantPerms := func(name string, mutate func(*PersonalThreadDecisionEvidence), want ...string) {
		t.Helper()
		got, err := issue(mutate)
		if err != nil {
			t.Errorf("%s: unexpected refusal %v", name, err)
			return
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s: permissions = %v, want %v", name, got, want)
		}
	}
	wantDenial := func(name string, mutate func(*PersonalThreadDecisionEvidence), code DenialCode) {
		t.Helper()
		_, err := issue(mutate)
		denial, ok := AsDenial(err)
		if !ok || denial.Code != code {
			t.Errorf("%s: want denial %s, got %v", name, code, err)
		}
	}

	wantPerms("editor, default purge", func(e *PersonalThreadDecisionEvidence) {}, ThreadRevisePermission, RevisePurgeOriginalPermission)
	wantPerms("editor, retain rule", func(e *PersonalThreadDecisionEvidence) { e.OriginalRetention = OriginalRetentionRetain },
		ThreadRevisePermission, ReviseRetainOriginalPermission)
	wantPerms("editor never moderates even when entitled", func(e *PersonalThreadDecisionEvidence) { e.MessageModerateEntitled = true },
		ThreadRevisePermission, RevisePurgeOriginalPermission)
	wantPerms("manager without the moderate entitlement", func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "manager" },
		ThreadRevisePermission, RevisePurgeOriginalPermission)
	for _, role := range []string{"manager", "owner"} {
		role := role
		wantPerms(role+" with moderate", func(e *PersonalThreadDecisionEvidence) {
			e.Membership.Role = role
			e.MessageModerateEntitled = true
		}, ThreadRevisePermission, ThreadModeratePermission, RevisePurgeOriginalPermission)
	}
	wantPerms("owner with moderate and retain", func(e *PersonalThreadDecisionEvidence) {
		e.Membership.Role = "owner"
		e.MessageModerateEntitled = true
		e.OriginalRetention = OriginalRetentionRetain
	}, ThreadRevisePermission, ThreadModeratePermission, ReviseRetainOriginalPermission)
	// A DM never moderates, whatever the role or entitlement.
	for _, kind := range []Kind{KindDM, KindGroupDM} {
		kind := kind
		wantPerms("owner in "+string(kind), func(e *PersonalThreadDecisionEvidence) {
			e.Membership.Kind = kind
			e.Membership.Role = "owner"
			e.MessageModerateEntitled = true
		}, ThreadRevisePermission, RevisePurgeOriginalPermission)
	}
	// A personal Space never moderates.
	personal := validPersonalThreadEvidence()
	personal.MessageReviseEntitled = true
	personal.MessageModerateEntitled = true
	personal.OriginalRetention = OriginalRetentionPurge
	personal.ResourceAuthorizationRef = "control:space-personal:thread-revise:7"
	decision, err := IssueThreadReviseDecision(personal, validReviseRequest(), now)
	if err != nil || !reflect.DeepEqual(decision.Permissions, []string{ThreadRevisePermission, RevisePurgeOriginalPermission}) {
		t.Errorf("personal Space: %v %v", decision.Permissions, err)
	}

	// Refusals are typed.
	wantDenial("no entitlement", func(e *PersonalThreadDecisionEvidence) { e.MessageReviseEntitled = false }, DenialEntitlementOff)
	wantDenial("viewer", func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor)
	wantDenial("ZDR", func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, DenialZDRForbidsEffect)
	wantDenial("create entitlement is not enough", func(e *PersonalThreadDecisionEvidence) {
		e.MessageReviseEntitled = false
		e.ThreadCreateEntitled = true
	}, DenialEntitlementOff)
	if _, err := issue(func(e *PersonalThreadDecisionEvidence) { e.OriginalRetention = "" }); err == nil {
		t.Error("an unresolved retention rule must be refused")
	}
	if _, err := issue(func(e *PersonalThreadDecisionEvidence) { e.OriginalRetention = "keep" }); err == nil {
		t.Error("an unknown retention rule must be refused")
	}
	if _, err := issue(func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = "" }); err == nil {
		t.Error("a missing resource reference must be refused")
	}
	if _, err := issue(func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef = "" }); err == nil {
		t.Error("a shared Space needs its recipient audience")
	}
}

func TestThreadReviseDeleteRequiresTheEmptyContentDigest(t *testing.T) {
	now := time.Now().UTC()
	request := validReviseRequest()
	request.Action = ThreadReviseActionDelete
	if _, err := IssueThreadReviseDecision(validSharedReviseEvidence(), request, now); err == nil {
		t.Fatal("a delete that commits to non-empty content must be refused")
	}
	request.ContentDigest = emptyContentDigest
	if _, err := IssueThreadReviseDecision(validSharedReviseEvidence(), request, now); err != nil {
		t.Fatalf("a delete with the empty digest: %v", err)
	}
	if emptyContentDigest != reviseContentDigest("") {
		t.Fatalf("emptyContentDigest is not the SHA-256 of the empty string: %s", emptyContentDigest)
	}
}

func TestThreadReviseRequestValidation(t *testing.T) {
	now := time.Now().UTC()
	for name, mutate := range map[string]func(*ThreadReviseDecisionRequest){
		"unknown action":  func(r *ThreadReviseDecisionRequest) { r.Action = "redact" },
		"empty action":    func(r *ThreadReviseDecisionRequest) { r.Action = "" },
		"short digest":    func(r *ThreadReviseDecisionRequest) { r.ContentDigest = "sha256:abc" },
		"no prefix":       func(r *ThreadReviseDecisionRequest) { r.ContentDigest = strings.Repeat("a", 71) },
		"missing message": func(r *ThreadReviseDecisionRequest) { r.MessageID = " " },
		"missing thread":  func(r *ThreadReviseDecisionRequest) { r.ThreadID = "" },
		"missing key":     func(r *ThreadReviseDecisionRequest) { r.IdempotencyKey = "" },
		"missing nonce":   func(r *ThreadReviseDecisionRequest) { r.Nonce = "" },
		"missing ref":     func(r *ThreadReviseDecisionRequest) { r.DecisionRef = "" },
	} {
		request := validReviseRequest()
		mutate(&request)
		if err := request.Validate(); err == nil {
			t.Errorf("%s: Validate accepted the request", name)
		}
		if _, err := IssueThreadReviseDecision(validSharedReviseEvidence(), request, now); err == nil {
			t.Errorf("%s: issuance accepted the request", name)
		}
	}
	if _, err := IssueThreadReviseDecision(validSharedReviseEvidence(), validReviseRequest(), time.Time{}); err == nil {
		t.Error("a zero issuance time must be refused")
	}
}

func TestValidOriginalRetentionIsAClosedSet(t *testing.T) {
	if !ValidOriginalRetention("retain") || !ValidOriginalRetention("purge") {
		t.Fatal("retain and purge must be valid")
	}
	for _, bad := range []string{"", "keep", "Retain", "purge "} {
		if ValidOriginalRetention(bad) {
			t.Fatalf("%q must be invalid", bad)
		}
	}
	if err := (MessagePolicy{OrgID: "org-1", OriginalRetention: "retain"}).Validate(); err != nil {
		t.Fatalf("valid policy: %v", err)
	}
	if err := (MessagePolicy{OrgID: " ", OriginalRetention: "retain"}).Validate(); err == nil {
		t.Fatal("a policy needs an organization")
	}
	if err := (MessagePolicy{OrgID: "org-1", OriginalRetention: "x"}).Validate(); err == nil {
		t.Fatal("a policy needs a closed rule")
	}
}
