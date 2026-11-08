package spaces

import (
	"context"
	"errors"
	"testing"
)

// WS10-S5a live proof of the tool policy store. Gated on TEST_DATABASE_URL via
// roomLiveFixture (migrations applied, per-run suffix, cleaned up after).

func (f *roomLiveFixture) toolPolicyCleanup(t *testing.T) {
	t.Helper()
	t.Cleanup(func() {
		ctx := context.Background()
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_tool_policies WHERE space_ref LIKE 'ws04s4-%-'||$1`, f.suffix)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_tool_policy_sets WHERE space_ref LIKE 'ws04s4-%-'||$1`, f.suffix)
	})
}

// serviceAgent makes subject an active service member of the room.
func (f *roomLiveFixture) serviceAgent(t *testing.T, ref, subject string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'service',$2,'editor','ws10s5a-test',$3)`, ref, subject, SourceMemberOps)
}

func policyUpdate(revision int64, entries ...ToolPolicyEntry) ToolPolicyUpdate {
	return ToolPolicyUpdate{Entries: entries, ExpectedRevision: revision}
}

func TestToolPolicyFirstWriteNeedsRevisionZeroAndStaleConflictsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.toolPolicyCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "tpol")
	write := func(rev int64, entries ...ToolPolicyEntry) (ToolPolicySet, error) {
		return f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerSpaceCeiling, "", policyUpdate(rev, entries...))
	}

	// Nothing stored: revision 0, empty entries.
	set, err := f.repo.GetToolPolicySet(ctx, s.ref, f.org, ToolPolicyLayerSpaceCeiling, "")
	if err != nil || set.Revision != 0 || len(set.Entries) != 0 {
		t.Fatalf("empty layer: %+v %v", set, err)
	}
	// A first write that presents revision 1 is stale.
	if _, err := write(1, entry("shell", ToolLevelBlock)); !errors.Is(err, ErrToolPolicyConflict) {
		t.Fatalf("first write with expected_revision 1: %v", err)
	}
	set, err = write(0, entry("shell", ToolLevelBlock), entry("*", ToolLevelAsk))
	if err != nil || set.Revision != 1 || len(set.Entries) != 2 || set.UpdatedBy != s.manager {
		t.Fatalf("first write: %+v %v", set, err)
	}
	// Replaying revision 0 is now stale; the layer is untouched.
	if _, err := write(0, entry("shell", ToolLevelAllow)); !errors.Is(err, ErrToolPolicyConflict) {
		t.Fatalf("stale write: %v", err)
	}
	set, _ = f.repo.GetToolPolicySet(ctx, s.ref, f.org, ToolPolicyLayerSpaceCeiling, "")
	if set.Revision != 1 || len(set.Entries) != 2 {
		t.Fatalf("a refused write changed the layer: %+v", set)
	}
	// Emptying the layer still advances the revision.
	set, err = write(1)
	if err != nil || set.Revision != 2 || len(set.Entries) != 0 {
		t.Fatalf("emptying: %+v %v", set, err)
	}
	// A whole-layer replace drops rows the new body does not name.
	set, err = write(2, entry("read", ToolLevelAsk))
	if err != nil || set.Revision != 3 || len(set.Entries) != 1 || set.Entries[0].ToolKey != "read" {
		t.Fatalf("replace: %+v %v", set, err)
	}
}

func TestToolPolicyAgentLayerNeedsAnActiveServiceMemberAndIsIndependentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.toolPolicyCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "tpolag")
	agentA, agentB := "ws10s5a-agent-a-"+f.suffix, "ws10s5a-agent-b-"+f.suffix
	f.serviceAgent(t, s.ref, agentA)
	f.serviceAgent(t, s.ref, agentB)

	// A human member (or any non-member) is not an agent of the Space.
	for _, notAgent := range []string{s.editor, "ws10s5a-ghost-" + f.suffix} {
		_, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerAgent, notAgent, policyUpdate(0, entry("x", ToolLevelBlock)))
		if !errors.Is(err, ErrApprovalAgentNotInSpace) {
			t.Fatalf("agent layer for %q: %v", notAgent, err)
		}
	}
	// An inactive service member is refused too.
	f.exec(t, `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, s.ref, agentB)
	if _, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerAgent, agentB, policyUpdate(0, entry("x", ToolLevelBlock))); !errors.Is(err, ErrApprovalAgentNotInSpace) {
		t.Fatalf("inactive agent: %v", err)
	}
	f.exec(t, `UPDATE space_memberships SET active=TRUE WHERE space_ref=$1 AND subject_id=$2`, s.ref, agentB)

	// Layers are independent per agent and from the ceiling: each starts at revision 0.
	a, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerAgent, agentA, policyUpdate(0, entry("send", ToolLevelBlock)))
	if err != nil || a.Revision != 1 || a.AgentSubjectID != agentA {
		t.Fatalf("agent A: %+v %v", a, err)
	}
	b, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.owner, ToolPolicyLayerAgent, agentB, policyUpdate(0, entry("send", ToolLevelAsk)))
	if err != nil || b.Revision != 1 {
		t.Fatalf("agent B starts at revision 0 regardless of A: %+v %v", b, err)
	}
	c, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerSpaceCeiling, "", policyUpdate(0, entry("*", ToolLevelAsk)))
	if err != nil || c.Revision != 1 {
		t.Fatalf("ceiling starts at revision 0 regardless of agents: %+v %v", c, err)
	}
	// The ceiling layer must not name an agent.
	if _, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerSpaceCeiling, agentA, policyUpdate(1)); err == nil {
		t.Fatal("the ceiling layer must refuse an agent subject")
	}

	list, err := f.repo.ListAgentToolPolicySets(ctx, s.ref, f.org)
	if err != nil || len(list) != 2 {
		t.Fatalf("agent layers: %+v %v", list, err)
	}
	merged, err := f.repo.MergedToolPolicyFor(ctx, s.ref, f.org, agentA)
	if err != nil {
		t.Fatalf("merged: %v", err)
	}
	if got := merged.LevelFor("send"); got.Level != ToolLevelBlock || got.Source != ToolPolicyLayerAgent {
		t.Fatalf("merged send for agent A: %+v", got)
	}
	if got := merged.LevelFor("other"); got.Level != ToolLevelAsk || got.Source != ToolPolicyLayerSpaceCeiling {
		t.Fatalf("merged other for agent A falls back to the ceiling *: %+v", got)
	}
	noAgent, _ := f.repo.MergedToolPolicyFor(ctx, s.ref, f.org, "")
	if noAgent.AgentRevision != 0 || noAgent.LevelFor("send").Level != ToolLevelAsk {
		t.Fatalf("no agent named: %+v", noAgent)
	}
}

func TestToolPolicyWriteInAnotherOrganizationIsRefusedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.toolPolicyCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "tpolorg")
	_, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.other, s.manager, ToolPolicyLayerSpaceCeiling, "", policyUpdate(0, entry("x", ToolLevelBlock)))
	if !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("wrong org: %v", err)
	}
	set, _ := f.repo.GetToolPolicySet(ctx, s.ref, f.org, ToolPolicyLayerSpaceCeiling, "")
	if set.Revision != 0 {
		t.Fatalf("a refused write must store nothing: %+v", set)
	}
	// An unknown Space is the same refusal.
	_, err = f.repo.ReplaceToolPolicySet(ctx, "ws04s4-nope-"+f.suffix, f.org, s.manager, ToolPolicyLayerSpaceCeiling, "", policyUpdate(0))
	if !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("unknown Space: %v", err)
	}
	// Invalid bodies fail before any transaction.
	if _, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, s.manager, ToolPolicyLayerSpaceCeiling, "", policyUpdate(0, entry("a", "deny"))); err == nil {
		t.Fatal("an unknown level must be refused")
	}
	if _, err := f.repo.ReplaceToolPolicySet(ctx, s.ref, f.org, "  ", ToolPolicyLayerSpaceCeiling, "", policyUpdate(0)); err == nil {
		t.Fatal("a blank actor must be refused")
	}
}
