package spaces

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type roleMatrixGolden struct {
	Version         int            `json:"version"`
	Ranks           map[string]int `json:"ranks"`
	ElevatedMinRank int            `json:"elevated_min_rank"`
	Vectors         struct {
		Add []struct {
			Actor   string  `json:"actor"`
			NewRole string  `json:"new_role"`
			Denial  *string `json:"denial"`
		} `json:"add"`
		ChangeRole []struct {
			Actor         string  `json:"actor"`
			Target        string  `json:"target"`
			NewRole       string  `json:"new_role"`
			TargetIsActor bool    `json:"target_is_actor"`
			ActiveOwners  int     `json:"active_owners"`
			Denial        *string `json:"denial"`
		} `json:"change_role"`
		Remove []struct {
			Actor         string  `json:"actor"`
			Target        string  `json:"target"`
			TargetIsActor bool    `json:"target_is_actor"`
			ActiveOwners  int     `json:"active_owners"`
			Denial        *string `json:"denial"`
		} `json:"remove"`
		Join []struct {
			JoinRole string  `json:"join_role"`
			Denial   *string `json:"denial"`
		} `json:"join"`
	} `json:"vectors"`
}

func loadRoleMatrixGolden(t *testing.T) roleMatrixGolden {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("testdata", "room-role-matrix.v1.json"))
	if err != nil {
		t.Fatalf("read room role matrix golden: %v", err)
	}
	var golden roleMatrixGolden
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode room role matrix golden: %v", err)
	}
	return golden
}

func denialCode(d *RoleDenial) string {
	if d == nil {
		return ""
	}
	return d.Code
}

func wantCode(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

// The golden is shared byte for byte with the SPA mirror
// (scripts/tests/space-contract-vectors-sync.test.sh). Every vector must
// match the Go role matrix, so a drift in either place is caught.
func TestRoomRoleMatrixGoldenVectors(t *testing.T) {
	golden := loadRoleMatrixGolden(t)
	if golden.Version != 1 {
		t.Fatalf("unexpected golden version %d", golden.Version)
	}
	for role, rank := range golden.Ranks {
		if RoleRank(role) != rank {
			t.Fatalf("rank of %s: golden %d, code %d", role, rank, RoleRank(role))
		}
	}
	if golden.ElevatedMinRank != RoleRank(RoleManager) {
		t.Fatalf("elevated_min_rank %d is not the manager rank", golden.ElevatedMinRank)
	}
	if len(golden.Vectors.Add) == 0 || len(golden.Vectors.ChangeRole) == 0 || len(golden.Vectors.Remove) == 0 || len(golden.Vectors.Join) == 0 {
		t.Fatal("the golden must carry the four vector groups")
	}
	for _, v := range golden.Vectors.Add {
		if got := denialCode(CanAdd(v.Actor, v.NewRole)); got != wantCode(v.Denial) {
			t.Errorf("CanAdd(%s,%s) = %q, want %q", v.Actor, v.NewRole, got, wantCode(v.Denial))
		}
	}
	for _, v := range golden.Vectors.ChangeRole {
		if got := denialCode(CanChangeRole(v.Actor, v.Target, v.NewRole, v.TargetIsActor, v.ActiveOwners)); got != wantCode(v.Denial) {
			t.Errorf("CanChangeRole(%s,%s,%s,self=%v,owners=%d) = %q, want %q", v.Actor, v.Target, v.NewRole, v.TargetIsActor, v.ActiveOwners, got, wantCode(v.Denial))
		}
	}
	for _, v := range golden.Vectors.Remove {
		if got := denialCode(CanRemove(v.Actor, v.Target, v.TargetIsActor, v.ActiveOwners)); got != wantCode(v.Denial) {
			t.Errorf("CanRemove(%s,%s,self=%v,owners=%d) = %q, want %q", v.Actor, v.Target, v.TargetIsActor, v.ActiveOwners, got, wantCode(v.Denial))
		}
	}
	for _, v := range golden.Vectors.Join {
		if got := denialCode(CanJoin(v.JoinRole)); got != wantCode(v.Denial) {
			t.Errorf("CanJoin(%s) = %q, want %q", v.JoinRole, got, wantCode(v.Denial))
		}
	}
}

// The named rules from the plan, asserted directly so they hold even if the
// golden were regenerated from a wrong implementation.
func TestRoomRoleMatrixNamedRules(t *testing.T) {
	cases := []struct {
		name string
		got  *RoleDenial
		want string
	}{
		{"viewer cannot add", CanAdd(RoleViewer, RoleViewer), CodeRoomRoleForbidden},
		{"editor cannot add", CanAdd(RoleEditor, RoleViewer), CodeRoomRoleForbidden},
		{"manager adds viewer", CanAdd(RoleManager, RoleViewer), ""},
		{"manager adds editor", CanAdd(RoleManager, RoleEditor), ""},
		{"manager cannot grant manager", CanAdd(RoleManager, RoleManager), CodeRoomElevatedNeedsOwner},
		{"manager cannot grant owner", CanAdd(RoleManager, RoleOwner), CodeRoomElevatedNeedsOwner},
		{"owner grants owner", CanAdd(RoleOwner, RoleOwner), ""},
		{"manager cannot touch an owner", CanChangeRole(RoleManager, RoleOwner, RoleViewer, false, 2), CodeRoomElevatedNeedsOwner},
		{"manager cannot promote to manager", CanChangeRole(RoleManager, RoleEditor, RoleManager, false, 1), CodeRoomElevatedNeedsOwner},
		{"manager cannot remove a manager", CanRemove(RoleManager, RoleManager, false, 1), CodeRoomElevatedNeedsOwner},
		{"self demotion is allowed", CanChangeRole(RoleManager, RoleManager, RoleEditor, true, 1), ""},
		{"self promotion to elevated is refused", CanChangeRole(RoleEditor, RoleEditor, RoleManager, true, 1), CodeRoomElevatedNeedsOwner},
		{"self promotion to editor is refused", CanChangeRole(RoleViewer, RoleViewer, RoleEditor, true, 1), CodeRoomRoleForbidden},
		{"sole owner cannot self demote", CanChangeRole(RoleOwner, RoleOwner, RoleManager, true, 1), CodeRoomSoleOwner},
		{"owner can self demote with a second owner", CanChangeRole(RoleOwner, RoleOwner, RoleManager, true, 2), ""},
		{"sole owner cannot be demoted by another owner", CanChangeRole(RoleOwner, RoleOwner, RoleEditor, false, 1), CodeRoomSoleOwner},
		{"sole owner cannot leave", CanRemove(RoleOwner, RoleOwner, true, 1), CodeRoomSoleOwner},
		{"owner can leave with a second owner", CanRemove(RoleOwner, RoleOwner, true, 2), ""},
		{"viewer can leave", CanRemove(RoleViewer, RoleViewer, true, 1), ""},
		{"editor can leave", CanRemove(RoleEditor, RoleEditor, true, 1), ""},
		{"viewer cannot remove others", CanRemove(RoleViewer, RoleViewer, false, 1), CodeRoomRoleForbidden},
		{"join as viewer", CanJoin(RoleViewer), ""},
		{"join as editor", CanJoin(RoleEditor), ""},
		{"join as manager refused", CanJoin(RoleManager), CodeRoomNotJoinable},
		{"join as owner refused", CanJoin(RoleOwner), CodeRoomNotJoinable},
		{"join as unknown role refused", CanJoin("admin"), CodeRoomNotJoinable},
		{"viewer cannot manage room", CanManageRoom(RoleViewer), CodeRoomRoleForbidden},
		{"editor cannot manage room", CanManageRoom(RoleEditor), CodeRoomRoleForbidden},
		{"manager manages room", CanManageRoom(RoleManager), ""},
		{"owner manages room", CanManageRoom(RoleOwner), ""},
		{"unknown actor role cannot manage room", CanManageRoom("admin"), CodeRoomRoleForbidden},
	}
	for _, tc := range cases {
		if got := denialCode(tc.got); got != tc.want {
			t.Errorf("%s: got %q, want %q", tc.name, got, tc.want)
		}
	}
	if d := CanRemove(RoleOwner, RoleOwner, true, 1); d == nil || d.Status != 409 {
		t.Errorf("sole owner denial must be a 409, got %+v", d)
	}
	if d := CanAdd(RoleViewer, RoleViewer); d == nil || d.Status != 403 {
		t.Errorf("role denial must be a 403, got %+v", d)
	}
}

func TestRoleRankAndElevation(t *testing.T) {
	if RoleRank("admin") != 0 || ValidRole("admin") || ValidRole("") {
		t.Fatal("unknown roles must rank 0 and be invalid")
	}
	for role, elevated := range map[string]bool{RoleViewer: false, RoleEditor: false, RoleManager: true, RoleOwner: true, "admin": false} {
		if IsElevatedRole(role) != elevated {
			t.Fatalf("IsElevatedRole(%s) = %v", role, !elevated)
		}
	}
}
