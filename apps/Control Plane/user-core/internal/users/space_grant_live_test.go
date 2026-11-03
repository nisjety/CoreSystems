package users_test

import (
	"context"
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS14-S2 live proofs for the Space grant subject (migration 043). Gated on
// TEST_DATABASE_URL (a disposable Postgres; every user-core migration is
// applied here). All rows live under a unique org and are removed afterwards.

type spaceGrantFixture struct {
	pool *pgxpool.Pool
	acl  *users.AclRepository
	org  string
	pfx  string
}

func newSpaceGrantFixture(t *testing.T) *spaceGrantFixture {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live Space grant test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	db := &database.DB{Pool: pool}
	if err := db.RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	pfx := fmt.Sprintf("ws14s2-%d", time.Now().UnixNano())
	f := &spaceGrantFixture{pool: pool, acl: users.NewAclRepository(db), org: pfx + "-org", pfx: pfx}
	t.Cleanup(func() {
		bg := context.Background()
		for _, q := range []string{
			`DELETE FROM resource_grants WHERE org_id = '` + f.org + `'`,
			`DELETE FROM space_recipient_audience_members WHERE space_ref LIKE '` + pfx + `-%'`,
			`DELETE FROM space_recipient_audiences WHERE space_ref LIKE '` + pfx + `-%'`,
			`DELETE FROM space_memberships WHERE space_ref LIKE '` + pfx + `-%'`,
			`DELETE FROM space_authority_revisions WHERE space_ref LIKE '` + pfx + `-%'`,
			`DELETE FROM registered_spaces WHERE space_ref LIKE '` + pfx + `-%'`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + f.org + `'`,
			`DELETE FROM users WHERE id LIKE '` + pfx + `-%'`,
		} {
			_, _ = pool.Exec(bg, q)
		}
	})
	return f
}

func (f *spaceGrantFixture) exec(t *testing.T, query string, args ...any) {
	t.Helper()
	if _, err := f.pool.Exec(context.Background(), query, args...); err != nil {
		t.Fatalf("exec %q: %v", query, err)
	}
}

func (f *spaceGrantFixture) user(t *testing.T, name string, orgStatus string) string {
	t.Helper()
	id := f.pfx + "-" + name
	f.exec(t, `INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws14.invalid', $1)`, id)
	f.exec(t, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member',$3)`, id, f.org, orgStatus)
	return id
}

// space registers a Space of the given kind/state in org, optionally with the
// owner as a member and an audience revision containing `audience`.
func (f *spaceGrantFixture) space(t *testing.T, name, org, kind, state, owner string, audience ...string) string {
	t.Helper()
	ref := f.pfx + "-" + name
	f.exec(t, `INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state)
	           VALUES ($1,$2,$3,$4,1,$5)`, ref, org, kind, owner, state)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, ref)
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'owner','ws14s2')`, ref, owner)
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
	           VALUES ($1, 1, 'audience:'||$1||':1', 'sha256:'||$1, $2)`, ref, len(audience))
	for _, subject := range audience {
		f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, ref, subject)
	}
	return ref
}

func TestGrantSpaceSubjectNeedsAnActiveSharedSpaceOfTheOrgAndIsIdempotentLive(t *testing.T) {
	f := newSpaceGrantFixture(t)
	ctx := context.Background()
	owner := f.user(t, "owner", "active")
	room := f.space(t, "room", f.org, "room", "active", owner, owner)
	personal := f.space(t, "personal", f.org, "personal", "active", owner, owner)
	archived := f.space(t, "archived", f.org, "room", "archived", owner, owner)
	suspended := f.space(t, "suspended", f.org, "room", "suspended", owner, owner)

	// Created, then replayed: the first granter and time stand, created=false.
	grant, created, err := f.acl.GrantSpaceSubject(ctx, f.org, "document", "doc-1", room, owner)
	if err != nil || !created {
		t.Fatalf("first grant: created=%v err=%v", created, err)
	}
	if grant.SubjectType != "space" || grant.SubjectID != room || grant.Role != "view" || grant.GrantedBy != owner {
		t.Fatalf("grant = %+v", grant)
	}
	again, created, err := f.acl.GrantSpaceSubject(ctx, f.org, "document", "doc-1", room, f.pfx+"-someone-else")
	if err != nil || created {
		t.Fatalf("replay: created=%v err=%v", created, err)
	}
	if again.GrantID != grant.GrantID || again.GrantedBy != owner || !again.GrantedAt.Equal(grant.GrantedAt) {
		t.Fatalf("a replay must keep the first grant: %+v vs %+v", again, grant)
	}

	refusals := map[string]struct {
		org, rtype, space string
	}{
		"unknown Space":         {f.org, "document", f.pfx + "-nope"},
		"personal Space":        {f.org, "document", personal},
		"another org's Space":   {f.org + "-other", "document", room},
		"archived Space":        {f.org, "document", archived},
		"suspended Space":       {f.org, "document", suspended},
		"non-grantable type":    {f.org, "inbox", room},
		"unknown resource type": {f.org, "nope", room},
	}
	for name, tc := range refusals {
		_, created, err := f.acl.GrantSpaceSubject(ctx, tc.org, tc.rtype, "doc-2", tc.space, owner)
		if err == nil || created {
			t.Errorf("%s: expected a refusal, got created=%v err=%v", name, created, err)
			continue
		}
		if name != "unknown resource type" && name != "non-grantable type" && !errors.Is(err, users.ErrSpaceNotGrantable) {
			t.Errorf("%s: error = %v, want ErrSpaceNotGrantable", name, err)
		}
	}
	var rows int
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE org_id = $1 AND resource_id = 'doc-2'`, f.org).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("refusals wrote %d rows (err %v)", rows, err)
	}
}

func TestListVisibleForSpaceNeedsCurrentMembershipAndAudienceLive(t *testing.T) {
	f := newSpaceGrantFixture(t)
	ctx := context.Background()
	owner := f.user(t, "owner", "active")
	member := f.user(t, "member", "active")
	removed := f.user(t, "removed", "active")
	suspendedOrg := f.user(t, "suspendedorg", "suspended")
	stranger := f.user(t, "stranger", "active")

	room := f.space(t, "room", f.org, "room", "active", owner, owner, member, removed, suspendedOrg)
	for _, who := range []string{member, removed, suspendedOrg} {
		f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'viewer','ws14s2')`, room, who)
	}
	f.exec(t, `UPDATE space_memberships SET active = FALSE WHERE space_ref = $1 AND subject_id = $2`, room, removed)

	for _, doc := range []string{"doc-b", "doc-a"} {
		if _, _, err := f.acl.GrantSpaceSubject(ctx, f.org, "document", doc, room, owner); err != nil {
			t.Fatalf("grant %s: %v", doc, err)
		}
	}

	list := func(org, space, user string) ([]string, int64, bool) {
		ids, revision, ok, err := f.acl.ListVisibleForSpace(ctx, org, "document", space, user)
		if err != nil {
			t.Fatalf("ListVisibleForSpace(%s,%s): %v", space, user, err)
		}
		return ids, revision, ok
	}

	ids, revision, ok := list(f.org, room, member)
	if !ok || revision != 1 || len(ids) != 2 {
		t.Fatalf("member: ids=%v revision=%d ok=%v", ids, revision, ok)
	}
	// The order is the grant order, not alphabetical: doc-b was attached first.
	if ids[0] != "doc-b" || ids[1] != "doc-a" {
		t.Fatalf("ids must follow grant order: %v", ids)
	}

	for name, who := range map[string]string{"removed member": removed, "org-suspended member": suspendedOrg, "not a member": stranger} {
		if ids, _, ok := list(f.org, room, who); ok || len(ids) != 0 {
			t.Errorf("%s must resolve to an empty set, got %v ok=%v", name, ids, ok)
		}
	}
	if ids, _, ok := list(f.org+"-other", room, member); ok || len(ids) != 0 {
		t.Errorf("another org must see nothing, got %v", ids)
	}

	// A new audience revision that no longer lists the member: the Space row is
	// not honoured for them any more (the join is on the CURRENT revision).
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count) VALUES ($1, 2, 'audience:'||$1||':2', 'sha256:v2', 1)`, room)
	f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 2, $2)`, room, owner)
	f.exec(t, `UPDATE space_authority_revisions SET recipient_audience_revision = 2 WHERE space_ref = $1`, room)
	if ids, _, ok := list(f.org, room, member); ok || len(ids) != 0 {
		t.Errorf("a member outside the current audience revision must see nothing, got %v", ids)
	}
	if ids, revision, ok := list(f.org, room, owner); !ok || revision != 2 || len(ids) != 2 {
		t.Errorf("owner in the new revision: ids=%v revision=%d ok=%v", ids, revision, ok)
	}

	// Personal Spaces never resolve through a Space subject; archived rooms do
	// (read-only).
	personal := f.space(t, "personal", f.org, "personal", "active", owner, owner)
	if ids, _, ok := list(f.org, personal, owner); ok || len(ids) != 0 {
		t.Errorf("personal Space must resolve to nothing, got %v", ids)
	}
	f.exec(t, `UPDATE registered_spaces SET registration_state = 'archived' WHERE space_ref = $1`, room)
	if ids, _, ok := list(f.org, room, owner); !ok || len(ids) != 2 {
		t.Errorf("an archived room stays readable: ids=%v ok=%v", ids, ok)
	}
	f.exec(t, `UPDATE registered_spaces SET registration_state = 'suspended' WHERE space_ref = $1`, room)
	if ids, _, ok := list(f.org, room, owner); ok || len(ids) != 0 {
		t.Errorf("a suspended room must not resolve, got %v ok=%v", ids, ok)
	}
}

func TestSpaceDeletionRemovesTheSpaceGrantsLive(t *testing.T) {
	f := newSpaceGrantFixture(t)
	ctx := context.Background()
	owner := f.user(t, "owner", "active")
	room := f.space(t, "room", f.org, "room", "active", owner, owner)
	other := f.space(t, "other", f.org, "room", "active", owner, owner)
	for _, ref := range []string{room, other} {
		if _, _, err := f.acl.GrantSpaceSubject(ctx, f.org, "document", "doc-"+ref, ref, owner); err != nil {
			t.Fatalf("grant %s: %v", ref, err)
		}
	}
	// A user grant on the same document must survive: it is the owner's ACL.
	f.exec(t, `INSERT INTO resource_grants (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by)
	           VALUES ('g-user', $1, 'document', $2, 'user', $3, 'view', $3)`, f.org, "doc-"+room, owner)

	repo := spaces.NewRepository(&database.DB{Pool: f.pool})
	if _, err := repo.Register(ctx, spaces.Registration{
		SpaceRef: room, OrgID: f.org, OwnerPrincipalID: owner, Kind: spaces.KindRoom,
		Lifecycle: spaces.LifecycleDeleting, LifecycleRevision: 2,
	}); err != nil {
		t.Fatalf("delete the Space: %v", err)
	}

	count := func(query string, args ...any) int {
		var n int
		if err := f.pool.QueryRow(ctx, query, args...).Scan(&n); err != nil {
			t.Fatalf("count: %v", err)
		}
		return n
	}
	if n := count(`SELECT count(*) FROM resource_grants WHERE subject_type='space' AND subject_id=$1`, room); n != 0 {
		t.Fatalf("the deleted Space still holds %d grants", n)
	}
	if n := count(`SELECT count(*) FROM resource_grants WHERE subject_type='space' AND subject_id=$1`, other); n != 1 {
		t.Fatalf("another Space's grant was removed: %d left", n)
	}
	if n := count(`SELECT count(*) FROM resource_grants WHERE grant_id='g-user'`); n != 1 {
		t.Fatal("the owner's user grant must survive Space deletion")
	}

	// The standalone form removes the rest.
	removed, err := f.acl.RevokeSpaceGrants(ctx, other)
	if err != nil || removed != 1 {
		t.Fatalf("RevokeSpaceGrants: removed=%d err=%v", removed, err)
	}
}

func TestSpaceGrantRowsAreViewOnlyByCheckLive(t *testing.T) {
	f := newSpaceGrantFixture(t)
	ctx := context.Background()
	insert := func(role, subjectType string) error {
		_, err := f.pool.Exec(ctx, `INSERT INTO resource_grants (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by)
		           VALUES ($1, $2, 'document', 'doc-check', $3, $4, $5, 'ws14s2')`,
			f.pfx+"-"+role+"-"+subjectType, f.org, subjectType, f.pfx+"-subject-"+role+subjectType, role)
		return err
	}
	if err := insert("view", "space"); err != nil {
		t.Fatalf("a view Space row must be allowed: %v", err)
	}
	if err := insert("edit", "space"); err == nil {
		t.Fatal("an edit Space row must be rejected by the CHECK")
	}
	if err := insert("edit", "user"); err != nil {
		t.Fatalf("an edit user row is unaffected: %v", err)
	}
	if err := insert("view", "robot"); err == nil {
		t.Fatal("an unknown subject_type must still be rejected")
	}
}
