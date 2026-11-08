package spaces

import (
	"context"
	"reflect"
	"testing"
)

// WS04-S8 live proofs: the room directory, self-join, room settings and the
// per-organization directory policy. Gated on TEST_DATABASE_URL (a disposable
// Postgres; every user-core migration is applied by the fixture). Reuses the
// WS04-S4 roomLiveFixture, whose cleanup covers every ws04s4-* Space and user.

func (f *roomLiveFixture) setSpace(t *testing.T, ref, set string, args ...any) {
	t.Helper()
	f.exec(t, `UPDATE registered_spaces SET `+set+` WHERE space_ref=$1`, append([]any{ref}, args...)...)
}

func (f *roomLiveFixture) policy(t *testing.T, org string, enabled bool) {
	t.Helper()
	if _, err := f.repo.UpsertDirectoryPolicy(context.Background(), DirectoryPolicy{OrgID: org, OrgPublicRoomsEnabled: enabled}, "control-space-policy"); err != nil {
		t.Fatalf("set directory policy: %v", err)
	}
	t.Cleanup(func() {
		_, _ = f.pool.Exec(context.Background(), `DELETE FROM space_directory_policy_events WHERE org_id=$1`, org)
		_, _ = f.pool.Exec(context.Background(), `DELETE FROM space_directory_policies WHERE org_id=$1`, org)
	})
}

func (f *roomLiveFixture) openRoom(t *testing.T, name, owner string) string {
	t.Helper()
	ref := f.room(t, name, owner)
	f.setSpace(t, ref, `visibility='org_public'`)
	return ref
}

func (f *roomLiveFixture) count(t *testing.T, query string, args ...any) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(), query, args...).Scan(&n); err != nil {
		t.Fatalf("count %q: %v", query, err)
	}
	return n
}

func (f *roomLiveFixture) listed(t *testing.T, org, caller string) *DirectoryRooms {
	t.Helper()
	out, err := f.repo.ListDirectoryRooms(context.Background(), org, caller)
	if err != nil {
		t.Fatalf("ListDirectoryRooms: %v", err)
	}
	return out
}

func refs(rooms []DirectoryRoom) []string {
	out := []string{}
	for _, room := range rooms {
		out = append(out, room.SpaceRef)
	}
	return out
}

func TestDirectoryListsOnlyOpenActiveManagedRoomsTheCallerHasNotJoinedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	owner := f.user(t, "dir-owner", f.org)
	caller := f.user(t, "dir-caller", f.org)
	outsider := f.user(t, "dir-outsider", f.other)

	open := f.openRoom(t, "dir-open", owner)
	f.room(t, "dir-private", owner)
	archived := f.openRoom(t, "dir-archived", owner)
	f.setSpace(t, archived, `registration_state='archived'`)
	suspended := f.openRoom(t, "dir-suspended", owner)
	f.setSpace(t, suspended, `registration_state='suspended'`)
	orgRoom := f.openRoom(t, "dir-orgroom", owner)
	f.setSpace(t, orgRoom, `roster_mode='org_derived'`)
	joined := f.openRoom(t, "dir-joined", owner)
	f.member(t, joined, caller, "viewer", SourceMemberOps)
	personalOwner := f.user(t, "dir-personal-owner", f.org)
	personal := f.openRoom(t, "dir-personal", personalOwner)
	f.setSpace(t, personal, `space_kind='personal'`)
	f.setSpace(t, open, `join_role='viewer'`)
	f.member(t, open, f.user(t, "dir-extra", f.org), "editor", SourceMemberOps)

	// No policy row: honest "off", never an error, never a room.
	off := f.listed(t, f.org, caller)
	if off.Enabled || off.Rooms == nil || len(off.Rooms) != 0 || off.Truncated {
		t.Fatalf("no policy row: %+v", off)
	}

	f.policy(t, f.org, true)
	on := f.listed(t, f.org, caller)
	if !on.Enabled || !reflect.DeepEqual(refs(on.Rooms), []string{open}) {
		t.Fatalf("enabled directory = %+v, want only %s (private, archived, suspended, personal, org-derived and joined rooms are never listed)", on, open)
	}
	if on.Rooms[0].JoinRole != "viewer" || on.Rooms[0].MemberCount != 2 || on.Rooms[0].RegisteredAt.IsZero() {
		t.Fatalf("room facts = %+v", on.Rooms[0])
	}
	// A member of the open room no longer sees it.
	for _, ref := range refs(f.listed(t, f.org, owner).Rooms) {
		if ref == open {
			t.Fatal("a room's own owner must not see it in the directory")
		}
	}

	// A user of another org lists nothing: that org has no policy row.
	if got := f.listed(t, f.other, outsider); got.Enabled || len(got.Rooms) != 0 {
		t.Fatalf("other org: %+v", got)
	}
	// Claiming this org without belonging to it is refused.
	if _, err := f.repo.ListDirectoryRooms(context.Background(), f.org, outsider); err == nil {
		t.Fatal("a non-member of the org must be refused")
	}
	// Turning the policy off hides every room again.
	f.policy(t, f.org, false)
	if got := f.listed(t, f.org, caller); got.Enabled || len(got.Rooms) != 0 {
		t.Fatalf("policy off: %+v", got)
	}
}

func TestJoinRefusalsAreByteEqualRoomNotFoundLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	owner := f.user(t, "jr-owner", f.org)
	joiner := f.user(t, "jr-joiner", f.org)
	otherOwner := f.user(t, "jr-other-owner", f.other)
	otherJoiner := f.user(t, "jr-other-joiner", f.other)
	f.policy(t, f.org, true) // f.other has no policy row

	private := f.room(t, "jr-private", owner)
	personalOwner := f.user(t, "jr-personal-owner", f.org)
	personal := f.openRoom(t, "jr-personal", personalOwner)
	f.setSpace(t, personal, `space_kind='personal'`)
	orgDerived := f.openRoom(t, "jr-orgroom", owner)
	f.setSpace(t, orgDerived, `roster_mode='org_derived'`)
	suspended := f.openRoom(t, "jr-suspended", owner)
	f.setSpace(t, suspended, `registration_state='suspended'`)
	// An open room of the org whose policy is off.
	disabledRoom := "ws04s4-jr-disabled-" + f.suffix
	f.exec(t, `INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state, visibility)
	           VALUES ($1,$2,'room',$3,1,'active','org_public')`, disabledRoom, f.other, otherOwner)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, disabledRoom)
	open := f.openRoom(t, "jr-open", owner)

	var want *RoomError
	record := func(name string, err error) {
		t.Helper()
		got := wantRoomErr(t, err, 404, CodeRoomNotFound)
		if want == nil {
			want = got
			return
		}
		if !reflect.DeepEqual(want, got) {
			t.Errorf("%s answered %+v, want the byte-equal %+v", name, got, want)
		}
	}
	join := func(ref, org, actor string) error {
		_, err := f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: ref, OrgID: org, ActorID: actor})
		return err
	}
	record("unknown ref", join("ws04s4-nope-"+f.suffix, f.org, joiner))
	record("private room", join(private, f.org, joiner))
	record("personal Space", join(personal, f.org, joiner))
	record("org-derived room", join(orgDerived, f.org, joiner))
	record("suspended room", join(suspended, f.org, joiner))
	record("organization with the directory off", join(disabledRoom, f.other, otherJoiner))
	record("another org's room", join(open, f.other, otherJoiner))
	record("caller outside the org", join(open, f.org, otherJoiner))

	// None of them wrote anything.
	if n := f.count(t, `SELECT count(*) FROM space_membership_events WHERE action='member_joined' AND space_ref LIKE 'ws04s4-jr-%-'||$1`, f.suffix); n != 0 {
		t.Fatalf("refused joins wrote %d ledger rows", n)
	}
}

func TestJoinGrantsTheJoinRoleThroughOneMembershipRowAndOneLedgerRowLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	owner := f.user(t, "jn-owner", f.org)
	joiner := f.user(t, "jn-joiner", f.org)
	f.policy(t, f.org, true)
	open := f.openRoom(t, "jn-open", owner)
	f.setSpace(t, open, `join_role='viewer'`)

	grantsBefore := f.count(t, `SELECT count(*) FROM resource_grants WHERE org_id=$1`, f.org)
	result, err := f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: joiner, IdempotencyKey: "join-1"})
	if err != nil || !result.Changed || result.Role != "viewer" {
		t.Fatalf("join: %+v %v", result, err)
	}
	role, active, source := f.role(t, open, joiner)
	if role != "viewer" || !active || source != "self_join" {
		t.Fatalf("membership = %s active=%v source=%s", role, active, source)
	}
	if n := f.count(t, `SELECT count(*) FROM space_membership_events WHERE space_ref=$1 AND action='member_joined' AND subject_id=$2`, open, joiner); n != 1 {
		t.Fatalf("member_joined rows = %d, want 1", n)
	}
	if grantsAfter := f.count(t, `SELECT count(*) FROM resource_grants WHERE org_id=$1`, f.org); grantsAfter != grantsBefore {
		t.Fatalf("a join must write zero resource_grants rows, %d -> %d", grantsBefore, grantsAfter)
	}
	if n := f.count(t, `SELECT count(*) FROM space_effect_policies WHERE org_id=$1`, f.org); n != 0 {
		t.Fatalf("a join must never write an effect policy, found %d", n)
	}

	// Second join: already a member, nothing changes, role kept.
	again, err := f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: joiner})
	if err != nil || again.Changed || again.Replayed || again.Role != "viewer" {
		t.Fatalf("second join: %+v %v", again, err)
	}
	if n := f.ledgerCount(t, open); n != 1 {
		t.Fatalf("ledger rows after a second join = %d, want 1", n)
	}

	// Replay of the original key answers the stored outcome.
	replay, err := f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: joiner, IdempotencyKey: "join-1"})
	if err != nil || !replay.Replayed || replay.Changed {
		t.Fatalf("replay: %+v %v", replay, err)
	}
	// The same key from another person is a reuse, not a replay.
	other := f.user(t, "jn-other", f.org)
	_, err = f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: other, IdempotencyKey: "join-1"})
	wantRoomErr(t, err, 409, CodeIdempotencyKeyReused)

	// A member a manager removed earlier may rejoin with the room's join role.
	f.exec(t, `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, open, joiner)
	f.setSpace(t, open, `join_role='editor'`)
	rejoin, err := f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: joiner})
	if err != nil || !rejoin.Changed || rejoin.Role != "editor" {
		t.Fatalf("rejoin: %+v %v", rejoin, err)
	}

	// An archived open room says so.
	f.setSpace(t, open, `registration_state='archived'`)
	_, err = f.repo.JoinSpace(ctx, JoinRequest{SpaceRef: open, OrgID: f.org, ActorID: other})
	wantRoomErr(t, err, 409, CodeRoomArchived)
}

func TestUpdateRoomSettingsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "st")
	str := func(v string) *string { return &v }
	settings := func(actor string, visibility, joinRole *string, key string) (*SettingsResult, error) {
		return f.repo.UpdateRoomSettings(ctx, SettingsRequest{
			SpaceRef: s.ref, OrgID: f.org, ActorID: actor, Visibility: visibility, JoinRole: joinRole, IdempotencyKey: key,
		})
	}

	// Only a manager or owner may change settings.
	for _, actor := range []string{s.viewer, s.editor} {
		_, err := settings(actor, str("private"), nil, "")
		wantRoomErr(t, err, 403, CodeRoomRoleForbidden)
	}

	// org_public needs the directory policy.
	_, err := settings(s.manager, str("org_public"), nil, "")
	wantRoomErr(t, err, 403, CodeDirectoryDisabled)
	f.policy(t, f.org, true)

	beforeAuthority := f.authority(t, s.ref)
	var beforePrivacy int64
	if err := f.pool.QueryRow(ctx, `SELECT privacy_revision FROM space_authority_revisions WHERE space_ref=$1`, s.ref).Scan(&beforePrivacy); err != nil {
		t.Fatal(err)
	}
	result, err := settings(s.manager, str("org_public"), nil, "vis-1")
	if err != nil || !result.Changed || result.Visibility != "org_public" {
		t.Fatalf("make public: %+v %v", result, err)
	}
	if result.AuthorityRevision != beforeAuthority+1 || result.PrivacyRevision != beforePrivacy+1 {
		t.Fatalf("a visibility change advances authority and privacy once: %+v (was %d/%d)", result, beforeAuthority, beforePrivacy)
	}
	if n := f.count(t, `SELECT count(*) FROM space_membership_events WHERE space_ref=$1 AND action='visibility_changed'`, s.ref); n != 1 {
		t.Fatalf("visibility_changed rows = %d, want 1", n)
	}

	// join_role only: authority advances, privacy does not.
	jr, err := settings(s.owner, nil, str("viewer"), "")
	if err != nil || !jr.Changed || jr.JoinRole != "viewer" {
		t.Fatalf("join role: %+v %v", jr, err)
	}
	if jr.AuthorityRevision != result.AuthorityRevision+1 || jr.PrivacyRevision != result.PrivacyRevision {
		t.Fatalf("a join-role-only change advances authority only: %+v after %+v", jr, result)
	}

	// An unchanged request writes no ledger row and no revision.
	ledger := f.ledgerCount(t, s.ref)
	same, err := settings(s.manager, str("org_public"), str("viewer"), "")
	if err != nil || same.Changed || same.AuthorityRevision != jr.AuthorityRevision {
		t.Fatalf("unchanged: %+v %v", same, err)
	}
	if f.ledgerCount(t, s.ref) != ledger {
		t.Fatal("an unchanged request must not write a ledger row")
	}

	// A replay of the first key reports the stored outcome without a new change.
	replay, err := settings(s.manager, str("org_public"), nil, "vis-1")
	if err != nil || !replay.Replayed || replay.Changed {
		t.Fatalf("replay: %+v %v", replay, err)
	}

	// Back to private never needs the policy.
	f.policy(t, f.org, false)
	back, err := settings(s.manager, str("private"), nil, "")
	if err != nil || !back.Changed || back.Visibility != "private" {
		t.Fatalf("private again: %+v %v", back, err)
	}

	// The organization room (derived roster) and archived rooms refuse.
	f.setSpace(t, s.ref, `roster_mode='org_derived'`)
	_, err = settings(s.manager, str("private"), str("editor"), "")
	wantRoomErr(t, err, 409, CodeRoomRosterDerived)
	f.setSpace(t, s.ref, `roster_mode='managed', registration_state='archived'`)
	_, err = settings(s.manager, str("private"), str("editor"), "")
	wantRoomErr(t, err, 409, CodeRoomArchived)
}

func TestDirectoryPolicyWritesOneEventPerRealChangeLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	f.exec(t, `DELETE FROM space_directory_policy_events WHERE org_id=$1`, f.org)
	f.exec(t, `DELETE FROM space_directory_policies WHERE org_id=$1`, f.org)
	t.Cleanup(func() {
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_directory_policy_events WHERE org_id=$1`, f.org)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_directory_policies WHERE org_id=$1`, f.org)
	})
	events := func() int {
		return f.count(t, `SELECT count(*) FROM space_directory_policy_events WHERE org_id=$1`, f.org)
	}
	if n := f.count(t, `SELECT count(*) FROM space_directory_policies WHERE org_id=$1`, f.org); n != 0 {
		t.Fatalf("an org starts with NO policy row, found %d", n)
	}

	set := func(enabled bool) bool {
		changed, err := f.repo.UpsertDirectoryPolicy(ctx, DirectoryPolicy{OrgID: f.org, OrgPublicRoomsEnabled: enabled}, "control-space-policy")
		if err != nil {
			t.Fatalf("UpsertDirectoryPolicy(%v): %v", enabled, err)
		}
		return changed
	}
	if !set(true) || events() != 1 {
		t.Fatalf("first write: events=%d", events())
	}
	if set(true) || events() != 1 {
		t.Fatalf("an identical replay must write nothing: events=%d", events())
	}
	if !set(false) || events() != 2 {
		t.Fatalf("a flip writes one event: events=%d", events())
	}
	// The very first write may also be "off": it still inserts the row once.
	f.exec(t, `DELETE FROM space_directory_policy_events WHERE org_id=$1`, f.org)
	f.exec(t, `DELETE FROM space_directory_policies WHERE org_id=$1`, f.org)
	if !set(false) || events() != 1 {
		t.Fatalf("first write of false: events=%d", events())
	}
	var actor string
	if err := f.pool.QueryRow(ctx, `SELECT actor_principal_id FROM space_directory_policy_events WHERE org_id=$1 ORDER BY event_id DESC LIMIT 1`, f.org).Scan(&actor); err != nil || actor != "control-space-policy" {
		t.Fatalf("event actor = %q (%v)", actor, err)
	}
}
