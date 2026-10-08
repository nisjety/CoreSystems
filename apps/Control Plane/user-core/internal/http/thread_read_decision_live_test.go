package http

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// D12: the handler-level proof that a personal Space's owner can obtain a
// Control `model.thread.read` decision for the schedule threads inside their own
// Space, and that nobody else can.
//
// A schedule's result thread is owned by `service:orchestrator-core`, so the
// owner-bound read can never reach it, and session-core refuses an org peer's
// read of it (it is one person's content, not the org's). The only way in is
// this decision. `issueThreadReadDecision` first tries the shared-room resolver,
// which by construction refuses a personal Space, and falls back to the narrow
// own-scheduled-thread resolver. The `spaces` package has unit tests for each
// half; this runs the real handler against the real repository and real SQL.
//
// Gated on TEST_DATABASE_URL (a disposable Postgres carrying the user-core
// migrations), like the other live tests in this package.

const (
	d12Org      = "d12-org"
	d12Owner    = "d12-owner"
	d12Peer     = "d12-peer"
	d12Personal = "d12-space-personal"
	d12Room     = "d12-space-room"
)

func d12Pool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live thread-read decision test")
	}
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	return pool
}

// d12Seed registers a personal Space (owned by d12Owner) and a room, makes two
// org members, and provisions the org's effect policy. threadReadEntitled is
// the one switch the own-scheduled-thread read depends on.
func d12Seed(t *testing.T, pool *pgxpool.Pool, repo *spaces.Repository, threadReadEntitled bool) {
	t.Helper()
	ctx := context.Background()
	cleanup := func() {
		for _, q := range []string{
			`DELETE FROM space_recipient_audience_members WHERE space_ref IN ('` + d12Personal + `','` + d12Room + `')`,
			`DELETE FROM space_recipient_audiences WHERE space_ref IN ('` + d12Personal + `','` + d12Room + `')`,
			`DELETE FROM space_memberships WHERE space_ref IN ('` + d12Personal + `','` + d12Room + `')`,
			`DELETE FROM space_authority_revisions WHERE space_ref IN ('` + d12Personal + `','` + d12Room + `')`,
			`DELETE FROM registered_spaces WHERE space_ref IN ('` + d12Personal + `','` + d12Room + `')`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + d12Org + `'`,
			`DELETE FROM users WHERE id IN ('` + d12Owner + `','` + d12Peer + `')`,
			`DELETE FROM space_effect_policies WHERE org_id = '` + d12Org + `'`,
		} {
			_, _ = pool.Exec(ctx, q)
		}
	}
	cleanup()
	t.Cleanup(cleanup)

	for _, space := range []struct{ ref, kind, owner string }{
		{d12Personal, "personal", d12Owner},
		{d12Room, "room", d12Owner},
	} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
			    application_lifecycle_revision, registration_state)
			VALUES ($1,$2,$3,$4,1,'active')`, space.ref, d12Org, space.kind, space.owner); err != nil {
			t.Fatalf("seed Space %s: %v", space.ref, err)
		}
		if _, err := pool.Exec(ctx,
			`INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, space.ref); err != nil {
			t.Fatalf("seed revisions %s: %v", space.ref, err)
		}
	}
	// The owner owns both; the peer is an ordinary org member with no
	// membership in the personal Space, and a plain room member.
	for _, member := range []struct{ space, subject, role string }{
		{d12Personal, d12Owner, "owner"},
		{d12Room, d12Owner, "owner"},
		{d12Room, d12Peer, "viewer"},
	} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
			VALUES ($1,'user',$2,$3,'d12-test')`, member.space, member.subject, member.role); err != nil {
			t.Fatalf("seed membership %s/%s: %v", member.space, member.subject, err)
		}
	}
	for _, user := range []string{d12Owner, d12Peer} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO users (id, email, name) VALUES ($1, $1 || '@d12.invalid', $1)`, user); err != nil {
			t.Fatalf("seed user %s: %v", user, err)
		}
		if _, err := pool.Exec(ctx, `
			INSERT INTO user_org_memberships (user_id, org_id, role, status)
			VALUES ($1,$2,'member','active')`, user, d12Org); err != nil {
			t.Fatalf("seed org membership %s: %v", user, err)
		}
	}
	if _, err := repo.UpsertEffectPolicy(ctx, spaces.EffectPolicy{
		OrgID: d12Org, PrivacyPolicyRef: "privacy:d12-org:1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
		ThreadReadEntitled: threadReadEntitled,
	}); err != nil {
		t.Fatalf("seed effect policy: %v", err)
	}
}

// d12SigningEnv installs a throwaway Ed25519 signing key the way the deployment
// does and returns the matching public key.
func d12SigningEnv(t *testing.T) ed25519.PublicKey {
	t.Helper()
	seed := bytes.Repeat([]byte{0x2b}, ed25519.SeedSize)
	t.Setenv("CONTROL_SPACE_DECISION_KEY_ID", "d12-test-key")
	t.Setenv("CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64", base64.RawURLEncoding.EncodeToString(seed))
	return ed25519.NewKeyFromSeed(seed).Public().(ed25519.PublicKey)
}

// d12ReadDecision drives the real handler as `user` in `org` for `spaceRef`.
func d12ReadDecision(t *testing.T, s *Server, org, user, spaceRef string) *httptest.ResponseRecorder {
	t.Helper()
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	body, err := json.Marshal(map[string]string{"space_ref": spaceRef, "idempotency_key": "d12-read-1"})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	c.Request = httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/thread-read-decision", bytes.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	c.Set("org_id", org)
	c.Set("user_id", user)
	s.issueThreadReadDecision(c)
	return w
}

type d12DecisionBody struct {
	Data struct {
		Decision spaces.Decision `json:"decision"`
		Token    string          `json:"token"`
	} `json:"data"`
}

// d12VerifyToken checks the signature the way a Model Plane verifier would and
// returns the signed decision, so the test asserts on what is actually signed
// rather than on the envelope next to it.
func d12VerifyToken(t *testing.T, token string, public ed25519.PublicKey) spaces.Decision {
	t.Helper()
	parts := strings.Split(token, ".")
	if len(parts) != 4 {
		t.Fatalf("token has %d parts, want 4", len(parts))
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	if !ed25519.Verify(public, []byte(strings.Join(parts[:3], ".")), signature) {
		t.Fatal("the decision's signature does not verify against the configured key")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	var decision spaces.Decision
	if err := json.Unmarshal(payload, &decision); err != nil {
		t.Fatalf("decode decision: %v", err)
	}
	return decision
}

func TestIssueThreadReadDecisionPersonalOwnerGetsAnOwnScheduledThreadReadLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12Seed(t, pool, repo, true)
	public := d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	w := d12ReadDecision(t, s, d12Org, d12Owner, d12Personal)
	if w.Code != http.StatusOK {
		t.Fatalf("personal owner: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var response d12DecisionBody
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	decision := d12VerifyToken(t, response.Data.Token, public)
	if decision.SubjectID != d12Owner || decision.SpaceRef != d12Personal || decision.OrgID != d12Org {
		t.Fatalf("decision bound to (%s, %s, %s), want the owner in their own personal Space",
			decision.OrgID, decision.SpaceRef, decision.SubjectID)
	}
	if decision.ActionID != "model.thread.read" || decision.ServiceAudience != "model-plane" {
		t.Fatalf("decision action/audience = %s/%s, want the thread read Session Core verifies",
			decision.ActionID, decision.ServiceAudience)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "thread:read" {
		t.Fatalf("permissions = %v, want exactly [thread:read]; a read decision must never carry a write", decision.Permissions)
	}
	if decision.RecipientAudienceRevision == 0 || decision.DecisionRef == "" || decision.Nonce == "" {
		t.Fatalf("decision lacks the audience revision, reference or nonce Session Core requires: %+v", decision)
	}
	if response.Data.Decision.DecisionRef != decision.DecisionRef {
		t.Fatalf("the envelope's decision_ref %q differs from the signed one %q",
			response.Data.Decision.DecisionRef, decision.DecisionRef)
	}
}

func TestIssueThreadReadDecisionRefusesAnOrgPeerForAPersonalSpaceLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12Seed(t, pool, repo, true)
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	// A second member of the same org, who is not in the owner's personal Space.
	w := d12ReadDecision(t, s, d12Org, d12Peer, d12Personal)
	if w.Code != http.StatusForbidden {
		t.Fatalf("org peer on a personal Space: want 403, got %d (%s)", w.Code, w.Body.String())
	}
	if strings.Contains(w.Body.String(), `"token"`) {
		t.Fatalf("a refusal must carry no token: %s", w.Body.String())
	}
	// Another org entirely, even for the owner's own subject id.
	w = d12ReadDecision(t, s, "d12-other-org", d12Owner, d12Personal)
	if w.Code != http.StatusForbidden {
		t.Fatalf("wrong org: want 403, got %d (%s)", w.Code, w.Body.String())
	}
}

// The own-scheduled read depends on the org's thread_read_entitled floor, which
// is deny-by-default. Without it the owner is refused too, which is the state
// any org is in until an operator provisions that column.
func TestIssueThreadReadDecisionNeedsTheOrgsThreadReadEntitlementLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12Seed(t, pool, repo, false)
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	w := d12ReadDecision(t, s, d12Org, d12Owner, d12Personal)
	if w.Code != http.StatusForbidden {
		t.Fatalf("owner without the org entitlement: want 403, got %d (%s)", w.Code, w.Body.String())
	}
}

// The fallback must not have widened the shared-room path: a room's current
// recipient still gets the shared read decision, a room member who is not in
// the current audience still does not, and the personal-only fallback never
// answers for a room.
func TestIssueThreadReadDecisionSharedRoomPathIsUnchangedLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12Seed(t, pool, repo, true)
	public := d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)
	ctx := context.Background()

	// Room membership alone does not make a reader: the shared resolver joins the
	// Space's CURRENT recipient audience, which this seed has not published yet.
	// The owner is a room owner but not an audience member, and the fallback is
	// personal-only, so neither path resolves.
	if w := d12ReadDecision(t, s, d12Org, d12Peer, d12Room); w.Code != http.StatusForbidden {
		t.Fatalf("room member outside the recipient audience: want 403, got %d (%s)", w.Code, w.Body.String())
	}

	if _, err := pool.Exec(ctx, `
		INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:d12-space-room:1', 'sha256:d12-audience', 1)`, d12Room); err != nil {
		t.Fatalf("seed recipient audience: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id)
		VALUES ($1, 1, $2)`, d12Room, d12Peer); err != nil {
		t.Fatalf("seed recipient audience member: %v", err)
	}

	w := d12ReadDecision(t, s, d12Org, d12Peer, d12Room)
	if w.Code != http.StatusOK {
		t.Fatalf("current room recipient: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var response d12DecisionBody
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	decision := d12VerifyToken(t, response.Data.Token, public)
	if decision.SubjectID != d12Peer || decision.SpaceRef != d12Room || decision.ActionID != "model.thread.read" {
		t.Fatalf("room decision bound to (%s, %s, %s)", decision.SubjectID, decision.SpaceRef, decision.ActionID)
	}
}
