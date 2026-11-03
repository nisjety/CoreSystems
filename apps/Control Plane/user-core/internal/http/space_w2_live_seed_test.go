package http

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// Shared seed for the W2 live tests (TEST_DATABASE_URL, disposable Postgres
// carrying every user-core migration). One shared room per prefix with an org,
// five human members (owner, manager, editor, viewer; "outside" is a room
// member left out of the published recipient audience), one bound service
// agent, a current recipient audience and an effect policy.

type w2Room struct {
	Prefix  string
	Org     string
	Room    string
	Owner   string
	Manager string
	Editor  string
	Viewer  string
	Outside string
	Agent   string
}

func newW2Room(prefix string) w2Room {
	return w2Room{
		Prefix: prefix, Org: prefix + "-org", Room: prefix + "-space-room",
		Owner: prefix + "-owner", Manager: prefix + "-manager", Editor: prefix + "-editor",
		Viewer: prefix + "-viewer", Outside: prefix + "-outside", Agent: prefix + "-agent",
	}
}

func (r w2Room) humans() []string {
	return []string{r.Owner, r.Manager, r.Editor, r.Viewer, r.Outside}
}

// seedW2Room (re)creates the room. mutate adjusts the effect policy before it is
// written; the base policy has every entitlement off.
func seedW2Room(t *testing.T, pool *pgxpool.Pool, repo *spaces.Repository, r w2Room, mutate func(*spaces.EffectPolicy)) {
	t.Helper()
	ctx := context.Background()
	cleanup := func() {
		for _, q := range []string{
			`DELETE FROM space_recipient_audience_members WHERE space_ref = '` + r.Room + `'`,
			`DELETE FROM space_recipient_audiences WHERE space_ref = '` + r.Room + `'`,
			`DELETE FROM space_memberships WHERE space_ref = '` + r.Room + `'`,
			`DELETE FROM space_authority_revisions WHERE space_ref = '` + r.Room + `'`,
			`DELETE FROM registered_spaces WHERE space_ref = '` + r.Room + `'`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + r.Org + `'`,
			`DELETE FROM users WHERE id LIKE '` + r.Prefix + `-%'`,
			`DELETE FROM space_effect_policies WHERE org_id = '` + r.Org + `'`,
		} {
			_, _ = pool.Exec(ctx, q)
		}
	}
	cleanup()
	t.Cleanup(cleanup)

	if _, err := pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room',$3,1,'active')`, r.Room, r.Org, r.Owner); err != nil {
		t.Fatalf("seed room: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, r.Room); err != nil {
		t.Fatalf("seed revisions: %v", err)
	}
	for _, user := range r.humans() {
		if _, err := pool.Exec(ctx, `INSERT INTO users (id, email, name) VALUES ($1, $1 || '@w2.invalid', $1)`, user); err != nil {
			t.Fatalf("seed user %s: %v", user, err)
		}
		if _, err := pool.Exec(ctx, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member','active')`, user, r.Org); err != nil {
			t.Fatalf("seed org membership %s: %v", user, err)
		}
	}
	for _, member := range []struct{ subject, role string }{
		{r.Owner, "owner"}, {r.Manager, "manager"}, {r.Editor, "editor"}, {r.Viewer, "viewer"}, {r.Outside, "manager"},
	} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
			VALUES ($1,'user',$2,$3,'w2-test')`, r.Room, member.subject, member.role); err != nil {
			t.Fatalf("seed membership %s: %v", member.subject, err)
		}
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
		VALUES ($1,'service',$2,'editor','w2-test')`, r.Room, r.Agent); err != nil {
		t.Fatalf("seed agent membership: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:`+r.Room+`:1', 'sha256:`+r.Prefix+`-audience', 4)`, r.Room); err != nil {
		t.Fatalf("seed audience: %v", err)
	}
	for _, subject := range []string{r.Owner, r.Manager, r.Editor, r.Viewer} {
		if _, err := pool.Exec(ctx, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, r.Room, subject); err != nil {
			t.Fatalf("seed audience member %s: %v", subject, err)
		}
	}
	policy := spaces.EffectPolicy{
		OrgID: r.Org, PrivacyPolicyRef: "privacy:" + r.Org + ":1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
	}
	if mutate != nil {
		mutate(&policy)
	}
	if _, err := repo.UpsertEffectPolicy(ctx, policy, spaces.EffectPolicyActor{Type: "service", ID: "w2-test"}); err != nil {
		t.Fatalf("seed effect policy: %v", err)
	}
}

// w2Post drives a handler as `user` in `org` with a JSON body.
func w2Post(s *Server, handler func(*Server, *gin.Context), path, org, user, body string) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	c.Set("org_id", org)
	c.Set("user_id", user)
	handler(s, c)
	return w
}
