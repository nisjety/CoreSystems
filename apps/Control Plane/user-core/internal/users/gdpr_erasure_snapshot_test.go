package users

import (
	"strings"
	"testing"
)

// TestErasureOrganizationSnapshotReachesEveryMembershipRow pins P2-cp-04-rel1. The fan-out
// snapshot names every organisation the erased user has a membership row in, whatever its
// status, because org-core erases a removed member's browser-site choices and history only
// through a child of this snapshot. A status filter here (the snapshot was limited to
// status='active') leaves those choices behind. The Postgres-backed saga test checks the rows
// the query returns; this one fails fast without a database.
func TestErasureOrganizationSnapshotReachesEveryMembershipRow(t *testing.T) {
	for _, required := range []string{
		"FROM user_org_memberships",
		"WHERE user_id=$1",
		"UNION ALL SELECT $2",
	} {
		if !strings.Contains(erasureOrganizationSnapshotSQL, required) {
			t.Fatalf("erasure organisation snapshot is missing %q", required)
		}
	}
	if strings.Contains(strings.ToLower(erasureOrganizationSnapshotSQL), "status") {
		t.Fatal("erasure organisation snapshot filters memberships by status; a removed membership would never be erased")
	}
}
