package users

import (
	"context"
	"strings"
	"testing"
)

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

func TestRemoveMembershipValidatesExactIdentifiersAndRepository(t *testing.T) {
	service := &Service{}
	tests := []struct {
		name   string
		userID string
		orgID  string
		want   string
	}{
		{name: "missing user", userID: " ", orgID: "org-1", want: "user ID and org ID are required"},
		{name: "missing org", userID: "user-1", orgID: " ", want: "user ID and org ID are required"},
		{name: "missing repository", userID: "user-1", orgID: "org-1", want: "membership repository is unavailable"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			err := service.RemoveMembership(context.Background(), test.userID, test.orgID)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("RemoveMembership() error = %v, want %q", err, test.want)
			}
		})
	}
}
