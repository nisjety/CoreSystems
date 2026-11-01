package spaces

import (
	"context"
	"errors"
	"fmt"
	"os"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS04-S4 live proof of the Control room membership writer. Gated on
// TEST_DATABASE_URL (a disposable Postgres; migrations are applied here).
// Every identifier carries a per-run suffix so the test cleans up exactly what
// it created.

type roomLiveFixture struct {
	pool   *pgxpool.Pool
	repo   *Repository
	org    string
	other  string
	suffix string
}

func newRoomLiveFixture(t *testing.T) *roomLiveFixture {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live room writer test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := (&database.DB{Pool: pool}).RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	f := &roomLiveFixture{
		pool: pool, repo: NewRepository(&database.DB{Pool: pool}),
		org: "ws04s4-org-" + suffix, other: "ws04s4-other-" + suffix, suffix: suffix,
	}
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_membership_events WHERE space_ref LIKE 'ws04s4-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref LIKE 'ws04s4-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref LIKE 'ws04s4-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref LIKE 'ws04s4-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM user_org_memberships WHERE org_id IN ($1,$2)`, f.org, f.other)
		_, _ = pool.Exec(ctx, `DELETE FROM users WHERE id LIKE 'ws04s4-%-'||$1`, suffix)
	})
	return f
}

func (f *roomLiveFixture) exec(t *testing.T, query string, args ...any) {
	t.Helper()
	if _, err := f.pool.Exec(context.Background(), query, args...); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
}

// user creates a user; orgID "" leaves them in no organization.
func (f *roomLiveFixture) user(t *testing.T, name, orgID string) string {
	t.Helper()
	id := "ws04s4-" + name + "-" + f.suffix
	f.exec(t, `INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws04s4.invalid', $1)`, id)
	if orgID != "" {
		f.exec(t, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member','active')`, id, orgID)
	}
	return id
}

// room registers a managed room in f.org owned by owner (source registration).
func (f *roomLiveFixture) room(t *testing.T, name, owner string) string {
	t.Helper()
	ref := "ws04s4-" + name + "-" + f.suffix
	f.exec(t, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room',$3,1,'active')`, ref, f.org, owner)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, ref)
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'user',$2,'owner','application-space-registration','registration')`, ref, owner)
	return ref
}

func (f *roomLiveFixture) member(t *testing.T, ref, subject, role, source string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'user',$2,$3,'ws04s4-test',$4)`, ref, subject, role, source)
}

func (f *roomLiveFixture) role(t *testing.T, ref, subject string) (role string, active bool, source string) {
	t.Helper()
	if err := f.pool.QueryRow(context.Background(),
		`SELECT role, active, source FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`, ref, subject,
	).Scan(&role, &active, &source); err != nil {
		t.Fatalf("read membership %s/%s: %v", ref, subject, err)
	}
	return
}

func (f *roomLiveFixture) ledgerCount(t *testing.T, ref string) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_membership_events WHERE space_ref=$1`, ref).Scan(&n); err != nil {
		t.Fatalf("count ledger: %v", err)
	}
	return n
}

func (f *roomLiveFixture) authority(t *testing.T, ref string) int64 {
	t.Helper()
	var n int64
	if err := f.pool.QueryRow(context.Background(), `SELECT authority_revision FROM space_authority_revisions WHERE space_ref=$1`, ref).Scan(&n); err != nil {
		t.Fatalf("read authority revision: %v", err)
	}
	return n
}

func wantRoomErr(t *testing.T, err error, status int, code string) *RoomError {
	t.Helper()
	var roomErr *RoomError
	if !errors.As(err, &roomErr) {
		t.Fatalf("want room error %d %s, got %v", status, code, err)
	}
	if roomErr.Status != status || roomErr.Code != code {
		t.Fatalf("want room error %d %s, got %d %s (%s)", status, code, roomErr.Status, roomErr.Code, roomErr.Message)
	}
	return roomErr
}

// roomStaff is a room with an owner, a manager, an editor and a viewer.
type roomStaff struct {
	ref                            string
	owner, manager, editor, viewer string
}

func (f *roomLiveFixture) staffedRoom(t *testing.T, name string) roomStaff {
	t.Helper()
	s := roomStaff{
		owner: f.user(t, name+"-owner", f.org), manager: f.user(t, name+"-manager", f.org),
		editor: f.user(t, name+"-editor", f.org), viewer: f.user(t, name+"-viewer", f.org),
	}
	s.ref = f.room(t, name, s.owner)
	f.member(t, s.ref, s.manager, "manager", SourceMemberOps)
	f.member(t, s.ref, s.editor, "editor", SourceMemberOps)
	f.member(t, s.ref, s.viewer, "viewer", SourceMemberOps)
	return s
}

func (f *roomLiveFixture) op(ref, actor, subject, role string) MemberOpRequest {
	return MemberOpRequest{SpaceRef: ref, OrgID: f.org, ActorID: actor, SubjectID: subject, Role: role}
}

func TestMemberOpsRoleMatrixLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "matrix")
	target := f.user(t, "matrix-target", f.org)

	// viewer and editor cannot add.
	for _, actor := range []string{s.viewer, s.editor} {
		_, err := f.repo.AddMember(ctx, f.op(s.ref, actor, target, "viewer"))
		wantRoomErr(t, err, 403, CodeRoomRoleForbidden)
	}
	// a manager cannot grant manager or owner.
	for _, role := range []string{"manager", "owner"} {
		_, err := f.repo.AddMember(ctx, f.op(s.ref, s.manager, target, role))
		wantRoomErr(t, err, 403, CodeRoomElevatedNeedsOwner)
	}
	// a manager cannot touch an owner: change role or remove.
	_, err := f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.manager, s.owner, "viewer"))
	wantRoomErr(t, err, 403, CodeRoomElevatedNeedsOwner)
	_, err = f.repo.RemoveMember(ctx, f.op(s.ref, s.manager, s.owner, ""))
	wantRoomErr(t, err, 403, CodeRoomElevatedNeedsOwner)
	// and cannot promote anyone to manager.
	_, err = f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.manager, s.editor, "manager"))
	wantRoomErr(t, err, 403, CodeRoomElevatedNeedsOwner)

	if got := f.ledgerCount(t, s.ref); got != 0 {
		t.Fatalf("a refused operation appended %d ledger rows", got)
	}
	if role, _, _ := f.role(t, s.ref, s.owner); role != "owner" {
		t.Fatalf("the owner's role changed to %q", role)
	}

	// A manager may add a viewer; the owner may add a manager.
	if res, err := f.repo.AddMember(ctx, f.op(s.ref, s.manager, target, "viewer")); err != nil || !res.Changed || res.Role != "viewer" {
		t.Fatalf("manager add viewer: %+v %v", res, err)
	}
	// Self demotion is allowed, self promotion is refused.
	if _, err := f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.manager, s.manager, "editor")); err != nil {
		t.Fatalf("self demotion: %v", err)
	}
	_, err = f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.manager, s.manager, "manager"))
	wantRoomErr(t, err, 403, CodeRoomElevatedNeedsOwner)
	// Leave is allowed for any non-sole-owner member.
	res, err := f.repo.RemoveMember(ctx, f.op(s.ref, s.viewer, s.viewer, ""))
	if err != nil || !res.Changed || !res.Left {
		t.Fatalf("viewer leave: %+v %v", res, err)
	}
	if _, active, _ := f.role(t, s.ref, s.viewer); active {
		t.Fatal("a member who left is still active")
	}
}

func TestMemberOpsNonMembersGetTheSameNotFoundAsAnUnknownSpaceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "hidden")
	outsider := f.user(t, "hidden-outsider", f.org) // org member, not in the room
	foreign := f.user(t, "hidden-foreign", f.other) // member of another org
	target := f.user(t, "hidden-target", f.org)

	_, unknown := f.repo.AddMember(ctx, f.op("ws04s4-no-such-room-"+f.suffix, s.owner, target, "viewer"))
	want := wantRoomErr(t, unknown, 404, CodeRoomNotFound)

	candidates := []struct {
		name string
		err  error
	}{
		{"non-member add", func() error { _, e := f.repo.AddMember(ctx, f.op(s.ref, outsider, target, "viewer")); return e }()},
		{"non-member change", func() error {
			_, e := f.repo.ChangeMemberRole(ctx, f.op(s.ref, outsider, s.editor, "viewer"))
			return e
		}()},
		{"non-member remove", func() error { _, e := f.repo.RemoveMember(ctx, f.op(s.ref, outsider, s.editor, "")); return e }()},
		{"other-org add", func() error {
			req := f.op(s.ref, foreign, target, "viewer")
			req.OrgID = f.other
			_, e := f.repo.AddMember(ctx, req)
			return e
		}()},
		{"actor from another org claiming this org", func() error { _, e := f.repo.AddMember(ctx, f.op(s.ref, foreign, target, "viewer")); return e }()},
	}
	for _, c := range candidates {
		got := wantRoomErr(t, c.err, 404, CodeRoomNotFound)
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s: %+v differs from the unknown-Space answer %+v", c.name, got, want)
		}
	}
}

func TestMemberOpsRefuseOrgDerivedAndSoleOwnerLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()

	derived := f.staffedRoom(t, "derived")
	f.exec(t, `UPDATE registered_spaces SET roster_mode='org_derived' WHERE space_ref=$1`, derived.ref)
	target := f.user(t, "derived-target", f.org)
	_, err := f.repo.AddMember(ctx, f.op(derived.ref, derived.owner, target, "viewer"))
	wantRoomErr(t, err, 409, CodeRoomRosterDerived)
	_, err = f.repo.ChangeMemberRole(ctx, f.op(derived.ref, derived.owner, derived.editor, "viewer"))
	wantRoomErr(t, err, 409, CodeRoomRosterDerived)
	_, err = f.repo.RemoveMember(ctx, f.op(derived.ref, derived.editor, derived.editor, ""))
	wantRoomErr(t, err, 409, CodeRoomRosterDerived)

	s := f.staffedRoom(t, "sole")
	_, err = f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.owner, s.owner, "manager"))
	wantRoomErr(t, err, 409, CodeRoomSoleOwner)
	_, err = f.repo.RemoveMember(ctx, f.op(s.ref, s.owner, s.owner, ""))
	wantRoomErr(t, err, 409, CodeRoomSoleOwner)

	// With a second owner the first may leave.
	second := f.user(t, "sole-second", f.org)
	if _, err := f.repo.AddMember(ctx, f.op(s.ref, s.owner, second, "owner")); err != nil {
		t.Fatalf("owner adds a second owner: %v", err)
	}
	if res, err := f.repo.RemoveMember(ctx, f.op(s.ref, s.owner, s.owner, "")); err != nil || !res.Left {
		t.Fatalf("first owner leaves: %+v %v", res, err)
	}
	// The remaining owner is now the sole owner again.
	_, err = f.repo.RemoveMember(ctx, f.op(s.ref, second, second, ""))
	wantRoomErr(t, err, 409, CodeRoomSoleOwner)
}

// Two owners each try to demote the other at the same moment. The room row
// lock serialises them and the actor's role is re-read under it, so exactly one
// wins and the room never ends with no owner.
func TestMemberOpsRaceToDemoteTheLastTwoOwnersLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "race")
	second := f.user(t, "race-second", f.org)
	if _, err := f.repo.AddMember(ctx, f.op(s.ref, s.owner, second, "owner")); err != nil {
		t.Fatalf("seed second owner: %v", err)
	}

	var wg sync.WaitGroup
	errs := make([]error, 2)
	start := make(chan struct{})
	for i, pair := range [][2]string{{s.owner, second}, {second, s.owner}} {
		wg.Add(1)
		go func(i int, actor, target string) {
			defer wg.Done()
			<-start
			_, errs[i] = f.repo.ChangeMemberRole(ctx, f.op(s.ref, actor, target, "manager"))
		}(i, pair[0], pair[1])
	}
	close(start)
	wg.Wait()

	successes := 0
	for _, err := range errs {
		if err == nil {
			successes++
			continue
		}
		var roomErr *RoomError
		if !errors.As(err, &roomErr) || roomErr.Status < 400 || roomErr.Status >= 500 {
			t.Fatalf("the loser must get a typed 4xx refusal, got %v", err)
		}
	}
	if successes != 1 {
		t.Fatalf("exactly one demotion must win, got %d (%v)", successes, errs)
	}
	var owners int
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM space_memberships WHERE space_ref=$1 AND role='owner' AND active`, s.ref).Scan(&owners); err != nil {
		t.Fatalf("count owners: %v", err)
	}
	if owners != 1 {
		t.Fatalf("the room must keep exactly one owner, has %d", owners)
	}
}

func TestMemberOpsExpectedRevisionIdempotencyAndNoOpsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "idem")
	target := f.user(t, "idem-target", f.org)
	nonOrg := f.user(t, "idem-nonorg", "")

	// expected_revision mismatch names the current revision.
	var revision int64
	if err := f.pool.QueryRow(ctx, `SELECT revision FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`, s.ref, s.editor).Scan(&revision); err != nil {
		t.Fatalf("read revision: %v", err)
	}
	stale := revision + 5
	req := f.op(s.ref, s.owner, s.editor, "viewer")
	req.ExpectedRevision = &stale
	_, err := f.repo.ChangeMemberRole(ctx, req)
	changed := wantRoomErr(t, err, 409, CodeRoomMemberChanged)
	if got, ok := changed.Details["current_revision"]; !ok || fmt.Sprint(got) != fmt.Sprint(revision) {
		t.Fatalf("details must carry current_revision %d, got %v", revision, changed.Details)
	}
	req.ExpectedRevision = &revision
	if res, err := f.repo.ChangeMemberRole(ctx, req); err != nil || !res.Changed || res.FromRole != "editor" {
		t.Fatalf("change with the right revision: %+v %v", res, err)
	}

	// A non-org-member target is refused with 422.
	_, err = f.repo.AddMember(ctx, f.op(s.ref, s.owner, nonOrg, "viewer"))
	wantRoomErr(t, err, 422, CodeMemberNotInOrganization)

	// Idempotency: replay returns the stored fact; reuse for another request is a 409.
	add := f.op(s.ref, s.owner, target, "editor")
	add.IdempotencyKey = "key-add-1"
	first, err := f.repo.AddMember(ctx, add)
	if err != nil || !first.Changed || first.Replayed {
		t.Fatalf("first add: %+v %v", first, err)
	}
	before := f.ledgerCount(t, s.ref)
	again, err := f.repo.AddMember(ctx, add)
	if err != nil || again.Changed || !again.Replayed {
		t.Fatalf("replay must be changed=false replayed=true: %+v %v", again, err)
	}
	if after := f.ledgerCount(t, s.ref); after != before {
		t.Fatalf("a replay appended ledger rows (%d -> %d)", before, after)
	}
	reused := f.op(s.ref, s.owner, s.viewer, "viewer")
	reused.IdempotencyKey = "key-add-1"
	_, err = f.repo.AddMember(ctx, reused)
	wantRoomErr(t, err, 409, CodeIdempotencyKeyReused)

	// AddMember on an already active target is a no-op that keeps its role.
	noop, err := f.repo.AddMember(ctx, f.op(s.ref, s.owner, s.editor, "owner"))
	if err != nil || noop.Changed {
		t.Fatalf("add on an active member: %+v %v", noop, err)
	}
	if role, _, _ := f.role(t, s.ref, s.editor); role == "owner" {
		t.Fatal("an add on an active member changed its role")
	}
	// A repeated set-to-state role change is a no-op too.
	same, err := f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.owner, s.viewer, "viewer"))
	if err != nil || same.Changed {
		t.Fatalf("change to the current role: %+v %v", same, err)
	}
}

func TestMemberOpsLedgerRowPerChangeLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "ledger")
	target := f.user(t, "ledger-target", f.org)

	steps := []struct {
		name   string
		action string
		run    func() (*MemberOpResult, error)
	}{
		{"add", ActionMemberAdded, func() (*MemberOpResult, error) {
			return f.repo.AddMember(ctx, f.op(s.ref, s.owner, target, "editor"))
		}},
		{"change", ActionRoleChanged, func() (*MemberOpResult, error) {
			return f.repo.ChangeMemberRole(ctx, f.op(s.ref, s.owner, target, "viewer"))
		}},
		{"remove", ActionMemberRemoved, func() (*MemberOpResult, error) {
			return f.repo.RemoveMember(ctx, f.op(s.ref, s.owner, target, ""))
		}},
	}
	for i, step := range steps {
		before := f.authority(t, s.ref)
		res, err := step.run()
		if err != nil || !res.Changed {
			t.Fatalf("%s: %+v %v", step.name, res, err)
		}
		if res.AuthorityRevision != before+1 {
			t.Fatalf("%s: authority revision %d, want %d", step.name, res.AuthorityRevision, before+1)
		}
		var action, actorKind, actorID string
		var spaceSeq int64
		if err := f.pool.QueryRow(ctx, `
			SELECT action, actor_kind, actor_id, space_seq FROM space_membership_events
			WHERE space_ref=$1 ORDER BY seq DESC LIMIT 1`, s.ref).Scan(&action, &actorKind, &actorID, &spaceSeq); err != nil {
			t.Fatalf("%s: read ledger: %v", step.name, err)
		}
		if action != step.action || actorKind != ActorKindUser || actorID != s.owner || spaceSeq != res.AuthorityRevision {
			t.Fatalf("%s: ledger action=%s kind=%s actor=%s seq=%d (want %s %s %s %d)", step.name, action, actorKind, actorID, spaceSeq, step.action, ActorKindUser, s.owner, res.AuthorityRevision)
		}
		if got := f.ledgerCount(t, s.ref); got != i+1 {
			t.Fatalf("%s: %d ledger rows, want %d", step.name, got, i+1)
		}
		if step.action == ActionRoleChanged && res.Event != nil && res.Event.Action != ActionRoleChanged {
			t.Fatalf("a role change carried the wrong event action %q", res.Event.Action)
		}
	}
}

// The declarative Application sync never touches a row a Control actor wrote,
// and its stale pass deactivates only application_sync rows.
func TestReplaceMembershipsIsSourceScopedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	owner := f.user(t, "sync-owner", f.org)
	ref := f.room(t, "sync", owner)
	opsMember := f.user(t, "sync-ops", f.org)
	joined := f.user(t, "sync-joined", f.org)
	synced := f.user(t, "sync-synced", f.org)
	staleSynced := f.user(t, "sync-stale", f.org)
	f.member(t, ref, opsMember, "editor", SourceMemberOps)
	f.member(t, ref, joined, "viewer", SourceSelfJoin)
	f.member(t, ref, synced, "viewer", SourceApplicationSync)
	f.member(t, ref, staleSynced, "editor", SourceApplicationSync)

	// The declaration names the two Control-owned members with other roles,
	// re-roles the synced one, and omits everyone else.
	detail, err := f.repo.ReplaceMembershipsDetailed(ctx, MembershipReplacement{
		SpaceRef: ref,
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: opsMember, Role: "viewer"},
			{SubjectType: "user", SubjectID: joined, Role: "editor"},
			{SubjectType: "user", SubjectID: synced, Role: "editor"},
		},
	})
	if err != nil {
		t.Fatalf("replace: %v", err)
	}
	if detail.SkippedControlOwned != 2 {
		t.Fatalf("skipped_control_owned = %d, want 2", detail.SkippedControlOwned)
	}
	if role, active, source := f.role(t, ref, opsMember); role != "editor" || !active || source != SourceMemberOps {
		t.Fatalf("a member_ops row was touched: %s %v %s", role, active, source)
	}
	if role, active, source := f.role(t, ref, joined); role != "viewer" || !active || source != SourceSelfJoin {
		t.Fatalf("a self_join row was touched: %s %v %s", role, active, source)
	}
	if role, active, _ := f.role(t, ref, synced); role != "editor" || !active {
		t.Fatalf("the application_sync row was not converged: %s %v", role, active)
	}
	if _, active, _ := f.role(t, ref, staleSynced); active {
		t.Fatal("an undeclared application_sync row stayed active")
	}
	if role, active, source := f.role(t, ref, owner); role != "owner" || !active || source != SourceRegistration {
		t.Fatalf("the registrar row was touched: %s %v %s", role, active, source)
	}

	// A declaration that omits the Control-owned members does not deactivate them.
	if _, err := f.repo.ReplaceMembershipsDetailed(ctx, MembershipReplacement{
		SpaceRef: ref, Members: []MemberGrant{{SubjectType: "user", SubjectID: synced, Role: "editor"}},
	}); err != nil {
		t.Fatalf("second replace: %v", err)
	}
	for _, subject := range []string{opsMember, joined} {
		if _, active, _ := f.role(t, ref, subject); !active {
			t.Fatalf("an omitted Control-owned member %s was deactivated by the sync", subject)
		}
	}
}
