package users

import "testing"

func TestSessionOnboardingStatusKeepsCompletedWithoutMembership(t *testing.T) {
	status := sessionOnboardingStatus(true, nil)
	if status != "COMPLETED" {
		t.Fatalf("expected COMPLETED, got %s", status)
	}
}

func TestSessionOnboardingStatusCreatedRequiresIncompleteUserWithoutMembership(t *testing.T) {
	status := sessionOnboardingStatus(false, nil)
	if status != "CREATED" {
		t.Fatalf("expected CREATED, got %s", status)
	}
}

func TestSessionOnboardingStatusProfileReadyRequiresMembership(t *testing.T) {
	status := sessionOnboardingStatus(false, &UserOrgMembership{OrgID: "org_1", Role: "owner"})
	if status != "PROFILE_READY" {
		t.Fatalf("expected PROFILE_READY, got %s", status)
	}
}
