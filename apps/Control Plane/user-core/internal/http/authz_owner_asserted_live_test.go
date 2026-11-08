package http

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS14-S2 live legs (TEST_DATABASE_URL via d12Pool): the DB-backed half of the
// owner-asserted grant writer and the room-file grant route.

func liveGrantServer(pool *pgxpool.Pool, repo *spaces.Repository) *Server {
	s := &Server{}
	s.SetSpaceRepository(repo)
	s.aclRepo = users.NewAclRepository(&database.DB{Pool: pool})
	s.SetOwnerAssertedGrants(true)
	s.SetSpaceSubjectGrants(true)
	return s
}

func TestOwnerAssertedGrantTakesGrantedByFromTheDelegationAndChecksTheSubjectLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("ws14s2g")
	seedW2Room(t, pool, repo, room, nil)
	s := liveGrantServer(pool, repo)
	ctx := context.Background()
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, room.Org) })

	delegation := with(ownerDelegation("authz:grant", "document", "doc-1"), map[string]any{"org_id": room.Org, "user_id": room.Owner})
	handler := func(c *gin.Context) { s.authzGrant(c) }
	grant := func(subject, extra string) *struct {
		code int
		body string
	} {
		engine, _ := authzEngine(delegation, s, handler)
		body := `{"org_id":"` + room.Org + `","resource_type":"document","resource_id":"doc-1","subject_id":"` + subject + `","role":"view","granted_by":"forged-admin"` + extra + `}`
		rec := authzDo(engine, http.MethodPost, authzGrantPath, body)
		return &struct {
			code int
			body string
		}{rec.Code, rec.Body.String()}
	}

	// An org member: stored, and granted_by is the delegated user, not the body.
	if got := grant(room.Viewer, ""); got.code != http.StatusOK {
		t.Fatalf("org member: %d %s", got.code, got.body)
	}
	var grantedBy string
	if err := pool.QueryRow(ctx, `SELECT granted_by FROM resource_grants WHERE org_id=$1 AND subject_id=$2`, room.Org, room.Viewer).Scan(&grantedBy); err != nil {
		t.Fatalf("read grant: %v", err)
	}
	if grantedBy != room.Owner {
		t.Fatalf("granted_by = %q, want the delegated user %q (never the body)", grantedBy, room.Owner)
	}

	// A subject outside the org is refused with the typed code and nothing is written.
	got := grant("ws14s2g-not-in-the-org", "")
	if got.code != http.StatusBadRequest || !strings.Contains(got.body, spaces.CodeMemberNotInOrganization) {
		t.Fatalf("non-member subject: %d %s", got.code, got.body)
	}
	var n int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE org_id=$1 AND subject_id=$2`, room.Org, "ws14s2g-not-in-the-org").Scan(&n); err != nil || n != 0 {
		t.Fatalf("a refused grant wrote %d rows (err %v)", n, err)
	}
}

func TestSpaceVisibleReturnsOnlyWhatTheCurrentMemberMaySeeLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("ws14s2v")
	seedW2Room(t, pool, repo, room, nil)
	s := liveGrantServer(pool, repo)
	ctx := context.Background()
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, room.Org) })
	if _, _, err := s.aclRepo.GrantSpaceSubject(ctx, room.Org, "document", "doc-v", room.Room, room.Owner); err != nil {
		t.Fatalf("attach: %v", err)
	}

	visible := func(user string) (int, map[string]any) {
		ctxMap := with(ownerDelegation("authz:space-visible", "document", room.Room), map[string]any{"org_id": room.Org, "user_id": user})
		engine, _ := authzEngine(ctxMap, s, func(c *gin.Context) { s.authzSpaceVisible(c) })
		rec := authzDo(engine, http.MethodGet,
			authzSpaceVisiblePath+"?org_id="+room.Org+"&subject_id="+user+"&resource_type=document&space_ref="+room.Room, "")
		var body map[string]any
		_ = json.Unmarshal(rec.Body.Bytes(), &body)
		return rec.Code, body
	}
	code, body := visible(room.Editor)
	if code != http.StatusOK || len(body["ids"].([]any)) != 1 || body["audience_revision"] != float64(1) {
		t.Fatalf("editor: %d %v", code, body)
	}
	// Outside the audience: an empty set, never an error that leaks existence.
	code, body = visible(room.Outside)
	if code != http.StatusOK || len(body["ids"].([]any)) != 0 {
		t.Fatalf("outside the audience: %d %v", code, body)
	}
}

func fileGrantLiveServer(t *testing.T, pool *pgxpool.Pool, repo *spaces.Repository) *Server {
	t.Helper()
	d12SigningEnv(t)
	return liveGrantServer(pool, repo)
}

func importEntitled(p *spaces.EffectPolicy) { p.ImportWriteEntitled = true }

func TestFileGrantAuthorityMatrixLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("ws14s2f")
	seedW2Room(t, pool, repo, room, importEntitled)
	s := fileGrantLiveServer(t, pool, repo)
	ctx := context.Background()
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, room.Org) })
	engine := fileGrantEngine(documentsAPIContext(), s)

	attach := func(user string, mutate func(*spaces.Decision)) (int, map[string]any) {
		token := signFileGrantToken(t, func(d *spaces.Decision) {
			d.OrgID, d.SpaceRef, d.SubjectID = room.Org, room.Room, user
			if mutate != nil {
				mutate(d)
			}
		}, nil)
		rec := authzDo(engine, http.MethodPost, fileGrantPath, fileGrantBody(token))
		var body map[string]any
		_ = json.Unmarshal(rec.Body.Bytes(), &body)
		return rec.Code, body
	}
	countGrants := func() int {
		var n int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE org_id=$1 AND subject_type='space'`, room.Org).Scan(&n); err != nil {
			t.Fatal(err)
		}
		return n
	}

	// Refusals first: none of them may leave a row behind.
	if _, err := pool.Exec(ctx, `UPDATE space_memberships SET active = FALSE WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Editor); err != nil {
		t.Fatal(err)
	}
	for name, tc := range map[string]struct {
		user string
		code string
	}{
		"removed member":          {room.Editor, string(spaces.DenialMembershipRequired)},
		"viewer role":             {room.Viewer, string(spaces.DenialRoleBelowFloor)},
		"not in current audience": {room.Outside, string(spaces.DenialMembershipRequired)},
		"stranger":                {"ws14s2f-stranger", string(spaces.DenialMembershipRequired)},
	} {
		code, body := attach(tc.user, nil)
		if code != http.StatusForbidden || body["code"] != tc.code {
			t.Errorf("%s: %d %v, want 403 %s", name, code, body, tc.code)
		}
	}
	if n := countGrants(); n != 0 {
		t.Fatalf("refusals left %d Space grants", n)
	}

	// A current editor attaches; the replay reports created=false.
	if _, err := pool.Exec(ctx, `UPDATE space_memberships SET active = TRUE WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Editor); err != nil {
		t.Fatal(err)
	}
	code, body := attach(room.Editor, nil)
	data, _ := body["data"].(map[string]any)
	if code != http.StatusOK || data == nil || data["created"] != true || data["grant_id"] == "" {
		t.Fatalf("first attach: %d %v", code, body)
	}
	code, body = attach(room.Editor, nil)
	data, _ = body["data"].(map[string]any)
	if code != http.StatusOK || data == nil || data["created"] != false {
		t.Fatalf("replay: %d %v", code, body)
	}
	if n := countGrants(); n != 1 {
		t.Fatalf("grants after a replay = %d, want 1", n)
	}

	// A manager and an owner may attach as well.
	for _, user := range []string{room.Manager, room.Owner} {
		if code, body := attach(user, nil); code != http.StatusOK {
			t.Errorf("%s: %d %v", user, code, body)
		}
	}
}

func TestFileGrantRefusesWhenTheEntitlementIsOffOrZDRIsOnLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	ctx := context.Background()

	off := newW2Room("ws14s2e")
	seedW2Room(t, pool, repo, off, nil) // import entitlement off
	s := fileGrantLiveServer(t, pool, repo)
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id IN ($1,$2)`, off.Org, "ws14s2z-org")
	})
	token := signFileGrantToken(t, func(d *spaces.Decision) { d.OrgID, d.SpaceRef, d.SubjectID = off.Org, off.Room, off.Editor }, nil)
	rec := authzDo(fileGrantEngine(documentsAPIContext(), s), http.MethodPost, fileGrantPath, fileGrantBody(token))
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), string(spaces.DenialEntitlementOff)) {
		t.Fatalf("entitlement off: %d %s", rec.Code, rec.Body.String())
	}

	// The org flips to ZDR after the decision was issued (the token itself is
	// not ZDR): Control re-resolves NOW and refuses.
	zdr := newW2Room("ws14s2z")
	seedW2Room(t, pool, repo, zdr, func(p *spaces.EffectPolicy) { p.ImportWriteEntitled = true; p.ZeroDataRetention = true })
	token = signFileGrantToken(t, func(d *spaces.Decision) { d.OrgID, d.SpaceRef, d.SubjectID = zdr.Org, zdr.Room, zdr.Editor }, nil)
	rec = authzDo(fileGrantEngine(documentsAPIContext(), s), http.MethodPost, fileGrantPath, fileGrantBody(token))
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), string(spaces.DenialZDRForbidsEffect)) {
		t.Fatalf("ZDR now: %d %s", rec.Code, rec.Body.String())
	}
}
