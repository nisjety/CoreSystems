package spaces

import "testing"

// A-REC-03: RunActionReadAdmitted needs the golden row, the operator allowlist
// and the row's role floor, all three.
func TestRunActionReadAdmitted(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "")
	if RunActionReadAdmitted("space.thread.rename", RoleOwner) {
		t.Fatal("admitted with an empty allowlist")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename,space.directory.resolve")
	if !RunActionReadAdmitted("space.thread.rename", RoleEditor) {
		t.Fatal("refused an allowlisted golden row at its floor")
	}
	if RunActionReadAdmitted("space.thread.rename", RoleViewer) {
		t.Fatal("admitted below the row's role floor")
	}
	if RunActionReadAdmitted("space.thread.rename", "") {
		t.Fatal("admitted an empty role")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename,space.not.a.golden.row")
	if RunActionReadAdmitted("space.not.a.golden.row", RoleOwner) {
		t.Fatal("admitted an action the golden does not name")
	}
	// space.directory.resolve is vendored (A-REC-03-TOOL): a run action at the
	// viewer floor, admitted for any current member once the allowlist names it,
	// and refused while it does not.
	spec, found := runActionSpec("space.directory.resolve")
	if !found || spec.DecisionType != runActionDecisionKind || spec.MinRole != RoleViewer ||
		spec.AutonomyRung != "read_only" || spec.Approval != "none" || spec.ServiceAudience != "control-plane-user-core" {
		t.Fatalf("directory golden row = %+v (found %v)", spec, found)
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename")
	if RunActionReadAdmitted("space.directory.resolve", RoleOwner) {
		t.Fatal("admitted the directory row the allowlist does not name")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.directory.resolve")
	for _, role := range []string{RoleViewer, RoleEditor, RoleManager, RoleOwner} {
		if !RunActionReadAdmitted("space.directory.resolve", role) {
			t.Fatalf("refused the vendored directory row for a %s", role)
		}
	}
	if RunActionReadAdmitted("space.directory.resolve", "") {
		t.Fatal("admitted the directory row for an empty role")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.schedule.follow_up")
	if RunActionReadAdmitted("space.schedule.follow_up", RoleOwner) {
		t.Fatal("admitted a schedule_create row as a run action read")
	}
}
