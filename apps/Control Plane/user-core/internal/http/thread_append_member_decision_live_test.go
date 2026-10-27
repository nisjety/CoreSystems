package http

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS01-S2: the member matrix for a human post to a room. Only a current
// recipient with an editor-or-higher role is issued the v2 (post_kind)
// thread-append decision; a viewer, a room member outside the current
// recipient audience and a removed member are refused with no token.
//
// Gated on TEST_DATABASE_URL like the other live tests in this package; it
// reuses the d12 seed helpers (a disposable Postgres carrying the user-core
// migrations). Not run in the unit tier.

const (
	t12Org     = "ws01s2-org"
	t12Room    = "ws01s2-space-room"
	t12Owner   = "ws01s2-owner"
	t12Manager = "ws01s2-manager"
	t12Editor  = "ws01s2-editor"
	t12Viewer  = "ws01s2-viewer"
	t12Outside = "ws01s2-outside"
	t12Removed = "ws01s2-removed"
)

func t12Subjects() []string {
	return []string{t12Owner, t12Manager, t12Editor, t12Viewer, t12Outside, t12Removed}
}

func TestThreadAppendMemberDecisionMatrix(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	public := d12SigningEnv(t)
	ctx := context.Background()

	cleanup := func() {
		for _, q := range []string{
			`DELETE FROM space_recipient_audience_members WHERE space_ref = '` + t12Room + `'`,
			`DELETE FROM space_recipient_audiences WHERE space_ref = '` + t12Room + `'`,
			`DELETE FROM space_memberships WHERE space_ref = '` + t12Room + `'`,
			`DELETE FROM space_authority_revisions WHERE space_ref = '` + t12Room + `'`,
			`DELETE FROM registered_spaces WHERE space_ref = '` + t12Room + `'`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + t12Org + `'`,
			`DELETE FROM users WHERE id LIKE 'ws01s2-%'`,
			`DELETE FROM space_effect_policies WHERE org_id = '` + t12Org + `'`,
		} {
			_, _ = pool.Exec(ctx, q)
		}
	}
	cleanup()
	t.Cleanup(cleanup)

	if _, err := pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room',$3,1,'active')`, t12Room, t12Org, t12Owner); err != nil {
		t.Fatalf("seed room: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, t12Room); err != nil {
		t.Fatalf("seed revisions: %v", err)
	}
	for _, user := range t12Subjects() {
		if _, err := pool.Exec(ctx, `INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws01s2.invalid', $1)`, user); err != nil {
			t.Fatalf("seed user %s: %v", user, err)
		}
		if _, err := pool.Exec(ctx, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member','active')`, user, t12Org); err != nil {
			t.Fatalf("seed org membership %s: %v", user, err)
		}
	}
	// outside: a room member who is not in the published recipient audience.
	// removed: had a membership, then lost it.
	for _, member := range []struct{ subject, role string }{
		{t12Owner, "owner"}, {t12Manager, "manager"}, {t12Editor, "editor"},
		{t12Viewer, "viewer"}, {t12Outside, "editor"}, {t12Removed, "editor"},
	} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
			VALUES ($1,'user',$2,$3,'ws01s2-test')`, t12Room, member.subject, member.role); err != nil {
			t.Fatalf("seed membership %s: %v", member.subject, err)
		}
	}
	if _, err := pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref = $1 AND subject_id = $2`, t12Room, t12Removed); err != nil {
		t.Fatalf("remove member: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:ws01s2-space-room:1', 'sha256:ws01s2-audience', 4)`, t12Room); err != nil {
		t.Fatalf("seed audience: %v", err)
	}
	for _, subject := range []string{t12Owner, t12Manager, t12Editor, t12Viewer} {
		if _, err := pool.Exec(ctx, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, t12Room, subject); err != nil {
			t.Fatalf("seed audience member %s: %v", subject, err)
		}
	}
	if _, err := repo.UpsertEffectPolicy(ctx, spaces.EffectPolicy{
		OrgID: t12Org, PrivacyPolicyRef: "privacy:ws01s2-org:1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
		ThreadCreateEntitled: true,
	}, spaces.EffectPolicyActor{Type: "service", ID: "ws01s2-test"}); err != nil {
		t.Fatalf("seed effect policy: %v", err)
	}

	s := &Server{}
	s.SetSpaceRepository(repo)

	post := func(user, postKind string) *httptest.ResponseRecorder {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		payload, _ := json.Marshal(map[string]string{
			"space_ref": t12Room, "thread_id": "thread-1",
			"content_digest":  "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
			"idempotency_key": "ws01s2-append-1", "post_kind": postKind,
		})
		c.Request = httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/thread-append-decision", bytes.NewReader(payload))
		c.Request.Header.Set("Content-Type", "application/json")
		c.Set("org_id", t12Org)
		c.Set("user_id", user)
		s.issueThreadAppendDecision(c)
		return w
	}

	for _, member := range []string{t12Owner, t12Manager, t12Editor} {
		w := post(member, "post")
		if w.Code != http.StatusOK {
			t.Fatalf("%s post: want 200, got %d (%s)", member, w.Code, w.Body.String())
		}
		var response d12DecisionBody
		if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
			t.Fatalf("decode: %v", err)
		}
		decision := d12VerifyToken(t, response.Data.Token, public)
		if decision.SubjectID != member || decision.ActionSchemaHash != "sha256:thread-append-v2" {
			t.Fatalf("%s post decision bound to subject %q schema %q", member, decision.SubjectID, decision.ActionSchemaHash)
		}
	}
	for _, member := range []string{t12Viewer, t12Outside, t12Removed} {
		w := post(member, "post")
		if w.Code != http.StatusForbidden {
			t.Fatalf("%s post: want 403, got %d (%s)", member, w.Code, w.Body.String())
		}
		if bytes.Contains(w.Body.Bytes(), []byte(`"token"`)) {
			t.Fatalf("%s refusal carried a token: %s", member, w.Body.String())
		}
	}
}
