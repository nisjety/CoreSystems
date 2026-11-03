package spaces

import (
	"context"
	"testing"
	"time"
)

// WS04-S11 live proof of the history floor and of the history_mode room
// setting. Gated on TEST_DATABASE_URL through roomLiveFixture.

// publishAudience writes one recipient-audience snapshot and makes it current.
func (f *roomLiveFixture) publishAudience(t *testing.T, ref string, revision int64, subjects ...string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, $2, 'audience:'||$1||':'||$2, 'sha256:ws04s11-'||$2, $3)`, ref, revision, len(subjects))
	for _, subject := range subjects {
		f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, $2, $3)`, ref, revision, subject)
	}
	f.exec(t, `UPDATE space_authority_revisions SET recipient_audience_revision=$2 WHERE space_ref=$1`, ref, revision)
}

func (f *roomLiveFixture) historyPolicy(t *testing.T) {
	t.Helper()
	f.cleanupPolicy(t)
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')
		ON CONFLICT (org_id) DO NOTHING`, f.org)
	f.exec(t, `UPDATE space_effect_policies SET thread_read_entitled=TRUE WHERE org_id=$1`, f.org)
}

func (f *roomLiveFixture) floorFor(t *testing.T, ref, subject string) int64 {
	t.Helper()
	evidence, err := f.repo.ResolveSharedThreadReadDecisionEvidence(context.Background(), ref, f.org, subject)
	if err != nil {
		t.Fatalf("resolve thread read evidence for %s: %v", subject, err)
	}
	return evidence.HistoryFloorRevision
}

func TestHistoryFloorFollowsTheAudienceSnapshotsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.historyPolicy(t)
	s := f.staffedRoom(t, "hist")
	f.repo.SetHistoryWindowClaim(true)
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)

	// rev 1: everyone but the editor.
	f.publishAudience(t, s.ref, 1, s.owner, s.manager, s.viewer)
	if got := f.floorFor(t, s.ref, s.viewer); got != 0 {
		t.Fatalf("a member present in every snapshot has no floor, got %d", got)
	}
	// rev 2: the editor joins -> floor is the revision they appear in.
	f.publishAudience(t, s.ref, 2, s.owner, s.manager, s.viewer, s.editor)
	if got := f.floorFor(t, s.ref, s.editor); got != 2 {
		t.Fatalf("editor added at audience revision 2 must get floor 2, got %d", got)
	}
	for _, present := range []string{s.owner, s.manager, s.viewer} {
		if got := f.floorFor(t, s.ref, present); got != 0 {
			t.Fatalf("%s was in every snapshot: no floor expected, got %d", present, got)
		}
	}
	// rev 3: the viewer leaves. rev 4: the viewer is re-added -> the NEW floor.
	f.publishAudience(t, s.ref, 3, s.owner, s.manager, s.editor)
	f.publishAudience(t, s.ref, 4, s.owner, s.manager, s.editor, s.viewer)
	if got := f.floorFor(t, s.ref, s.viewer); got != 4 {
		t.Fatalf("a removed then re-added member gets the new floor 4, got %d", got)
	}
	if got := f.floorFor(t, s.ref, s.editor); got != 2 {
		t.Fatalf("an unaffected member keeps floor 2, got %d", got)
	}
	if got := f.floorFor(t, s.ref, s.owner); got != 0 {
		t.Fatalf("the owner never left, got floor %d", got)
	}
}

func TestHistoryFloorIsAbsentWhenFlagOffOrModeFullLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.historyPolicy(t)
	s := f.staffedRoom(t, "histoff")
	f.publishAudience(t, s.ref, 1, s.owner, s.manager, s.viewer)
	f.publishAudience(t, s.ref, 2, s.owner, s.manager, s.viewer, s.editor)

	// since_join but the switch is off.
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)
	f.repo.SetHistoryWindowClaim(false)
	if got := f.floorFor(t, s.ref, s.editor); got != 0 {
		t.Fatalf("flag off must emit no floor, got %d", got)
	}
	// switch on but the room keeps full history.
	f.repo.SetHistoryWindowClaim(true)
	f.exec(t, `UPDATE registered_spaces SET history_mode='full' WHERE space_ref=$1`, s.ref)
	if got := f.floorFor(t, s.ref, s.editor); got != 0 {
		t.Fatalf("history_mode full must emit no floor, got %d", got)
	}
	// and the full decision JSON omits the claim.
	evidence, err := f.repo.ResolveSharedThreadReadDecisionEvidence(context.Background(), s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("resolve: %v", err)
	}
	decision, err := IssueSharedThreadReadDecision(evidence, readRequest(), time.Now().UTC())
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if decision.HistoryFloorRevision != 0 {
		t.Fatalf("decision floor = %d", decision.HistoryFloorRevision)
	}
}

func (f *roomLiveFixture) historyMode(t *testing.T, ref string) string {
	t.Helper()
	var mode string
	if err := f.pool.QueryRow(context.Background(), `SELECT history_mode FROM registered_spaces WHERE space_ref=$1`, ref).Scan(&mode); err != nil {
		t.Fatalf("read history_mode: %v", err)
	}
	return mode
}

func (f *roomLiveFixture) settingsLedger(t *testing.T, ref string) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM space_membership_events WHERE space_ref=$1 AND action='settings_changed'`, ref).Scan(&n); err != nil {
		t.Fatalf("count settings ledger: %v", err)
	}
	return n
}

func (f *roomLiveFixture) historyReq(ref, actor, mode string) SettingsRequest {
	return SettingsRequest{SpaceRef: ref, OrgID: f.org, ActorID: actor, HistoryMode: &mode}
}

func TestUpdateRoomSettingsHistoryModeRulesLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "histset")

	// A manager may manage the room but not the history window.
	_, err := f.repo.UpdateRoomSettings(ctx, f.historyReq(s.ref, s.manager, "since_join"))
	wantRoomErr(t, err, 403, CodeRoomRoleForbidden)
	_, err = f.repo.UpdateRoomSettings(ctx, f.historyReq(s.ref, s.editor, "since_join"))
	wantRoomErr(t, err, 403, CodeRoomRoleForbidden)
	if f.historyMode(t, s.ref) != "full" {
		t.Fatal("a refused request must write nothing")
	}

	// An invalid value is a 400 before anything is read.
	_, err = f.repo.UpdateRoomSettings(ctx, f.historyReq(s.ref, s.owner, "forever"))
	wantRoomErr(t, err, 400, CodeInvalidRequest)

	// The owner changes it: one authority advance and one settings_changed row.
	beforeAuthority, beforeLedger := f.authority(t, s.ref), f.settingsLedger(t, s.ref)
	res, err := f.repo.UpdateRoomSettings(ctx, f.historyReq(s.ref, s.owner, "since_join"))
	if err != nil || !res.Changed || res.HistoryMode != "since_join" {
		t.Fatalf("owner change: %+v %v", res, err)
	}
	if res.AuthorityRevision != beforeAuthority+1 || f.authority(t, s.ref) != beforeAuthority+1 {
		t.Fatalf("authority revision must advance once: %d -> %d", beforeAuthority, res.AuthorityRevision)
	}
	if got := f.settingsLedger(t, s.ref); got != beforeLedger+1 {
		t.Fatalf("settings_changed rows = %d, want %d", got, beforeLedger+1)
	}
	if f.historyMode(t, s.ref) != "since_join" {
		t.Fatal("history_mode was not stored")
	}

	// Set-to-state: the same request again writes nothing.
	res, err = f.repo.UpdateRoomSettings(ctx, f.historyReq(s.ref, s.owner, "since_join"))
	if err != nil || res.Changed {
		t.Fatalf("unchanged request: %+v %v", res, err)
	}
	if f.authority(t, s.ref) != beforeAuthority+1 || f.settingsLedger(t, s.ref) != beforeLedger+1 {
		t.Fatal("an unchanged request must not advance the revision or write a ledger row")
	}

	// GET membership carries the mode.
	membership, err := f.repo.ResolveReadableUserMembership(ctx, s.ref, f.org, s.viewer)
	if err != nil || membership.HistoryMode != "since_join" {
		t.Fatalf("membership history_mode = %q (%v)", membership.HistoryMode, err)
	}
}

func TestUpdateRoomSettingsHistoryModeRefusesOrgAndArchivedRoomsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	org := f.staffedRoom(t, "historg")
	f.exec(t, `UPDATE registered_spaces SET roster_mode='org_derived' WHERE space_ref=$1`, org.ref)
	_, err := f.repo.UpdateRoomSettings(ctx, f.historyReq(org.ref, org.owner, "since_join"))
	wantRoomErr(t, err, 409, CodeRoomRosterDerived)

	arch := f.staffedRoom(t, "histarch")
	f.exec(t, `UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, arch.ref)
	_, err = f.repo.UpdateRoomSettings(ctx, f.historyReq(arch.ref, arch.owner, "since_join"))
	wantRoomErr(t, err, 409, CodeRoomArchived)
	if f.historyMode(t, arch.ref) != "full" || f.historyMode(t, org.ref) != "full" {
		t.Fatal("refused requests must write nothing")
	}
}
