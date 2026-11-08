package spaces

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

// WS04-S7 live proof of room archive and restore. Gated on TEST_DATABASE_URL;
// it reuses roomLiveFixture from membership_ops_live_test.go.

// archiveFixtureRoom is a staffed room with a published recipient audience of
// all four members and an effect policy that holds every entitlement, so each
// resolver would succeed for an active room.
func (f *roomLiveFixture) archiveFixtureRoom(t *testing.T, name string) roomStaff {
	t.Helper()
	s := f.staffedRoom(t, name)
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')
		ON CONFLICT (org_id) DO NOTHING`, f.org)
	f.exec(t, `UPDATE space_effect_policies SET thread_create_entitled=TRUE, retrieval_read_entitled=TRUE,
		thread_read_entitled=TRUE, agent_action_entitled=TRUE, schedule_fire_entitled=TRUE,
		import_write_entitled=TRUE, sandbox_capability_entitled=TRUE WHERE org_id=$1`, f.org)
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:'||$1||':1', 'sha256:ws04s7', 4)`, s.ref)
	f.exec(t, `UPDATE space_authority_revisions SET recipient_audience_revision = 1 WHERE space_ref = $1`, s.ref)
	for _, subject := range []string{s.owner, s.manager, s.editor, s.viewer} {
		f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, s.ref, subject)
	}
	return s
}

func (f *roomLiveFixture) cleanupPolicy(t *testing.T) {
	t.Helper()
	t.Cleanup(func() {
		_, _ = f.pool.Exec(context.Background(), `DELETE FROM space_recipient_audience_members WHERE space_ref LIKE 'ws04s4-%-'||$1`, f.suffix)
		_, _ = f.pool.Exec(context.Background(), `DELETE FROM space_recipient_audiences WHERE space_ref LIKE 'ws04s4-%-'||$1`, f.suffix)
		_, _ = f.pool.Exec(context.Background(), `DELETE FROM space_effect_policies WHERE org_id=$1`, f.org)
	})
}

func (f *roomLiveFixture) archiveReq(ref, actor string) ArchiveRequest {
	return ArchiveRequest{SpaceRef: ref, OrgID: f.org, ActorID: actor}
}

func TestArchivedRoomIsReadOnlyAndUnarchiveRestoresWritesLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "archive")

	// Baseline: the active room resolves for every write-class and read-class surface.
	if _, err := f.repo.ResolveSharedThreadDecisionEvidence(ctx, s.ref, f.org, s.editor); err != nil {
		t.Fatalf("active room write evidence: %v", err)
	}

	beforeAuthority := f.authority(t, s.ref)
	res, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner))
	if err != nil || !res.Changed || res.State != RegistrationStateArchived || res.ArchivedAt == nil {
		t.Fatalf("archive: %+v %v", res, err)
	}
	if res.AuthorityRevision != beforeAuthority+1 || f.authority(t, s.ref) != beforeAuthority+1 {
		t.Fatalf("authority revision must advance exactly once: %d -> %d", beforeAuthority, res.AuthorityRevision)
	}

	// Write-class: thread create and append, run action and the strict
	// membership used by every write decision all refuse an archived room.
	if _, err := f.repo.ResolveCurrentUserMembership(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("strict membership of an archived room: %v", err)
	}
	if _, err := f.repo.ResolveSharedThreadDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("thread create/append evidence of an archived room: %v", err)
	}
	if _, err := f.repo.ResolveAgentActionDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("run-action evidence of an archived room: %v", err)
	}
	if archived, err := f.repo.ArchivedForMember(ctx, s.ref, f.org, s.editor); err != nil || !archived {
		t.Fatalf("ArchivedForMember for a member: %v %v", archived, err)
	}
	stranger := f.user(t, "archive-stranger", f.org)
	if archived, err := f.repo.ArchivedForMember(ctx, s.ref, f.org, stranger); err != nil || archived {
		t.Fatalf("ArchivedForMember must not reveal the room to a non-member: %v %v", archived, err)
	}

	// Read-class: roster, index, readable membership, thread read, retrieval read.
	roster, err := f.repo.RosterForSpace(ctx, s.ref, f.org, s.viewer)
	if err != nil || len(roster) != 4 {
		t.Fatalf("roster of an archived room: %d %v", len(roster), err)
	}
	index, err := f.repo.SpacesForSubject(ctx, f.org, s.viewer)
	if err != nil {
		t.Fatalf("index: %v", err)
	}
	found := false
	for _, entry := range index {
		if entry.SpaceRef == s.ref {
			found = true
			if entry.State != RegistrationStateArchived {
				t.Fatalf("index state = %q, want archived", entry.State)
			}
		}
	}
	if !found {
		t.Fatal("an archived room dropped out of the index")
	}
	membership, err := f.repo.ResolveReadableUserMembership(ctx, s.ref, f.org, s.viewer)
	if err != nil || membership.State != RegistrationStateArchived || membership.ArchivedAt == nil {
		t.Fatalf("readable membership: %+v %v", membership, err)
	}
	if _, err := f.repo.ResolveSharedThreadReadDecisionEvidence(ctx, s.ref, f.org, s.viewer); err != nil {
		t.Fatalf("thread read evidence of an archived room: %v", err)
	}
	if _, err := f.repo.ResolveSharedRetrievalDecisionEvidence(ctx, s.ref, f.org, s.viewer); err != nil {
		t.Fatalf("retrieval read evidence of an archived room: %v", err)
	}

	// Unarchive restores writes and clears the archive facts.
	res, err = f.repo.UnarchiveSpace(ctx, f.archiveReq(s.ref, s.manager))
	if err != nil || !res.Changed || res.State != "active" || res.ArchivedAt != nil {
		t.Fatalf("unarchive: %+v %v", res, err)
	}
	if res.AuthorityRevision != beforeAuthority+2 {
		t.Fatalf("authority revision after unarchive = %d, want %d", res.AuthorityRevision, beforeAuthority+2)
	}
	if _, err := f.repo.ResolveSharedThreadDecisionEvidence(ctx, s.ref, f.org, s.editor); err != nil {
		t.Fatalf("write evidence after unarchive: %v", err)
	}
	var archivedBy *string
	if err := f.pool.QueryRow(ctx, `SELECT archived_by FROM registered_spaces WHERE space_ref=$1`, s.ref).Scan(&archivedBy); err != nil || archivedBy != nil {
		t.Fatalf("archived_by must be cleared on unarchive: %v %v", archivedBy, err)
	}

	// Ledger: one archived and one unarchived row, space_seq = authority revision.
	rows, err := f.pool.Query(ctx, `SELECT action, actor_kind, actor_id, space_seq FROM space_membership_events
		WHERE space_ref=$1 AND action IN ('archived','unarchived') ORDER BY seq`, s.ref)
	if err != nil {
		t.Fatalf("read ledger: %v", err)
	}
	defer rows.Close()
	var seen []string
	seqs := []int64{beforeAuthority + 1, beforeAuthority + 2}
	for rows.Next() {
		var action, kind, actor string
		var seq int64
		if err := rows.Scan(&action, &kind, &actor, &seq); err != nil {
			t.Fatalf("scan ledger: %v", err)
		}
		if kind != ActorKindUser || seq != seqs[len(seen)] {
			t.Fatalf("ledger row %s kind=%s seq=%d", action, kind, seq)
		}
		seen = append(seen, action)
	}
	if !reflect.DeepEqual(seen, []string{"archived", "unarchived"}) {
		t.Fatalf("ledger actions = %v", seen)
	}
}

// Personal-Space evidence (import, schedule, sandbox capability) is resolved by
// the personal resolvers, which stay active-only. A personal Space cannot be
// archived through the writer, so the state is forced here to prove the
// predicate itself.
func TestArchivedStateIsNeverResolvedByPersonalWriteEvidenceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	owner := f.user(t, "personal-owner", f.org)
	ref := "ws04s4-personal-" + f.suffix
	f.exec(t, `INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		application_lifecycle_revision, registration_state) VALUES ($1,$2,'personal',$3,1,'active')`, ref, f.org, owner)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, ref)
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'user',$2,'owner','application-space-registration','registration')`, ref, owner)
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')
		ON CONFLICT (org_id) DO NOTHING`, f.org)
	f.exec(t, `UPDATE space_effect_policies SET thread_create_entitled=TRUE, retrieval_read_entitled=TRUE,
		thread_read_entitled=TRUE, agent_action_entitled=TRUE, schedule_fire_entitled=TRUE,
		import_write_entitled=TRUE, sandbox_capability_entitled=TRUE WHERE org_id=$1`, f.org)

	if _, err := f.repo.resolvePersonalDecisionEvidence(ctx, ref, f.org, owner); err != nil {
		t.Fatalf("active personal evidence: %v", err)
	}
	f.exec(t, `UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, ref)
	resolvers := map[string]func() error{
		"personal evidence (sandbox, schedule)": func() error {
			_, err := f.repo.resolvePersonalDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
		"personal thread": func() error {
			_, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
		"personal import": func() error {
			_, err := f.repo.ResolvePersonalImportDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
		"personal retrieval": func() error {
			_, err := f.repo.ResolvePersonalRetrievalDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
		"own scheduled thread read": func() error {
			_, err := f.repo.ResolveOwnScheduledThreadReadDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
		"run action": func() error {
			_, err := f.repo.ResolveAgentActionDecisionEvidence(ctx, ref, f.org, owner)
			return err
		},
	}
	for name, resolve := range resolvers {
		if err := resolve(); err == nil {
			t.Errorf("%s resolved an archived Space", name)
		}
	}
}

func TestArchiveRegisterReplayKeepsArchivedAndStrongerStatesWinLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()

	register := func(ref, owner string, lifecycle SpaceLifecycle, revision int64) *RegisteredSpace {
		t.Helper()
		got, err := f.repo.Register(ctx, Registration{
			SpaceRef: ref, OrgID: f.org, OwnerPrincipalID: owner, Kind: KindRoom,
			Lifecycle: lifecycle, LifecycleRevision: revision,
		})
		if err != nil {
			t.Fatalf("register %s %s: %v", ref, lifecycle, err)
		}
		return got
	}

	for _, tc := range []struct {
		name      string
		lifecycle SpaceLifecycle
		want      string
	}{
		{"active replay keeps archived", LifecycleActive, RegistrationStateArchived},
		{"pending replay keeps archived", LifecyclePendingRegistration, RegistrationStateArchived},
		{"suspended wins", LifecycleSuspended, "suspended"},
		{"deleting wins", LifecycleDeleting, "deleting"},
		{"deleted wins", LifecycleDeleted, "deleted"},
	} {
		s := f.staffedRoom(t, "replay-"+string(tc.lifecycle))
		if _, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner)); err != nil {
			t.Fatalf("%s: archive: %v", tc.name, err)
		}
		got := register(s.ref, s.owner, tc.lifecycle, 2)
		if got.RegistrationState != tc.want {
			t.Errorf("%s: state = %q, want %q", tc.name, got.RegistrationState, tc.want)
		}
	}
}

func TestArchiveRefusalsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "refuse")

	// viewer and editor may not archive.
	for _, actor := range []string{s.viewer, s.editor} {
		_, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, actor))
		wantRoomErr(t, err, 403, CodeRoomRoleForbidden)
	}
	// non-member and other-org callers get the unknown-Space answer, byte for byte.
	_, unknownErr := f.repo.ArchiveSpace(ctx, f.archiveReq("ws04s4-no-such-room-"+f.suffix, s.owner))
	want := wantRoomErr(t, unknownErr, 404, CodeRoomNotFound)
	outsider := f.user(t, "refuse-outsider", f.org)
	foreign := f.user(t, "refuse-foreign", f.other)
	for name, req := range map[string]ArchiveRequest{
		"non-member": f.archiveReq(s.ref, outsider),
		"other org":  {SpaceRef: s.ref, OrgID: f.other, ActorID: foreign},
		"org claim":  f.archiveReq(s.ref, foreign),
	} {
		_, err := f.repo.ArchiveSpace(ctx, req)
		got := wantRoomErr(t, err, 404, CodeRoomNotFound)
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s: %+v differs from the unknown-Space answer %+v", name, got, want)
		}
	}
	// the organization room cannot be archived.
	f.exec(t, `UPDATE registered_spaces SET roster_mode='org_derived' WHERE space_ref=$1`, s.ref)
	_, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner))
	wantRoomErr(t, err, 409, CodeRoomNotArchivable)
	f.exec(t, `UPDATE registered_spaces SET roster_mode='managed' WHERE space_ref=$1`, s.ref)
	// a suspended room is in a conflicting state.
	f.exec(t, `UPDATE registered_spaces SET registration_state='suspended' WHERE space_ref=$1`, s.ref)
	_, err = f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner))
	conflict := wantRoomErr(t, err, 409, CodeRoomStateConflict)
	if conflict.Details["state"] != "suspended" {
		t.Fatalf("details must name the state: %v", conflict.Details)
	}
	if got := f.ledgerCount(t, s.ref); got != 0 {
		t.Fatalf("a refused archive appended %d ledger rows", got)
	}
}

func TestArchiveIsSetToStateAndIdempotentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "idem-archive")

	// Unarchiving an active room is a no-op.
	res, err := f.repo.UnarchiveSpace(ctx, f.archiveReq(s.ref, s.owner))
	if err != nil || res.Changed || res.State != "active" {
		t.Fatalf("unarchive of an active room: %+v %v", res, err)
	}

	req := f.archiveReq(s.ref, s.owner)
	req.IdempotencyKey = "archive-key-1"
	first, err := f.repo.ArchiveSpace(ctx, req)
	if err != nil || !first.Changed || first.Replayed {
		t.Fatalf("first archive: %+v %v", first, err)
	}
	authority := f.authority(t, s.ref)
	rows := f.ledgerCount(t, s.ref)

	replay, err := f.repo.ArchiveSpace(ctx, req)
	if err != nil || replay.Changed || !replay.Replayed || replay.State != RegistrationStateArchived {
		t.Fatalf("replay: %+v %v", replay, err)
	}
	if f.authority(t, s.ref) != authority || f.ledgerCount(t, s.ref) != rows {
		t.Fatal("a replay advanced the authority revision or appended a ledger row")
	}

	// The same key for the opposite operation is a reuse.
	_, err = f.repo.UnarchiveSpace(ctx, req)
	wantRoomErr(t, err, 409, CodeIdempotencyKeyReused)
	// The same key from another actor is a reuse too.
	other := req
	other.ActorID = s.manager
	_, err = f.repo.ArchiveSpace(ctx, other)
	wantRoomErr(t, err, 409, CodeIdempotencyKeyReused)

	// Archiving twice (a fresh key) is set-to-state: changed=false, nothing advances.
	again, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner))
	if err != nil || again.Changed {
		t.Fatalf("second archive: %+v %v", again, err)
	}
	if f.authority(t, s.ref) != authority || f.ledgerCount(t, s.ref) != rows {
		t.Fatal("a no-op archive advanced the authority revision or appended a ledger row")
	}
}
