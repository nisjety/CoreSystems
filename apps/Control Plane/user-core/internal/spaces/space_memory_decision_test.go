package spaces

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// EP-8 S7: the Space memory management decision (pure logic). The repository
// join is covered by the live tier; this pins the request shape, the role
// floor, the disjoint permission and the digest binding.

func spaceMemoryEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.ResourceAuthorizationRef = "control:space-room:memory-manage:7"
	return e
}

func spaceMemoryRequest(operation, memoryID string) SpaceMemoryDecisionRequest {
	return SpaceMemoryDecisionRequest{
		DecisionRef: "decision-1", Operation: operation, MemoryID: memoryID,
		IdempotencyKey: "decision-1", Nonce: "nonce-1",
	}
}

func TestSpaceMemoryRequestShapeIsClosed(t *testing.T) {
	for name, request := range map[string]SpaceMemoryDecisionRequest{
		"list":        spaceMemoryRequest("list", ""),
		"delete":      spaceMemoryRequest("delete", "mem-1"),
		"erase agent": spaceMemoryRequest("erase_agent", "agent-1"),
		"longest id":  spaceMemoryRequest("delete", strings.Repeat("m", MaxSpaceMemoryIDLen)),
	} {
		if err := request.Validate(); err != nil {
			t.Errorf("%s refused: %v", name, err)
		}
	}
	for name, request := range map[string]SpaceMemoryDecisionRequest{
		"unknown operation":          spaceMemoryRequest("purge", ""),
		"operation case":             spaceMemoryRequest("LIST", ""),
		"list with a memory id":      spaceMemoryRequest("list", "mem-1"),
		"delete without an id":       spaceMemoryRequest("delete", ""),
		"delete with a blank id":     spaceMemoryRequest("delete", "   "),
		"erase agent without an id":  spaceMemoryRequest("erase_agent", ""),
		"id with surrounding spaces": spaceMemoryRequest("delete", " mem-1"),
		"id too long":                spaceMemoryRequest("delete", strings.Repeat("m", MaxSpaceMemoryIDLen+1)),
		"no decision ref":            {Operation: "list", IdempotencyKey: "k", Nonce: "n"},
		"no idempotency key":         {DecisionRef: "d", Operation: "list", Nonce: "n"},
		"no nonce":                   {DecisionRef: "d", Operation: "list", IdempotencyKey: "k"},
	} {
		if err := request.Validate(); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

func TestSpaceMemoryManageNeedsManagerOrOwnerOfASharedSpace(t *testing.T) {
	for _, role := range []string{"manager", "owner"} {
		if err := spaceMemoryEvidence(role).ValidateForSpaceMemoryManage(); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
		m := spaceMemoryEvidence(role).Membership
		if err := ValidateMembershipForSpaceMemoryManage(&m); err != nil {
			t.Errorf("%s refused early: %v", role, err)
		}
	}
	for _, role := range []string{"editor", "viewer"} {
		denial := wantDenial(t, spaceMemoryEvidence(role).ValidateForSpaceMemoryManage(), DenialRoleBelowFloor)
		if strings.Join(denial.RequiredRoles, ",") != "manager,owner" {
			t.Errorf("%s: required roles = %v", role, denial.RequiredRoles)
		}
		m := spaceMemoryEvidence(role).Membership
		wantDenial(t, ValidateMembershipForSpaceMemoryManage(&m), DenialRoleBelowFloor)
	}

	personal := spaceMemoryEvidence("owner")
	personal.Membership.Kind = KindPersonal
	wantDenial(t, personal.ValidateForSpaceMemoryManage(), DenialKindUnsupported)
	wantDenial(t, ValidateMembershipForSpaceMemoryManage(&personal.Membership), DenialKindUnsupported)

	if err := ValidateMembershipForSpaceMemoryManage(nil); !errors.Is(err, ErrNoCurrentMembership) {
		t.Errorf("nil membership = %v, want ErrNoCurrentMembership", err)
	}

	noAudience := spaceMemoryEvidence("owner")
	noAudience.RecipientAudienceHash = ""
	if err := noAudience.ValidateForSpaceMemoryManage(); err == nil {
		t.Error("evidence without the current recipient audience accepted")
	}
	noAuthority := spaceMemoryEvidence("owner")
	noAuthority.ResourceAuthorizationRef = ""
	if err := noAuthority.ValidateForSpaceMemoryManage(); err == nil {
		t.Error("evidence without the memory authority ref accepted")
	}
}

func TestSpaceMemoryDecisionCarriesOnlyMemoryManageForModelPlane(t *testing.T) {
	now := time.Date(2026, 11, 12, 10, 0, 0, 0, time.UTC)
	evidence := spaceMemoryEvidence("manager")
	evidence.Privacy.ZeroDataRetention = true
	decision, err := IssueSpaceMemoryDecision(evidence, spaceMemoryRequest("delete", "mem-1"), now)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != SpaceMemoryPermission {
		t.Errorf("permissions = %v, want only %s", decision.Permissions, SpaceMemoryPermission)
	}
	if decision.ServiceAudience != "model-plane" || decision.ActionID != "model.space.memory.manage" ||
		decision.ActionSchemaHash != "sha256:space-memory-manage-v1" {
		t.Errorf("audience/action/schema = %q %q %q", decision.ServiceAudience, decision.ActionID, decision.ActionSchemaHash)
	}
	if !decision.ZeroDataRetention {
		t.Error("the Space ZDR claim was dropped from the decision")
	}

	if _, err := IssueSpaceMemoryDecision(spaceMemoryEvidence("editor"), spaceMemoryRequest("list", ""), now); err == nil {
		t.Error("an editor obtained a memory decision")
	}
	if _, err := IssueSpaceMemoryDecision(evidence, spaceMemoryRequest("purge", ""), now); err == nil {
		t.Error("an unknown operation obtained a decision")
	}
	if _, err := IssueSpaceMemoryDecision(evidence, spaceMemoryRequest("list", ""), time.Time{}); err == nil {
		t.Error("a zero issuance time was accepted")
	}
}

// The vectors below were computed by an independent re-implementation of
// session-core's space_memory_payload_digest (grpc.rs), not by this package, so
// a drift on either side of the Control/Session Core boundary shows up here.
func TestSpaceMemoryDigestMatchesTheSessionCoreVector(t *testing.T) {
	now := time.Date(2026, 11, 12, 10, 0, 0, 0, time.UTC)
	for _, tc := range []struct {
		request SpaceMemoryDecisionRequest
		want    string
	}{
		{spaceMemoryRequest("list", ""), "sha256:5bf224bf6e271964b70bb20f57c55dcc46a52dfcf4cddaebc8b938804e35e66f"},
		{spaceMemoryRequest("delete", "mem-1"), "sha256:dc8907af220f4ac9c256af89fc214baa2554189fa479ad96f0d5f4eabc354302"},
	} {
		decision, err := IssueSpaceMemoryDecision(spaceMemoryEvidence("owner"), tc.request, now)
		if err != nil {
			t.Fatalf("%s: %v", tc.request.Operation, err)
		}
		if decision.PayloadDigest != tc.want {
			t.Errorf("%s digest = %s, want %s", tc.request.Operation, decision.PayloadDigest, tc.want)
		}
	}
}

func TestSpaceMemoryDigestBindsOperationAndMemoryID(t *testing.T) {
	now := time.Date(2026, 11, 12, 10, 0, 0, 0, time.UTC)
	evidence := spaceMemoryEvidence("owner")
	digests := map[string]string{}
	for name, request := range map[string]SpaceMemoryDecisionRequest{
		"list":          spaceMemoryRequest("list", ""),
		"delete mem-1":  spaceMemoryRequest("delete", "mem-1"),
		"delete mem-2":  spaceMemoryRequest("delete", "mem-2"),
		"erase agent-1": spaceMemoryRequest("erase_agent", "mem-1"),
	} {
		decision, err := IssueSpaceMemoryDecision(evidence, request, now)
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

	// A newer authority revision changes the digest, so a token minted before a
	// role change does not verify against the current revision.
	bumped := spaceMemoryEvidence("owner")
	bumped.Membership.Revisions.Authority++
	decision, err := IssueSpaceMemoryDecision(bumped, spaceMemoryRequest("list", ""), now)
	if err != nil {
		t.Fatal(err)
	}
	if decision.PayloadDigest == digests["list"] {
		t.Error("the authority revision is not bound by the digest")
	}
}
