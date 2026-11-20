package http

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// W3-FIX-12 and WS18-S2 at the handlers. The view_version guard needs no database.
// The canvas floor, the failed floor reads and the issued view tokens run the real
// handlers against a real repository (TEST_DATABASE_URL via d12Pool, skipped
// without it); the Session Core run binding is read from the resolved evidence.

// Golden registry hashes (internal/spaces/registry/space-self-api-registry.json).
const (
	ticketsCreateSchemaHash = "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df"
	canvasReadSchemaHash    = "sha256:7760a5a5ce90f1c50ad52b3bb68cfcb409a0d348b55eb2f066cd92e85474cac1"
	canvasWriteSchemaHash   = "sha256:9f0b5e21a35e9daf7b00b5cda023b71b598b169f28c4e112208f73b66a7d19e0"
)

// Only view_version 0, 1 and 2 exist; any other version is a 400 before any lookup.
func TestModelActionViewRejectsUnknownViewVersions(t *testing.T) {
	s := &Server{}
	for version, want := range map[string]int{
		"3":  http.StatusBadRequest,
		"-1": http.StatusBadRequest,
		// The known versions pass the guard and stop at the unwired authority.
		"0": http.StatusServiceUnavailable,
		"1": http.StatusServiceUnavailable,
		"2": http.StatusServiceUnavailable,
	} {
		c, w := newGinJSONCtx(`{"run_id":"run-1","org_id":"org-1","view_version":` + version + `}`)
		s.issueModelActionView(c)
		if w.Code != want {
			t.Errorf("view_version %s: status %d, want %d (%s)", version, w.Code, want, w.Body.String())
		}
	}
}

// failHistoryModeRead fails the query the canvas floor read starts with (the
// history_mode read in spaces/history_window.go) by handing it a cancelled
// context. Every other query on the pool, the evidence resolvers' included, runs.
type failHistoryModeRead struct{}

func (failHistoryModeRead) TraceQueryStart(ctx context.Context, _ *pgx.Conn, data pgx.TraceQueryStartData) context.Context {
	if !strings.Contains(data.SQL, "SELECT history_mode FROM registered_spaces") {
		return ctx
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	return cancelled
}

func (failHistoryModeRead) TraceQueryEnd(context.Context, *pgx.Conn, pgx.TraceQueryEndData) {}

// failingFloorReadRepo is a live repository, history window switch on, whose
// canvas floor read always fails.
func failingFloorReadRepo(t *testing.T) *spaces.Repository {
	t.Helper()
	config, err := pgxpool.ParseConfig(os.Getenv("TEST_DATABASE_URL"))
	if err != nil {
		t.Fatalf("parse TEST_DATABASE_URL: %v", err)
	}
	config.ConnConfig.Tracer = failHistoryModeRead{}
	pool, err := pgxpool.NewWithConfig(context.Background(), config)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	repo := newLiveSpaceRepo(pool)
	repo.SetHistoryWindowClaim(true)
	return repo
}

// canvasRunActionRoom seeds a W2 room whose org has agent_action, thread_create and
// canvas on, with full history. Audience revision 2 leaves the editor out and
// revision 3 brings them back, so once the room is since_join the editor's floor is
// 3; the owner, manager and viewer are in every snapshot. The allowlist names
// tickets.create and both canvas rows; the returned repository has the history
// window switch on.
func canvasRunActionRoom(t *testing.T, prefix string) (w2Room, *spaces.Repository, func(string, ...any), ed25519.PublicKey) {
	t.Helper()
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	repo.SetHistoryWindowClaim(true)
	publicKey := d12SigningEnv(t)
	room := newW2Room(prefix)
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) {
		p.AgentActionEntitled, p.ThreadCreateEntitled, p.CanvasEntitled = true, true, true
	})
	exec := func(query string, args ...any) {
		t.Helper()
		if _, err := pool.Exec(context.Background(), query, args...); err != nil {
			t.Fatalf("%s: %v", query, err)
		}
	}
	for _, snapshot := range []struct {
		revision int64
		members  []string
	}{
		{2, []string{room.Owner, room.Manager, room.Viewer}},
		{3, []string{room.Owner, room.Manager, room.Editor, room.Viewer}},
	} {
		// The ref and hash are built here so each SQL parameter has a single type.
		exec(`INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
			VALUES ($1, $2, $3, $4, $5)`, room.Room, snapshot.revision,
			fmt.Sprintf("audience:%s:%d", room.Room, snapshot.revision), fmt.Sprintf("sha256:%s-audience-%d", room.Prefix, snapshot.revision),
			len(snapshot.members))
		for _, member := range snapshot.members {
			exec(`INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, $2, $3)`, room.Room, snapshot.revision, member)
		}
	}
	exec(`UPDATE space_authority_revisions SET recipient_audience_revision=3 WHERE space_ref=$1`, room.Room)
	t.Setenv("CONTROL_SPACE_SELF_API_ACTIONS", "tickets.create,space.canvas.read,space.canvas.write")
	return room, repo, exec, publicKey
}

// runActionServer serves the run-action handlers from repo with a fixed run binding.
func runActionServer(repo *spaces.Repository, authority spaces.RunActionAuthority) *Server {
	s := &Server{}
	s.SetSpaceRepository(repo)
	s.SetRunActionAuthorityResolver(fakeRunAuthority{authority: authority})
	return s
}

func postRunActionDecision(s *Server, org, actionID, schemaHash string) *httptest.ResponseRecorder {
	body, _ := json.Marshal(map[string]string{
		"run_id": "run-1", "org_id": org, "action_id": actionID, "action_schema_hash": schemaHash,
		"payload_digest": "sha256:" + strings.Repeat("b", 64), "idempotency_key": "run-1:" + actionID + ":1",
	})
	c, w := newGinJSONCtx(string(body))
	s.issueRunActionDecision(c)
	return w
}

func issuedRunActionDecision(t *testing.T, w *httptest.ResponseRecorder) spaces.RunActionDecision {
	t.Helper()
	if w.Code != http.StatusOK {
		t.Fatalf("run action decision: %d %s", w.Code, w.Body.String())
	}
	var body struct {
		Data struct {
			Decision spaces.RunActionDecision `json:"decision"`
			Token    string                   `json:"token"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil || body.Data.Token == "" {
		t.Fatalf("run action decision body: %v %s", err, w.Body.String())
	}
	return body.Data.Decision
}

func postRunActionCheck(t *testing.T, s *Server, decision spaces.RunActionDecision) *httptest.ResponseRecorder {
	t.Helper()
	body, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("marshal decision: %v", err)
	}
	c, w := newGinJSONCtx(string(body))
	s.checkCurrentRunActionAuthority(c)
	return w
}

func postModelActionView(t *testing.T, s *Server, org string, version int) string {
	t.Helper()
	c, w := newGinJSONCtx(fmt.Sprintf(`{"run_id":"run-1","org_id":%q,"view_version":%d}`, org, version))
	s.issueModelActionView(c)
	if w.Code != http.StatusOK {
		t.Fatalf("view_version %d: %d %s", version, w.Code, w.Body.String())
	}
	var body struct {
		Data struct {
			Token string `json:"token"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil || body.Data.Token == "" {
		t.Fatalf("view_version %d body: %v %s", version, err, w.Body.String())
	}
	return body.Data.Token
}

// verifiedModelActionView checks a view token as its recipient does: the envelope
// version, Control's key id and signature, then a strict decode of the claims.
func verifiedModelActionView(t *testing.T, publicKey ed25519.PublicKey, version, token string, claims any) {
	t.Helper()
	parts := strings.Split(token, ".")
	if len(parts) != 4 || parts[0] != version {
		t.Fatalf("token %q does not use the %s envelope", token, version)
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != "d12-test-key" {
		t.Fatalf("token key id %q (%v)", keyID, err)
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(publicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		t.Fatalf("token signature does not verify (%v)", err)
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("token payload: %v", err)
	}
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(claims); err != nil {
		t.Fatalf("token claims: %v", err)
	}
}

func viewV2ActionIDs(view spaces.ModelActionViewV2) []string {
	ids := make([]string, 0, len(view.Actions))
	for _, action := range view.Actions {
		ids = append(ids, action.ActionID)
	}
	return ids
}

// W3-FIX-12: once the room is since_join, a canvas run action for a member with a
// floor is refused with the typed space_history_floor_unsupported envelope, at
// issuance and at the effect-time check. tickets.create and a member with no floor
// are unaffected.
func TestRunActionCanvasFloorRefusalIsTypedLive(t *testing.T) {
	room, repo, exec, _ := canvasRunActionRoom(t, "w3fix12f")
	editor := runActionServer(repo, liveRunAuthority(t, repo, room, room.Editor))
	// Full history: the editor's canvas read issues.
	decision := issuedRunActionDecision(t, postRunActionDecision(editor, room.Org, "space.canvas.read", canvasReadSchemaHash))

	exec(`UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, room.Room)
	for actionID, hash := range map[string]string{"space.canvas.read": canvasReadSchemaHash, "space.canvas.write": canvasWriteSchemaHash} {
		wantApprovalError(t, postRunActionDecision(editor, room.Org, actionID, hash), http.StatusForbidden, spaces.CanvasHistoryFloorCode)
	}
	wantApprovalError(t, postRunActionCheck(t, editor, decision), http.StatusForbidden, spaces.CanvasHistoryFloorCode)
	issuedRunActionDecision(t, postRunActionDecision(editor, room.Org, "tickets.create", ticketsCreateSchemaHash))

	owner := runActionServer(repo, liveRunAuthority(t, repo, room, room.Owner))
	issuedRunActionDecision(t, postRunActionDecision(owner, room.Org, "space.canvas.read", canvasReadSchemaHash))
}

// D22ar (pure tier): the typed archived-room answer is for canvas rows only, and a
// failed archive lookup never writes it; the caller then keeps the generic 403.
func TestRefuseArchivedCanvasRunActionOnlyTypesACanvasRowItCouldCheck(t *testing.T) {
	s := &Server{} // no repository: ArchivedForMember answers an error
	for _, actionID := range []string{"", "tickets.create", "space.canvas.read", "space.canvas.write"} {
		c, w := newGinJSONCtx(`{}`)
		if s.refuseArchivedCanvasRunAction(c, actionID, "room-1", "org-1", "user-1") {
			t.Fatalf("%q: wrote a refusal without a successful archive lookup: %d %s", actionID, w.Code, w.Body.String())
		}
		if w.Body.Len() != 0 {
			t.Fatalf("%q: wrote %s", actionID, w.Body.String())
		}
	}
}

// D22ar: once the room is archived, a canvas run action (read or write) answers the
// typed 409 room_archived with no token, at issuance and at the effect-time check,
// the shape thread writes get. tickets.create keeps the unchanged untyped 403, and a
// subject who is not a member of the archived room learns nothing: the same 403.
func TestRunActionCanvasArchivedRoomRefusalIsTypedLive(t *testing.T) {
	room, repo, exec, _ := canvasRunActionRoom(t, "d22ar")
	editor := runActionServer(repo, liveRunAuthority(t, repo, room, room.Editor))
	decision := issuedRunActionDecision(t, postRunActionDecision(editor, room.Org, "space.canvas.read", canvasReadSchemaHash))
	// The run binding is read while the room is still active; only its subject differs.
	outsider := liveRunAuthority(t, repo, room, room.Editor)
	outsider.SubjectID = room.Prefix + "-stranger"

	exec(`UPDATE registered_spaces SET registration_state='archived', archived_at=NOW(), archived_by=$2 WHERE space_ref=$1`, room.Room, room.Owner)
	for actionID, hash := range map[string]string{"space.canvas.read": canvasReadSchemaHash, "space.canvas.write": canvasWriteSchemaHash} {
		w := postRunActionDecision(editor, room.Org, actionID, hash)
		wantApprovalError(t, w, http.StatusConflict, spaces.CodeRoomArchived)
		if strings.Contains(w.Body.String(), `"token"`) {
			t.Fatalf("%s: an archived-room refusal carried a token: %s", actionID, w.Body.String())
		}
	}
	wantApprovalError(t, postRunActionCheck(t, editor, decision), http.StatusConflict, spaces.CodeRoomArchived)

	// A non-canvas row keeps the generic refusal, byte for byte.
	if w := postRunActionDecision(editor, room.Org, "tickets.create", ticketsCreateSchemaHash); w.Code != http.StatusForbidden ||
		w.Body.String() != `{"error":"current Space authority required"}` {
		t.Fatalf("archived-room tickets.create: %d %s", w.Code, w.Body.String())
	}
	// A subject with no membership in the room is not told it is archived.
	if w := postRunActionDecision(runActionServer(repo, outsider), room.Org, "space.canvas.read", canvasReadSchemaHash); w.Code != http.StatusForbidden ||
		w.Body.String() != `{"error":"current Space authority required"}` {
		t.Fatalf("archived-room canvas read by a non-member: %d %s", w.Code, w.Body.String())
	}
}

// W3-FIX-12: a floor read that fails answers 503 for a canvas run action, on issue
// and on the effect-time check; it never issues or authorizes without the floor.
func TestRunActionCanvasFloorReadFailureFailsClosedLive(t *testing.T) {
	room, repo, _, _ := canvasRunActionRoom(t, "w3fix12g")
	authority := liveRunAuthority(t, repo, room, room.Owner)
	decision := issuedRunActionDecision(t, postRunActionDecision(runActionServer(repo, authority), room.Org, "space.canvas.read", canvasReadSchemaHash))

	broken := runActionServer(failingFloorReadRepo(t), authority)
	if w := postRunActionDecision(broken, room.Org, "space.canvas.read", canvasReadSchemaHash); w.Code != http.StatusServiceUnavailable {
		t.Fatalf("issue with a failed floor read: %d %s", w.Code, w.Body.String())
	}
	if w := postRunActionCheck(t, broken, decision); w.Code != http.StatusServiceUnavailable {
		t.Fatalf("effect-time check with a failed floor read: %d %s", w.Code, w.Body.String())
	}
	// tickets.create reads no floor, so it still issues through the same repository.
	issuedRunActionDecision(t, postRunActionDecision(broken, room.Org, "tickets.create", ticketsCreateSchemaHash))
	if w := postRunActionCheck(t, runActionServer(repo, authority), decision); w.Code != http.StatusOK {
		t.Fatalf("effect-time check with a working floor read: %d %s", w.Code, w.Body.String())
	}
}

// W3-FIX-12: with a failed floor read the v2 view omits both canvas rows and still
// offers tickets.create.
func TestModelActionViewV2FloorReadFailureOmitsCanvasRowsLive(t *testing.T) {
	room, repo, _, publicKey := canvasRunActionRoom(t, "w3fix12h")
	authority := liveRunAuthority(t, repo, room, room.Owner)
	for name, test := range map[string]struct {
		repo *spaces.Repository
		want []string
	}{
		"floor read":        {repo, []string{"space.canvas.read", "space.canvas.write", "tickets.create"}},
		"failed floor read": {failingFloorReadRepo(t), []string{"tickets.create"}},
	} {
		var view spaces.ModelActionViewV2
		verifiedModelActionView(t, publicKey, "model-action-view-v2", postModelActionView(t, runActionServer(test.repo, authority), room.Org, 2), &view)
		if err := view.Validate(); err != nil {
			t.Fatalf("%s: v2 claims do not validate: %v", name, err)
		}
		if got := viewV2ActionIDs(view); !slices.Equal(got, test.want) {
			t.Fatalf("%s: v2 view lists %v, want %v", name, got, test.want)
		}
	}
}

// WS18-S2: view_version 0 and 1 answer the v1 tickets.create view and 2 the v2
// list; each token verifies under Control's key and its claims pass the view's own
// Validate.
func TestModelActionViewVersionTokensValidateLive(t *testing.T) {
	room, repo, _, publicKey := canvasRunActionRoom(t, "ws18s2v")
	s := runActionServer(repo, liveRunAuthority(t, repo, room, room.Editor))
	for _, version := range []int{0, 1} {
		var view spaces.ModelActionView
		verifiedModelActionView(t, publicKey, "model-action-view-v1", postModelActionView(t, s, room.Org, version), &view)
		if err := view.Validate(); err != nil || view.ActionID != "tickets.create" || view.SubjectID != room.Editor || view.SpaceRef != room.Room {
			t.Fatalf("view_version %d claims = %+v (%v)", version, view, err)
		}
	}
	var view spaces.ModelActionViewV2
	verifiedModelActionView(t, publicKey, "model-action-view-v2", postModelActionView(t, s, room.Org, 2), &view)
	if err := view.Validate(); err != nil || view.SubjectID != room.Editor || view.SpaceRef != room.Room {
		t.Fatalf("view_version 2 claims = %+v (%v)", view, err)
	}
	if got := viewV2ActionIDs(view); !slices.Equal(got, []string{"space.canvas.read", "space.canvas.write", "tickets.create"}) {
		t.Fatalf("view_version 2 lists %v", got)
	}
}
