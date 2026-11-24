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
	if RunActionReadAdmitted("space.directory.resolve", RoleOwner) {
		t.Fatal("admitted an action the golden does not name")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "space.schedule.follow_up")
	if RunActionReadAdmitted("space.schedule.follow_up", RoleOwner) {
		t.Fatal("admitted a schedule_create row as a run action read")
	}
}
