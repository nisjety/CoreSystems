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

// WS04-S7 live proof of the archived-room answers of the gateway-called
// routes: the two write decisions answer a typed 409 room_archived to an
// active member and keep the unchanged denial for everyone else, and the
// membership read reports the archive state only when the caller asks for the
// readable view. Gated on TEST_DATABASE_URL like the other live tests in this
// package; it reuses d12Pool and d12SigningEnv. Not run in the unit tier.

const (
	t7Org     = "ws04s7-org"
	t7Room    = "ws04s7-room"
	t7Owner   = "ws04s7-owner"
	t7Editor  = "ws04s7-editor"
	t7Outside = "ws04s7-outside"
)

func TestArchivedRoomAnswersOfTheGatewayRoutesLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12SigningEnv(t)
	ctx := context.Background()

	cleanup := func() {
		for _, q := range []string{
			`DELETE FROM space_recipient_audience_members WHERE space_ref = '` + t7Room + `'`,
			`DELETE FROM space_recipient_audiences WHERE space_ref = '` + t7Room + `'`,
			`DELETE FROM space_memberships WHERE space_ref = '` + t7Room + `'`,
			`DELETE FROM space_authority_revisions WHERE space_ref = '` + t7Room + `'`,
			`DELETE FROM registered_spaces WHERE space_ref = '` + t7Room + `'`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + t7Org + `'`,
			`DELETE FROM users WHERE id LIKE 'ws04s7-%'`,
			`DELETE FROM space_effect_policies WHERE org_id = '` + t7Org + `'`,
		} {
			_, _ = pool.Exec(ctx, q)
		}
	}
	cleanup()
	t.Cleanup(cleanup)

	exec := func(query string, args ...any) {
		t.Helper()
		if _, err := pool.Exec(ctx, query, args...); err != nil {
			t.Fatalf("%s: %v", query, err)
		}
	}
	exec(`INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		application_lifecycle_revision, registration_state) VALUES ($1,$2,'room',$3,1,'active')`, t7Room, t7Org, t7Owner)
	exec(`INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, t7Room)
	for _, user := range []string{t7Owner, t7Editor, t7Outside} {
		exec(`INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws04s7.invalid', $1)`, user)
		exec(`INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member','active')`, user, t7Org)
	}
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'owner','ws04s7-test')`, t7Room, t7Owner)
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'editor','ws04s7-test')`, t7Room, t7Editor)
	exec(`INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:ws04s7-room:1', 'sha256:ws04s7-audience', 2)`, t7Room)
	exec(`UPDATE space_authority_revisions SET recipient_audience_revision = 1 WHERE space_ref = $1`, t7Room)
	for _, subject := range []string{t7Owner, t7Editor} {
		exec(`INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, t7Room, subject)
	}
	if _, err := repo.UpsertEffectPolicy(ctx, spaces.EffectPolicy{
		OrgID: t7Org, PrivacyPolicyRef: "privacy:ws04s7-org:1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space", ThreadCreateEntitled: true,
	}, spaces.EffectPolicyActor{Type: "service", ID: "ws04s7-test"}); err != nil {
		t.Fatalf("seed effect policy: %v", err)
	}

	s := &Server{}
	s.SetSpaceRepository(repo)

	decision := func(handler gin.HandlerFunc, path, user, spaceRef string) *httptest.ResponseRecorder {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		payload, _ := json.Marshal(map[string]string{
			"space_ref": spaceRef, "thread_id": "thread-1",
			"content_digest":  "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
			"idempotency_key": "ws04s7-key-1",
		})
		c.Request = httptest.NewRequest(http.MethodPost, path, bytes.NewReader(payload))
		c.Request.Header.Set("Content-Type", "application/json")
		c.Set("org_id", t7Org)
		c.Set("user_id", user)
		handler(c)
		return w
	}
	membership := func(query, user, spaceRef string) *httptest.ResponseRecorder {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = httptest.NewRequest(http.MethodGet, "/api/v1/internal/spaces/"+spaceRef+"/membership"+query, nil)
		c.Params = gin.Params{{Key: "space_ref", Value: spaceRef}}
		c.Set("org_id", t7Org)
		c.Set("user_id", user)
		s.resolveCurrentSpaceMembership(c)
		return w
	}

	// Active room: the member is issued a decision and the membership read works.
	if w := decision(s.issueThreadDecision, "/api/v1/internal/spaces/thread-decision", t7Editor, t7Room); w.Code != http.StatusOK {
		t.Fatalf("active room thread-decision: %d (%s)", w.Code, w.Body.String())
	}

	exec(`UPDATE registered_spaces SET registration_state='archived', archived_at=NOW(), archived_by=$2 WHERE space_ref=$1`, t7Room, t7Owner)

	type route struct {
		name    string
		handler gin.HandlerFunc
		path    string
	}
	routes := []route{
		{"thread-decision", s.issueThreadDecision, "/api/v1/internal/spaces/thread-decision"},
		{"thread-append-decision", s.issueThreadAppendDecision, "/api/v1/internal/spaces/thread-append-decision"},
	}
	for _, r := range routes {
		w := decision(r.handler, r.path, t7Editor, t7Room)
		if w.Code != http.StatusConflict {
			t.Fatalf("%s for an archived-room member: want 409, got %d (%s)", r.name, w.Code, w.Body.String())
		}
		var body struct {
			Error struct {
				Code string `json:"code"`
			} `json:"error"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil || body.Error.Code != spaces.CodeRoomArchived {
			t.Fatalf("%s: want typed room_archived, got %s", r.name, w.Body.String())
		}
		if bytes.Contains(w.Body.Bytes(), []byte(`"token"`)) {
			t.Fatalf("%s: an archived-room refusal carried a token", r.name)
		}

		// A non-member keeps the unchanged denial, identical to an unknown room.
		outside := decision(r.handler, r.path, t7Outside, t7Room)
		unknown := decision(r.handler, r.path, t7Outside, "ws04s7-no-such-room")
		if outside.Code != http.StatusForbidden {
			t.Fatalf("%s for a non-member: want 403, got %d (%s)", r.name, outside.Code, outside.Body.String())
		}
		if outside.Code != unknown.Code || !bytes.Equal(outside.Body.Bytes(), unknown.Body.Bytes()) {
			t.Fatalf("%s: a non-member's answer reveals the archived room:\n%s\nvs unknown:\n%s", r.name, outside.Body.String(), unknown.Body.String())
		}
	}

	// ?readable=1 reports the archive state; the plain read stays strict.
	readable := membership("?readable=1", t7Editor, t7Room)
	if readable.Code != http.StatusOK {
		t.Fatalf("readable membership: want 200, got %d (%s)", readable.Code, readable.Body.String())
	}
	var data struct {
		Data struct {
			State      string  `json:"state"`
			ArchivedAt *string `json:"archived_at"`
		} `json:"data"`
	}
	if err := json.Unmarshal(readable.Body.Bytes(), &data); err != nil || data.Data.State != "archived" || data.Data.ArchivedAt == nil {
		t.Fatalf("readable membership body: %s", readable.Body.String())
	}
	if strict := membership("", t7Editor, t7Room); strict.Code != http.StatusForbidden {
		t.Fatalf("plain membership of an archived room: want 403, got %d (%s)", strict.Code, strict.Body.String())
	}
	// A non-member gets no readable view either.
	if w := membership("?readable=1", t7Outside, t7Room); w.Code != http.StatusForbidden {
		t.Fatalf("readable membership for a non-member: want 403, got %d (%s)", w.Code, w.Body.String())
	}
}
