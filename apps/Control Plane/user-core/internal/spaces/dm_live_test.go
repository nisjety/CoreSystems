package spaces

import (
	"context"
	"errors"
	"testing"
)

// WS12-S1 live proof of direct-message registration and its fixed roster.
// Gated on TEST_DATABASE_URL through newRoomLiveFixture (skipped without it).

type dmLive struct {
	*roomLiveFixture
}

func newDMLive(t *testing.T, entitled bool) *dmLive {
	t.Helper()
	f := newRoomLiveFixture(t)
	f.repo.SetDMRegistrationEnabled(true)
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope)
		VALUES ($1, 'privacy:'||$1||':1', 'assistant_collaboration', 'contract', 'internal', 'standard', 'swedencentral', 'space')`, f.org)
	t.Cleanup(func() {
		ctx := context.Background()
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id=$1`, f.org)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_deletion_policies WHERE org_id=$1`, f.org)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_deletion_requests WHERE org_id=$1`, f.org)
	})
	if entitled {
		f.exec(t, `UPDATE space_effect_policies SET dm_create_entitled=TRUE, thread_create_entitled=TRUE, thread_read_entitled=TRUE WHERE org_id=$1`, f.org)
	}
	return &dmLive{f}
}

func (d *dmLive) register(t *testing.T, name string, kind Kind, owner string, participants ...string) (*RegisteredSpace, error) {
	t.Helper()
	hash, err := ParticipantSetHash(participants...)
	if err != nil {
		t.Fatalf("hash: %v", err)
	}
	return d.repo.Register(context.Background(), Registration{
		SpaceRef: "ws04s4-" + name + "-" + d.suffix, OrgID: d.org, OwnerPrincipalID: owner, Kind: kind,
		Lifecycle: LifecyclePendingRegistration, LifecycleRevision: 1, Participants: participants, ParticipantHash: hash,
	})
}

func TestRegisterDMSeedsAllParticipants(t *testing.T) {
	d := newDMLive(t, true)
	a, b, c := d.user(t, "dmseed-a", d.org), d.user(t, "dmseed-b", d.org), d.user(t, "dmseed-c", d.org)
	space, err := d.register(t, "dmseed", KindGroupDM, a, a, b, c)
	if err != nil {
		t.Fatalf("register: %v", err)
	}
	if space.Kind != KindGroupDM || space.ParticipantHash == "" {
		t.Fatalf("registered space = %+v", space)
	}
	ref := space.SpaceRef
	if role, active, source := d.role(t, ref, a); role != "owner" || !active || source != SourceRegistration {
		t.Fatalf("creator = %s %v %s", role, active, source)
	}
	for _, person := range []string{b, c} {
		if role, active, source := d.role(t, ref, person); role != "editor" || !active || source != SourceRegistration {
			t.Fatalf("participant %s = %s %v %s, want an active editor from registration", person, role, active, source)
		}
	}
	var members int
	if err := d.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_memberships WHERE space_ref=$1`, ref).Scan(&members); err != nil || members != 3 {
		t.Fatalf("membership rows = %d err=%v, want exactly the three participants", members, err)
	}
	// A replay of the same registration is harmless.
	if _, err := d.register(t, "dmseed", KindGroupDM, a, a, b, c); err != nil {
		t.Fatalf("replay: %v", err)
	}
}

func TestRegisterDMRefusedWithoutEntitlements(t *testing.T) {
	d := newDMLive(t, false)
	a, b := d.user(t, "dment-a", d.org), d.user(t, "dment-b", d.org)

	var notEntitled *DMNotEntitledError
	_, err := d.register(t, "dment1", KindDM, a, a, b)
	if !errors.As(err, &notEntitled) || len(notEntitled.Missing) != 3 {
		t.Fatalf("no bits: err = %v, want all three missing", err)
	}
	d.exec(t, `UPDATE space_effect_policies SET dm_create_entitled=TRUE WHERE org_id=$1`, d.org)
	_, err = d.register(t, "dment2", KindDM, a, a, b)
	if !errors.As(err, &notEntitled) || len(notEntitled.Missing) != 2 || notEntitled.Missing[0] != "thread_create_entitled" {
		t.Fatalf("only dm_create: err = %v, want thread_create and thread_read missing", err)
	}
	// ZDR makes a DM unusable even with every bit on.
	d.exec(t, `UPDATE space_effect_policies SET thread_create_entitled=TRUE, thread_read_entitled=TRUE, zero_data_retention=TRUE WHERE org_id=$1`, d.org)
	_, err = d.register(t, "dment3", KindDM, a, a, b)
	if !errors.As(err, &notEntitled) || notEntitled.Missing[len(notEntitled.Missing)-1] != "zero_data_retention" {
		t.Fatalf("zdr: err = %v, want zero_data_retention reported", err)
	}
	d.exec(t, `UPDATE space_effect_policies SET zero_data_retention=FALSE WHERE org_id=$1`, d.org)
	// The rollout switch off: no new DM, whatever the entitlements.
	d.repo.SetDMRegistrationEnabled(false)
	if _, err := d.register(t, "dment4", KindDM, a, a, b); !errors.Is(err, ErrDMDisabled) {
		t.Fatalf("switch off: err = %v, want ErrDMDisabled", err)
	}
	d.repo.SetDMRegistrationEnabled(true)
	if _, err := d.register(t, "dment5", KindDM, a, a, b); err != nil {
		t.Fatalf("all on: %v", err)
	}
	// Nothing was written by the refused attempts.
	var n int
	if err := d.pool.QueryRow(context.Background(), `SELECT count(*) FROM registered_spaces WHERE org_id=$1`, d.org).Scan(&n); err != nil || n != 1 {
		t.Fatalf("registered spaces = %d err=%v, want only the successful one", n, err)
	}
}

func TestRegisterDMRefusesInactiveOrForeignParticipant(t *testing.T) {
	d := newDMLive(t, true)
	a := d.user(t, "dmpeople-a", d.org)
	foreign := d.user(t, "dmpeople-f", d.other)
	reasonOf := func(err error) string {
		var invalid *DMParticipantInvalidError
		if errors.As(err, &invalid) {
			return invalid.Reason
		}
		return ""
	}
	if _, err := d.register(t, "dmpeople1", KindDM, a, a, foreign); reasonOf(err) != DMReasonNotInOrg {
		t.Fatalf("foreign participant: err = %v, want not_in_org", err)
	}
	if _, err := d.register(t, "dmpeople2", KindDM, a, a, "ws04s4-nobody-"+d.suffix); reasonOf(err) != DMReasonNotInOrg {
		t.Fatalf("unknown participant: err = %v, want not_in_org", err)
	}
	suspended := d.user(t, "dmpeople-s", d.org)
	d.exec(t, `UPDATE user_org_memberships SET status='suspended' WHERE user_id=$1 AND org_id=$2`, suspended, d.org)
	if _, err := d.register(t, "dmpeople3", KindDM, a, a, suspended); reasonOf(err) != DMReasonInactive {
		t.Fatalf("suspended participant: err = %v, want inactive", err)
	}
}

func TestRegisterDMUniqueIndexReturnsErrDMExists(t *testing.T) {
	d := newDMLive(t, true)
	a, b := d.user(t, "dmuniq-a", d.org), d.user(t, "dmuniq-b", d.org)
	first, err := d.register(t, "dmuniq1", KindDM, a, a, b)
	if err != nil {
		t.Fatalf("first: %v", err)
	}
	// The same pair under another Space ref, opened by the other person.
	if _, err := d.register(t, "dmuniq2", KindDM, b, b, a); !errors.Is(err, ErrDMExists) {
		t.Fatalf("second: err = %v, want ErrDMExists", err)
	}
	// A deleted DM frees the slot.
	d.exec(t, `UPDATE registered_spaces SET registration_state='deleted' WHERE space_ref=$1`, first.SpaceRef)
	if _, err := d.register(t, "dmuniq3", KindDM, a, a, b); err != nil {
		t.Fatalf("after deletion: %v", err)
	}
}

func TestReplaceMembershipsRefusesDM(t *testing.T) {
	d := newDMLive(t, true)
	a, b, extra := d.user(t, "dmfixed-a", d.org), d.user(t, "dmfixed-b", d.org), d.user(t, "dmfixed-x", d.org)
	space, err := d.register(t, "dmfixed", KindDM, a, a, b)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if _, _, _, _, err := d.repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: space.SpaceRef,
		Members:  []MemberGrant{{SubjectType: "user", SubjectID: extra, Role: "editor"}},
	}); !errors.Is(err, ErrFixedRoster) {
		t.Fatalf("replace: err = %v, want ErrFixedRoster", err)
	}
	// The member operations refuse it with the typed room error.
	_, err = d.repo.AddMember(ctx, d.op(space.SpaceRef, a, extra, "viewer"))
	wantRoomErr(t, err, 409, CodeRoomFixedRoster)
	_, err = d.repo.RemoveMember(ctx, d.op(space.SpaceRef, a, b, ""))
	wantRoomErr(t, err, 409, CodeRoomFixedRoster)
	if _, active, _ := d.role(t, space.SpaceRef, b); !active {
		t.Fatal("a DM participant was deactivated")
	}
	if got := d.ledgerCount(t, space.SpaceRef); got != 0 {
		t.Fatalf("a refused operation wrote %d ledger rows", got)
	}
}

func TestAuthorizeDeletionRejectsDM(t *testing.T) {
	d := newDMLive(t, true)
	a, b := d.user(t, "dmdel-a", d.org), d.user(t, "dmdel-b", d.org)
	space, err := d.register(t, "dmdel", KindDM, a, a, b)
	if err != nil {
		t.Fatal(err)
	}
	// Even an organization with deletion fully on cannot delete a DM: the
	// policy covers personal Spaces only.
	d.exec(t, `INSERT INTO space_deletion_policies (org_id, deletion_entitled, personal_rollout_enabled) VALUES ($1, TRUE, TRUE)
		ON CONFLICT (org_id) DO UPDATE SET deletion_entitled=TRUE, personal_rollout_enabled=TRUE`, d.org)
	_, err = d.repo.AuthorizeDeletion(context.Background(), DeletionAuthorizationRequest{
		SpaceRef: space.SpaceRef, OrgID: d.org, OwnerPrincipalID: a, RequestID: "ws04s4-delreq-" + d.suffix,
		IdempotencyKey: "ws04s4-delkey-" + d.suffix,
	})
	if err == nil {
		t.Fatal("a direct message deletion was authorized")
	}
}

func TestSpacesForSubjectReturnsParticipantsOnlyForDM(t *testing.T) {
	d := newDMLive(t, true)
	a, b := d.user(t, "dmlist-a", d.org), d.user(t, "dmlist-b", d.org)
	room := d.room(t, "dmlist-room", a)
	dm, err := d.register(t, "dmlist-dm", KindDM, a, a, b)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := d.repo.SpacesForSubject(context.Background(), d.org, a)
	if err != nil {
		t.Fatalf("SpacesForSubject: %v", err)
	}
	seen := map[string]SpaceIndexEntry{}
	for _, entry := range entries {
		seen[entry.SpaceRef] = entry
	}
	roomEntry, dmEntry := seen[room], seen[dm.SpaceRef]
	if roomEntry.SpaceRef == "" || dmEntry.SpaceRef == "" {
		t.Fatalf("index misses a Space: %+v", entries)
	}
	if len(roomEntry.Participants) != 0 || roomEntry.AgentScope != AgentScopeRoom {
		t.Fatalf("room entry = %+v, want no participants and room agent scope", roomEntry)
	}
	if dmEntry.Kind != KindDM || dmEntry.AgentScope != AgentScopeNone || len(dmEntry.Participants) != 2 ||
		dmEntry.Participants[0].SubjectID != a || dmEntry.Participants[1].SubjectID != b {
		t.Fatalf("dm entry = %+v", dmEntry)
	}
}

func TestDMEligibilityLive(t *testing.T) {
	d := newDMLive(t, true)
	a, b, c := d.user(t, "dmelig-a", d.org), d.user(t, "dmelig-b", d.org), d.user(t, "dmelig-c", d.org)
	ctx := context.Background()
	got, err := d.repo.DMEligibility(ctx, d.org, a, []string{b})
	if err != nil || !got.Eligible || got.Kind != KindDM || len(got.ParticipantIDs) != 2 {
		t.Fatalf("pair: %+v err=%v", got, err)
	}
	want, _ := ParticipantSetHash(a, b)
	if got.ParticipantHash != want {
		t.Fatalf("hash = %s, want %s", got.ParticipantHash, want)
	}
	if group, err := d.repo.DMEligibility(ctx, d.org, a, []string{b, c}); err != nil || group.Kind != KindGroupDM {
		t.Fatalf("group: %+v err=%v", group, err)
	}
	reasonOf := func(err error) string {
		var invalid *DMParticipantInvalidError
		if errors.As(err, &invalid) {
			return invalid.Reason
		}
		return ""
	}
	if _, err := d.repo.DMEligibility(ctx, d.org, a, []string{a}); reasonOf(err) != DMReasonSelf {
		t.Fatalf("self: err = %v", err)
	}
	if _, err := d.repo.DMEligibility(ctx, d.org, a, nil); reasonOf(err) != DMReasonCount {
		t.Fatalf("alone: err = %v", err)
	}
	if _, err := d.repo.DMEligibility(ctx, d.org, a, []string{b, b}); reasonOf(err) != DMReasonDuplicate {
		t.Fatalf("duplicate: err = %v", err)
	}
	if _, err := d.repo.DMEligibility(ctx, d.org, a, []string{"ws04s4-ghost-" + d.suffix}); reasonOf(err) != DMReasonNotInOrg {
		t.Fatalf("ghost: err = %v", err)
	}
	d.repo.SetDMRegistrationEnabled(false)
	if _, err := d.repo.DMEligibility(ctx, d.org, a, []string{b}); !errors.Is(err, ErrDMDisabled) {
		t.Fatalf("switch off: err = %v", err)
	}
}
