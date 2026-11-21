package org

import (
	"os"
	"strings"
	"testing"
)

func TestComplianceRoleEnabledIsTheStrictLiteral(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "True": false, "1": false, "on": false, "": false} {
		got := ComplianceRoleEnabled(func(string) string { return value })
		if got != want {
			t.Fatalf("%q: got %v want %v", value, got, want)
		}
	}
}

func TestComplianceIsAssignableOnlyWithTheSwitchOn(t *testing.T) {
	for _, role := range []string{"owner", "admin", "member", "viewer"} {
		if !IsAssignableMemberRole(role, false) || !IsAssignableMemberRole(role, true) {
			t.Fatalf("%s is a base tier and stays assignable either way", role)
		}
	}
	if IsAssignableMemberRole(RoleCompliance, false) {
		t.Fatal("compliance must be refused while the switch is off")
	}
	if !IsAssignableMemberRole(RoleCompliance, true) {
		t.Fatal("compliance must be accepted while the switch is on")
	}
	for _, role := range []string{"", "Compliance", "auditor", "owner,admin"} {
		if IsAssignableMemberRole(role, true) {
			t.Fatalf("%q must never be assignable", role)
		}
	}
}

// The membership projection CHECK (015, widened by 033) must list compliance, and
// its down file must put the four-role list back.
func TestComplianceMigrationWidensTheProjectionCheck(t *testing.T) {
	up, err := os.ReadFile("../../migrations/033_org_compliance_role.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	down, err := os.ReadFile("../../migrations/033_org_compliance_role.down.sql")
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{
		"DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check",
		"ADD CONSTRAINT auth_membership_projection_role_state_check",
		"'owner', 'admin', 'member', 'viewer', 'compliance'",
	} {
		if !strings.Contains(string(up), required) {
			t.Fatalf("up migration is missing %q", required)
		}
	}
	if strings.Contains(string(down), "compliance'") {
		t.Fatal("down migration must not list compliance in the CHECK")
	}
	if !strings.Contains(string(down), "'owner', 'admin', 'member', 'viewer'\n") {
		t.Fatal("down migration must restore the four-role CHECK")
	}
}
