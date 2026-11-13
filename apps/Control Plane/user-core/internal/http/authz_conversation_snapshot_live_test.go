package http

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// W4-SH-3 live leg (TEST_DATABASE_URL via d12Pool): a conversation_snapshot
// grant answers allowed only while the grantee is a CURRENT active member of
// the org. A grant row that outlives the membership grants nothing, and a
// document grant of the same user keeps answering from the grant alone.
func TestConversationSnapshotCheckRequiresCurrentOrgMembershipLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("w4sh3c")
	seedW2Room(t, pool, repo, room, nil)
	s := liveGrantServer(pool, repo)
	ctx := context.Background()
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, room.Org) })

	for _, g := range []users.ResourceGrant{
		{OrgID: room.Org, ResourceType: "conversation_snapshot", ResourceID: "snap-1", SubjectType: "user", SubjectID: room.Viewer, Role: "view"},
		{OrgID: room.Org, ResourceType: "document", ResourceID: "doc-1", SubjectType: "user", SubjectID: room.Viewer, Role: "view"},
	} {
		grant := g
		if _, err := s.aclRepo.Grant(ctx, &grant); err != nil {
			t.Fatalf("seed %s grant: %v", g.ResourceType, err)
		}
	}

	check := func(resourceType, resourceID string) (bool, string) {
		t.Helper()
		c, w := newGinCtx("/api/v1/internal/authz/check?org_id=" + room.Org + "&resource_type=" + resourceType +
			"&resource_id=" + resourceID + "&subject_id=" + room.Viewer)
		s.authzCheck(c)
		if w.Code != http.StatusOK {
			t.Fatalf("check %s: %d %s", resourceType, w.Code, w.Body.String())
		}
		var resp struct {
			Allowed bool   `json:"allowed"`
			Role    string `json:"role"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
			t.Fatalf("decode: %v", err)
		}
		return resp.Allowed, resp.Role
	}

	if allowed, role := check("conversation_snapshot", "snap-1"); !allowed || role != "view" {
		t.Fatalf("active member: allowed=%v role=%q, want true view", allowed, role)
	}

	if _, err := pool.Exec(ctx, `UPDATE user_org_memberships SET status='suspended' WHERE org_id=$1 AND user_id=$2`, room.Org, room.Viewer); err != nil {
		t.Fatalf("remove membership: %v", err)
	}
	if allowed, role := check("conversation_snapshot", "snap-1"); allowed || role != "" {
		t.Fatalf("suspended member: allowed=%v role=%q, want false and no role", allowed, role)
	}
	// Only the snapshot type is gated at read time; a document grant is unchanged.
	if allowed, role := check("document", "doc-1"); !allowed || role != "view" {
		t.Fatalf("document grant of a removed member: allowed=%v role=%q, want true view (unchanged)", allowed, role)
	}
}
