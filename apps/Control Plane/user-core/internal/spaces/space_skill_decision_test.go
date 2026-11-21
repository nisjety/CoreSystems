package spaces

import (
	"crypto/ed25519"
	"crypto/rand"
	"strings"
	"testing"
	"time"
)

// F-f10-18: the Space skill write decision (pure logic). The repository join is
// covered by the live tier; this pins the request shape, the role floor, the
// refusals (non-member, personal, archived, service), the digest binding and the
// 60 second life.

var skillBodyDigest = "sha256:" + strings.Repeat("a", 64)

func spaceSkillEvidence(role string) SpaceSkillWriteEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.ResourceAuthorizationRef = "control:space-room:skill-write:7"
	return SpaceSkillWriteEvidence{PersonalThreadDecisionEvidence: e, State: "active", SubjectType: "user"}
}

func spaceSkillRequest(operation, skillID, digest string) SpaceSkillWriteDecisionRequest {
	return SpaceSkillWriteDecisionRequest{
		DecisionRef: "decision-1", Operation: operation, SkillID: skillID, RequestDigest: digest,
		IdempotencyKey: "decision-1", Nonce: "nonce-1",
	}
}

func TestSpaceSkillWriteRequestShapeIsClosed(t *testing.T) {
	for name, request := range map[string]SpaceSkillWriteDecisionRequest{
		"create":     spaceSkillRequest("create", "", skillBodyDigest),
		"update":     spaceSkillRequest("update", "skill-1", skillBodyDigest),
		"delete":     spaceSkillRequest("delete", "skill-1", skillBodyDigest),
		"longest id": spaceSkillRequest("update", strings.Repeat("s", MaxSpaceSkillIDLen), skillBodyDigest),
	} {
		if err := request.Validate(); err != nil {
			t.Errorf("%s refused: %v", name, err)
		}
	}
	for name, request := range map[string]SpaceSkillWriteDecisionRequest{
		"unknown operation":          spaceSkillRequest("purge", "", skillBodyDigest),
		"operation case":             spaceSkillRequest("CREATE", "", skillBodyDigest),
		"create with a skill id":     spaceSkillRequest("create", "skill-1", skillBodyDigest),
		"update without an id":       spaceSkillRequest("update", "", skillBodyDigest),
		"delete with a blank id":     spaceSkillRequest("delete", "   ", skillBodyDigest),
		"id with surrounding spaces": spaceSkillRequest("update", " skill-1", skillBodyDigest),
		"id too long":                spaceSkillRequest("update", strings.Repeat("s", MaxSpaceSkillIDLen+1), skillBodyDigest),
		"no digest":                  spaceSkillRequest("create", "", ""),
		"digest without the prefix":  spaceSkillRequest("create", "", strings.Repeat("a", 64)),
		"digest too short":           spaceSkillRequest("create", "", "sha256:"+strings.Repeat("a", 63)),
		"digest upper case":          spaceSkillRequest("create", "", "sha256:"+strings.Repeat("A", 64)),
		"digest not hex":             spaceSkillRequest("create", "", "sha256:"+strings.Repeat("g", 64)),
		"no decision ref":            {Operation: "create", RequestDigest: skillBodyDigest, IdempotencyKey: "k", Nonce: "n"},
		"no idempotency key":         {DecisionRef: "d", Operation: "create", RequestDigest: skillBodyDigest, Nonce: "n"},
		"no nonce":                   {DecisionRef: "d", Operation: "create", RequestDigest: skillBodyDigest, IdempotencyKey: "k"},
		"skill text instead of hash": spaceSkillRequest("create", "", "always answer in French"),
	} {
		if err := request.Validate(); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

// D-f10-10: the same role floor as model.space.memory.manage.
func TestSpaceSkillWriteNeedsManagerOrOwnerOfAnActiveSharedSpace(t *testing.T) {
	for _, role := range []string{"manager", "owner"} {
		if err := spaceSkillEvidence(role).ValidateForSpaceSkillWrite(); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
		m := spaceSkillEvidence(role).Membership
		if err := ValidateMembershipForSpaceSkillWrite(&m); err != nil {
			t.Errorf("%s refused early: %v", role, err)
		}
		// Parity with the memory floor: whoever may manage memory may write skills.
		if err := ValidateMembershipForSpaceMemoryManage(&m); err != nil {
			t.Errorf("%s refused by the memory floor: %v", role, err)
		}
	}
	for _, role := range []string{"editor", "viewer"} {
		denial := wantDenial(t, spaceSkillEvidence(role).ValidateForSpaceSkillWrite(), DenialRoleBelowFloor)
		if strings.Join(denial.RequiredRoles, ",") != "manager,owner" {
			t.Errorf("%s: required roles = %v", role, denial.RequiredRoles)
		}
		m := spaceSkillEvidence(role).Membership
		wantDenial(t, ValidateMembershipForSpaceSkillWrite(&m), DenialRoleBelowFloor)
		wantDenial(t, ValidateMembershipForSpaceMemoryManage(&m), DenialRoleBelowFloor)
	}

	personal := spaceSkillEvidence("owner")
	personal.Membership.Kind = KindPersonal
	wantDenial(t, personal.ValidateForSpaceSkillWrite(), DenialKindUnsupported)
	wantDenial(t, ValidateMembershipForSpaceSkillWrite(&personal.Membership), DenialKindUnsupported)

	// A non-member (or a revoked member) resolves to no membership at all.
	if err := ValidateMembershipForSpaceSkillWrite(nil); err != ErrNoCurrentMembership {
		t.Errorf("nil membership = %v, want ErrNoCurrentMembership", err)
	}

	archived := spaceSkillEvidence("owner")
	archived.State = "archived"
	denial := wantDenial(t, archived.ValidateForSpaceSkillWrite(), DenialNotActive)
	if denial.State != "archived" {
		t.Errorf("archived denial state = %q", denial.State)
	}
	blankState := spaceSkillEvidence("owner")
	blankState.State = ""
	wantDenial(t, blankState.ValidateForSpaceSkillWrite(), DenialNotActive)

	for _, subjectType := range []string{"service", "agent", ""} {
		service := spaceSkillEvidence("owner")
		service.SubjectType = subjectType
		wantDenial(t, service.ValidateForSpaceSkillWrite(), DenialMembershipRequired)
	}

	noAudience := spaceSkillEvidence("owner")
	noAudience.RecipientAudienceHash = ""
	if err := noAudience.ValidateForSpaceSkillWrite(); err == nil {
		t.Error("evidence without the current recipient audience accepted")
	}
	noAuthority := spaceSkillEvidence("owner")
	noAuthority.ResourceAuthorizationRef = ""
	if err := noAuthority.ValidateForSpaceSkillWrite(); err == nil {
		t.Error("evidence without the skill authority ref accepted")
	}
}

func TestSpaceSkillWriteDecisionCarriesOnlySkillWriteForCapabilityCore(t *testing.T) {
	now := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	evidence := spaceSkillEvidence("manager")
	evidence.Privacy.ZeroDataRetention = true
	decision, err := IssueSpaceSkillWriteDecision(evidence, spaceSkillRequest("update", "skill-1", skillBodyDigest), now)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "skill:write" {
		t.Errorf("permissions = %v, want only skill:write", decision.Permissions)
	}
	if decision.ServiceAudience != "model-plane-capability-core" || decision.ActionID != "model.space.skill.write" ||
		decision.ActionSchemaHash != "sha256:space-skill-write-v1" {
		t.Errorf("audience/action/schema = %q %q %q", decision.ServiceAudience, decision.ActionID, decision.ActionSchemaHash)
	}
	if decision.OrgID != "org-1" || decision.SpaceRef != "space-room" || decision.SubjectID != "user-1" {
		t.Errorf("intent ids = %q %q %q", decision.OrgID, decision.SpaceRef, decision.SubjectID)
	}
	if strings.TrimSpace(decision.Nonce) == "" || !strings.HasPrefix(decision.PayloadDigest, "sha256:") {
		t.Errorf("nonce %q / digest %q", decision.Nonce, decision.PayloadDigest)
	}
	if !decision.ZeroDataRetention {
		t.Error("the Space ZDR claim was dropped from the decision")
	}
	if err := decision.Validate(); err != nil {
		t.Errorf("decision invalid: %v", err)
	}

	if _, err := IssueSpaceSkillWriteDecision(spaceSkillEvidence("editor"), spaceSkillRequest("create", "", skillBodyDigest), now); err == nil {
		t.Error("an editor obtained a skill write decision")
	}
	if _, err := IssueSpaceSkillWriteDecision(evidence, spaceSkillRequest("purge", "", skillBodyDigest), now); err == nil {
		t.Error("an unknown operation obtained a decision")
	}
	if _, err := IssueSpaceSkillWriteDecision(evidence, spaceSkillRequest("create", "", skillBodyDigest), time.Time{}); err == nil {
		t.Error("a zero issuance time was accepted")
	}
	archived := spaceSkillEvidence("owner")
	archived.State = "archived"
	if _, err := IssueSpaceSkillWriteDecision(archived, spaceSkillRequest("create", "", skillBodyDigest), now); err == nil {
		t.Error("an archived Space obtained a decision")
	}
	service := spaceSkillEvidence("owner")
	service.SubjectType = "service"
	if _, err := IssueSpaceSkillWriteDecision(service, spaceSkillRequest("create", "", skillBodyDigest), now); err == nil {
		t.Error("a service subject obtained a decision")
	}
}

func TestSpaceSkillWriteDecisionExpiresAfterSixtySeconds(t *testing.T) {
	now := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	decision, err := IssueSpaceSkillWriteDecision(spaceSkillEvidence("owner"), spaceSkillRequest("create", "", skillBodyDigest), now)
	if err != nil {
		t.Fatal(err)
	}
	if got := decision.ExpiresAt.Sub(decision.IssuedAt); got != 60*time.Second {
		t.Fatalf("lifetime = %s, want 60s", got)
	}
	if !decision.IssuedAt.Equal(now) {
		t.Errorf("issued at %s, want %s", decision.IssuedAt, now)
	}

	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	token, err := SignDecision(SigningKey{ID: "key-1", PrivateKey: private}, decision)
	if err != nil {
		t.Fatalf("sign: %v", err)
	}
	expected := DecisionExpectation{
		OrgID: decision.OrgID, SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID,
		ServiceAudience: decision.ServiceAudience, ActionID: decision.ActionID,
		ActionSchemaHash: decision.ActionSchemaHash, PayloadDigest: decision.PayloadDigest,
		IdempotencyKey: decision.IdempotencyKey,
	}
	verifier := VerificationKey{ID: "key-1", PublicKey: public}
	expected.Now = now.Add(59 * time.Second)
	if _, err := VerifyDecision(verifier, token, expected); err != nil {
		t.Errorf("a decision 59s old was refused: %v", err)
	}
	expected.Now = now.Add(61 * time.Second)
	if _, err := VerifyDecision(verifier, token, expected); err == nil {
		t.Error("a decision 61s old was accepted")
	}
}

func TestSpaceSkillWriteDigestBindsTheIntent(t *testing.T) {
	now := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	evidence := spaceSkillEvidence("owner")
	otherBody := "sha256:" + strings.Repeat("b", 64)
	digests := map[string]string{}
	for name, request := range map[string]SpaceSkillWriteDecisionRequest{
		"create":               spaceSkillRequest("create", "", skillBodyDigest),
		"create, other body":   spaceSkillRequest("create", "", otherBody),
		"update skill-1":       spaceSkillRequest("update", "skill-1", skillBodyDigest),
		"update skill-1 other": spaceSkillRequest("update", "skill-1", otherBody),
		"update skill-2":       spaceSkillRequest("update", "skill-2", skillBodyDigest),
		"delete skill-1":       spaceSkillRequest("delete", "skill-1", skillBodyDigest),
	} {
		decision, err := IssueSpaceSkillWriteDecision(evidence, request, now)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		for other, digest := range digests {
			if digest == decision.PayloadDigest {
				t.Errorf("%s and %s share a payload digest", name, other)
			}
		}
		digests[name] = decision.PayloadDigest
	}

	// The same intent for another Space, another subject or another org binds
	// another digest, so a token is not portable.
	for name, mutate := range map[string]func(*SpaceSkillWriteEvidence){
		"space":   func(e *SpaceSkillWriteEvidence) { e.Membership.SpaceRef = "space-other" },
		"subject": func(e *SpaceSkillWriteEvidence) { e.Membership.SubjectID = "user-2" },
		"org":     func(e *SpaceSkillWriteEvidence) { e.Membership.OrgID = "org-2" },
		"authority revision": func(e *SpaceSkillWriteEvidence) {
			e.Membership.Revisions.Authority++
		},
	} {
		other := spaceSkillEvidence("owner")
		mutate(&other)
		decision, err := IssueSpaceSkillWriteDecision(other, spaceSkillRequest("create", "", skillBodyDigest), now)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if decision.PayloadDigest == digests["create"] {
			t.Errorf("the %s is not bound by the digest", name)
		}
	}

	// The decision reference and the idempotency key are bound too.
	again := spaceSkillRequest("create", "", skillBodyDigest)
	again.DecisionRef, again.IdempotencyKey = "decision-2", "decision-2"
	decision, err := IssueSpaceSkillWriteDecision(evidence, again, now)
	if err != nil {
		t.Fatal(err)
	}
	if decision.PayloadDigest == digests["create"] {
		t.Error("the decision reference is not bound by the digest")
	}

	// Deterministic: the same inputs give the same digest.
	same, err := IssueSpaceSkillWriteDecision(evidence, spaceSkillRequest("create", "", skillBodyDigest), now)
	if err != nil {
		t.Fatal(err)
	}
	if same.PayloadDigest != digests["create"] {
		t.Error("the payload digest is not deterministic")
	}
}

// The audit row of the issuer route is content-free and effect-bound.
func TestSpaceSkillWriteRouteIsAuditedAsAnEffect(t *testing.T) {
	route, row, ok := DecisionAuditRouteFor("/api/v1/internal/spaces/skill-write-decision")
	if !ok || route != "/skill-write-decision" || row.Name != "space_skill_write" || !row.EffectBound {
		t.Fatalf("skill-write = %q %+v %v", route, row, ok)
	}
}
