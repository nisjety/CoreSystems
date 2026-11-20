package handler

// Route tests for the room canvas (WS19-S3 and WS19-S9 handler entries). The helpers in this file are shared by the canvas test
// files of this package: they mint the bearers, sign the Control decisions with a fixed test key, build the router and send one
// request. The routes run the production handlers. A route that reaches the canvas store is built over a store with no pool, so a
// request that gets past its refusals panics there, and serveCanvas reports that as a failure. A refusal decided after the not-found
// probe (D21n) is tested over fakeCanvasStore, which answers the probe and records any store call past it. Tests that need a live
// store are gated on WIKI_TEST_DATABASE_URL.

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

const (
	canvasTestOrg      = "org-authorized"
	canvasTestUser     = "user-authorized"
	canvasTestSpace    = "space-room-1"
	canvasTestRunID    = "run-canvas-1"
	canvasTestKeyID    = "control-space-decision-test"
	canvasTestIssuer   = "https://auth.test/issuer"
	canvasTestAudience = "data-plane"
	canvasSpacePath    = "/v1/space-canvas/spaces/" + canvasTestSpace
)

var (
	canvasSignerOnce sync.Once
	canvasSignerKey  *rsa.PrivateKey
	canvasSignerErr  error
)

// canvasTestSigner returns the RSA key that signs the bearers of these tests and the verifier that trusts it. The key is generated
// once per test binary.
func canvasTestSigner(t *testing.T) (*rsa.PrivateKey, *authctx.Verifier) {
	t.Helper()
	canvasSignerOnce.Do(func() {
		canvasSignerKey, canvasSignerErr = rsa.GenerateKey(rand.Reader, 2048)
	})
	if canvasSignerErr != nil {
		t.Fatalf("generate RSA key: %v", canvasSignerErr)
	}
	encoded, err := x509.MarshalPKIXPublicKey(&canvasSignerKey.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience: canvasTestAudience, Issuer: canvasTestIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: encoded}),
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	return canvasSignerKey, verifier
}

// canvasBearer mints a user bearer for org with the given scopes and zero-retention posture.
func canvasBearer(t *testing.T, org string, scopes []string, zdr bool) string {
	t.Helper()
	return canvasSignBearer(t, jwt.MapClaims{
		"sub": canvasTestUser, "user_id": canvasTestUser, "org_id": org,
		"scopes": scopes, "zdr": zdr,
	})
}

// canvasServiceBearer mints a service bearer, the form Control's Space-deletion coordinator and other service principals present.
func canvasServiceBearer(t *testing.T, serviceID, org string, scopes []string) string {
	t.Helper()
	return canvasSignBearer(t, jwt.MapClaims{
		"sub": serviceID, "service_id": serviceID, "principal_type": "service", "org_id": org,
		"scopes": scopes, "zdr": false,
	})
}

// canvasSignBearer signs claims with the test key and adds the issuer, audience and validity window the verifier checks.
func canvasSignBearer(t *testing.T, claims jwt.MapClaims) string {
	t.Helper()
	key, _ := canvasTestSigner(t)
	now := time.Now()
	claims["iss"] = canvasTestIssuer
	claims["aud"] = canvasTestAudience
	claims["iat"] = now.Add(-time.Minute).Unix()
	claims["nbf"] = now.Add(-time.Minute).Unix()
	claims["exp"] = now.Add(time.Hour).Unix()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign canvas bearer: %v", err)
	}
	return token
}

// canvasDecisionKey is the Ed25519 key that signs the Control decisions of these tests. It comes from a fixed seed, so the test
// writes no key material.
func canvasDecisionKey() ed25519.PrivateKey {
	seed := sha256.Sum256([]byte("wiki-store canvas handler tests: Control decision key"))
	return ed25519.NewKeyFromSeed(seed[:])
}

// canvasDecisionKeys is the key set the canvas handler verifies decisions with.
func canvasDecisionKeys() spacedecision.KeySet {
	return spacedecision.KeySet{canvasTestKeyID: canvasDecisionKey().Public().(ed25519.PublicKey)}
}

// signCanvasEnvelope signs a decision the way Control does: version.key-id.payload.signature, with an Ed25519 signature over the
// first three parts and the key id and payload in unpadded base64url.
func signCanvasEnvelope(t *testing.T, version string, payload any) string {
	t.Helper()
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("encode decision: %v", err)
	}
	signingInput := version + "." + base64.RawURLEncoding.EncodeToString([]byte(canvasTestKeyID)) + "." +
		base64.RawURLEncoding.EncodeToString(body)
	signature := ed25519.Sign(canvasDecisionKey(), []byte(signingInput))
	return signingInput + "." + base64.RawURLEncoding.EncodeToString(signature)
}

// canvasHumanClaims is a complete, unexpired human decision for one canvas operation on a Space. The caller sets the payload digest.
func canvasHumanClaims(org, space, idempotencyKey string, write bool) spacedecision.Claims {
	action, schema, permission := spacedecision.ActionCanvasRead, spacedecision.SchemaCanvasRead, spacedecision.PermissionCanvasRead
	if write {
		action, schema, permission = spacedecision.ActionCanvasWrite, spacedecision.SchemaCanvasWrite, spacedecision.PermissionCanvasWrite
	}
	now := time.Now().UTC()
	return spacedecision.Claims{
		DecisionRef:               "decision-" + idempotencyKey,
		OrgID:                     org,
		SpaceRef:                  space,
		SubjectID:                 canvasTestUser,
		ServiceAudience:           spacedecision.AudienceCanvas,
		ActionID:                  action,
		ActionSchemaHash:          schema,
		IdempotencyKey:            idempotencyKey,
		RecipientAudienceRef:      "audience:" + space + ":1",
		RecipientAudienceHash:     "sha256:canvas-test-audience",
		PrivacyPolicyRef:          "privacy:" + org + ":1",
		ResourceAuthorizationRef:  "control:" + space + ":canvas:1",
		AuthorityRevision:         1,
		MembershipRevision:        1,
		PrivacyRevision:           1,
		RecipientAudienceRevision: 1,
		EntitlementRevision:       1,
		Permissions:               []string{permission},
		Purpose:                   "room_canvas",
		LawfulBasis:               "contract",
		PrivacyClass:              "room",
		RetentionClass:            "room",
		Residency:                 "eu",
		DeletionScope:             "space",
		Nonce:                     "nonce-" + idempotencyKey,
		SpaceKind:                 "room",
		IssuedAt:                  now.Add(-10 * time.Second),
		ExpiresAt:                 now.Add(2 * time.Minute),
	}
}

// canvasReadOp is the operation a read decision signs: no canvas, no head and no content.
func canvasReadOp(idempotencyKey string) spacedecision.CanvasOp {
	return spacedecision.CanvasOp{Op: spacedecision.OpRead, Schema: spacedecision.SchemaCanvasRead, IdempotencyKey: idempotencyKey}
}

// canvasHumanToken signs a human decision whose payload digest is the one the handler recomputes for op.
func canvasHumanToken(t *testing.T, claims spacedecision.Claims, op spacedecision.CanvasOp) string {
	t.Helper()
	claims.PayloadDigest = spacedecision.CanvasPayloadDigest(claims, op)
	return signCanvasEnvelope(t, spacedecision.EnvelopeVersion, claims)
}

// canvasWrongDigest is a well-formed digest that no request computes, so a decision that carries it can only be refused on its digest.
func canvasWrongDigest() string {
	return "sha256:" + strings.Repeat("0", 64)
}

// canvasRunClaims is a complete, unexpired agent decision for one canvas call of the test run. args are the arguments the request
// carries (nil for a read). The payload digest is the one the handler recomputes from them. The caller may edit the claims and sign.
func canvasRunClaims(org, space, idempotencyKey string, write bool, args []spacedecision.RunArgument) spacedecision.RunClaims {
	action, schema, permission := spacedecision.ActionRunCanvasRead, spacedecision.SchemaRunCanvasRead, spacedecision.PermissionCanvasRead
	if write {
		action, schema, permission = spacedecision.ActionRunCanvasWrite, spacedecision.SchemaRunCanvasWrite, spacedecision.PermissionCanvasWrite
	}
	now := time.Now().UTC()
	run := spacedecision.RunClaims{
		DecisionRef:                "decision-run-" + idempotencyKey,
		RunID:                      canvasTestRunID,
		ThreadID:                   "thread-canvas-1",
		OrgID:                      org,
		SpaceRef:                   space,
		SubjectID:                  canvasTestUser,
		ServiceAudience:            spacedecision.AudienceCanvas,
		ActionID:                   action,
		ActionSchemaHash:           schema,
		IdempotencyKey:             idempotencyKey,
		RecipientAudienceRef:       "audience:" + space + ":1",
		RecipientAudienceHash:      "sha256:canvas-test-audience",
		RecipientAudienceRevision:  1,
		PrivacyPolicyRef:           "privacy:" + org + ":1",
		RunContextAuthorizationRef: "control:" + canvasTestRunID + ":context:1",
		AuthorityRevision:          1,
		Permissions:                []string{permission},
		Purpose:                    "room_canvas",
		LawfulBasis:                "contract",
		PrivacyClass:               "room",
		RetentionClass:             "room",
		Residency:                  "eu",
		DeletionScope:              "space",
		Nonce:                      "nonce-run-" + idempotencyKey,
		IssuedAt:                   now.Add(-10 * time.Second),
		ExpiresAt:                  now.Add(2 * time.Minute),
	}
	run.PayloadDigest = spacedecision.RunPayloadDigest(action, run.RunID, run.OrgID, run.IdempotencyKey, args)
	return run
}

// canvasRouter mounts the room canvas routes over repo with the canvas flag as given.
func canvasRouter(t *testing.T, repo *canvas.Repo, enabled bool) http.Handler {
	t.Helper()
	_, verifier := canvasTestSigner(t)
	router := chi.NewRouter()
	MountCanvasRoutes(router, authctx.Middleware(verifier), NewCanvasHandler(repo, enabled, canvasDecisionKeys()))
	return router
}

// The ids the fake store's canvas holds.
const (
	fakeCanvasID  = "canvas-1"
	fakeVersionID = "version-1"
)

// fakeCanvasStore answers the not-found probe of the canvas routes from fixed facts, with no database, and records every store
// call past the probe. A test that expects a refusal asserts that nothing past the probe ran. Its reads and writes answer fixed
// results, so a request that passes every refusal gets a 200 or 201.
type fakeCanvasStore struct {
	probe   canvas.Probe
	mu      sync.Mutex
	probed  int
	reached []string
	// err, when set, is what every call past the probe answers.
	err error
}

// fakeStoreWithCanvas is a store whose Space holds canvas-1 with version-1.
func fakeStoreWithCanvas() *fakeCanvasStore {
	return &fakeCanvasStore{probe: canvas.Probe{CanvasID: fakeCanvasID, VersionFound: true}}
}

func (f *fakeCanvasStore) Probe(_ context.Context, _, _, versionID string) (canvas.Probe, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.probed++
	p := f.probe
	if versionID == "" {
		p.VersionFound = false
	}
	return p, nil
}

func (f *fakeCanvasStore) record(call string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.reached = append(f.reached, call)
	return f.err
}

func (f *fakeCanvasStore) Head(_ context.Context, _, spaceRef string, _ *string, _ canvas.Floor) (canvas.HeadView, error) {
	if err := f.record("head"); err != nil {
		return canvas.HeadView{}, err
	}
	head := fakeVersionID
	v := canvas.Version{VersionID: fakeVersionID, CanvasID: fakeCanvasID, VersionNo: 1, Content: "# Room\n"}
	return canvas.HeadView{Canvas: canvas.Canvas{CanvasID: fakeCanvasID, SpaceRef: spaceRef, State: "active",
		HeadVersionID: &head, HeadVersionNo: 1}, Head: &v}, nil
}

func (f *fakeCanvasStore) ListVersions(_ context.Context, _, _ string, _ *int, _ int, _ canvas.Floor) (canvas.VersionPage, error) {
	if err := f.record("list"); err != nil {
		return canvas.VersionPage{}, err
	}
	return canvas.VersionPage{Versions: []canvas.Version{{VersionID: fakeVersionID, CanvasID: fakeCanvasID, VersionNo: 1}}}, nil
}

func (f *fakeCanvasStore) GetVersion(_ context.Context, _, _, versionID string, _ canvas.Floor) (canvas.Version, error) {
	if err := f.record("get-version"); err != nil {
		return canvas.Version{}, err
	}
	return canvas.Version{VersionID: versionID, CanvasID: fakeCanvasID, VersionNo: 1, Content: "# Room\n"}, nil
}

func (f *fakeCanvasStore) Save(_ context.Context, in canvas.SaveInput) (canvas.WriteResult, error) {
	if err := f.record("save"); err != nil {
		return canvas.WriteResult{}, err
	}
	return canvas.WriteResult{CanvasID: fakeCanvasID, Inserted: true, HeadVersionID: "version-2",
		Version: canvas.Version{VersionID: "version-2", CanvasID: fakeCanvasID, VersionNo: 2, AuthorKind: in.Author.Kind}}, nil
}

func (f *fakeCanvasStore) Restore(_ context.Context, _ canvas.RestoreInput) (canvas.WriteResult, error) {
	if err := f.record("restore"); err != nil {
		return canvas.WriteResult{}, err
	}
	return canvas.WriteResult{CanvasID: fakeCanvasID, Inserted: true, HeadVersionID: "version-2",
		Version: canvas.Version{VersionID: "version-2", CanvasID: fakeCanvasID, VersionNo: 2, ChangeKind: "restore"}}, nil
}

// wantStoreUntouched asserts that no store call past the probe ran.
func (f *fakeCanvasStore) wantStoreUntouched(t *testing.T) {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.reached) != 0 {
		t.Fatalf("the route used the store past its probe (%v) where it must refuse first", f.reached)
	}
}

// fakeCanvasRouter mounts the room canvas routes, flag on, over a fake store.
func fakeCanvasRouter(t *testing.T, store *fakeCanvasStore) http.Handler {
	t.Helper()
	_, verifier := canvasTestSigner(t)
	router := chi.NewRouter()
	MountCanvasRoutes(router, authctx.Middleware(verifier), newCanvasHandler(store, true, canvasDecisionKeys()))
	return router
}

// canvasRouteOp is the operation the handler derives for route over the fake store's canvas (canvas-1), with the route's own body,
// so a decision signed for it matches the digest the handler recomputes.
func canvasRouteOp(route canvasRoute, idempotencyKey string) spacedecision.CanvasOp {
	switch route.name {
	case "save":
		return spacedecision.CanvasOp{Op: spacedecision.OpSave, Schema: spacedecision.SchemaCanvasWrite, CanvasID: fakeCanvasID,
			ContentDigest: canvas.ContentDigest("# Room\n"), IdempotencyKey: "save-key-1"}
	case "restore":
		return spacedecision.CanvasOp{Op: spacedecision.OpRestore, Schema: spacedecision.SchemaCanvasWrite, CanvasID: fakeCanvasID,
			RestoreFromVersionID: fakeVersionID, IdempotencyKey: "restore-key-1"}
	default:
		return canvasReadOp(idempotencyKey)
	}
}

// canvasRouteHumanToken signs a human decision that is correct for route over the fake store's canvas. edit, when set, changes the
// claims before the digest is computed and the decision is signed.
func canvasRouteHumanToken(t *testing.T, route canvasRoute, key string, edit func(*spacedecision.Claims)) string {
	t.Helper()
	claims := canvasHumanClaims(canvasTestOrg, canvasTestSpace, key, route.write)
	if edit != nil {
		edit(&claims)
	}
	return canvasHumanToken(t, claims, canvasRouteOp(route, key))
}

// canvasRouteRunToken signs an agent decision that is correct for route (head read or save) with the route's own body.
func canvasRouteRunToken(t *testing.T, route canvasRoute, edit func(*spacedecision.RunClaims)) string {
	t.Helper()
	key, args := "agent-read-key-1", []spacedecision.RunArgument(nil)
	if route.write {
		key = "save-key-1"
		args = runWriteArgs(saveBody{Content: "# Room\n", IdempotencyKey: key})
	}
	run := canvasRunClaims(canvasTestOrg, canvasTestSpace, key, route.write, args)
	if edit != nil {
		edit(&run)
	}
	return signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, run)
}

// serveCanvas sends one request through router. A handler that reaches a store with no pool panics, which means it ran where its
// route must refuse first, so a panic here fails the test.
func serveCanvas(t *testing.T, router http.Handler, method, path, bearer, decision, body string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(method, path, strings.NewReader(body))
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	if decision != "" {
		req.Header.Set(canvasDecisionHeader, decision)
	}
	recorder := httptest.NewRecorder()
	func() {
		defer func() {
			if p := recover(); p != nil {
				t.Fatalf("%s %s ran a handler that needed the canvas store: %v", method, path, p)
			}
		}()
		router.ServeHTTP(recorder, req)
	}()
	return recorder
}

// canvasBody encodes a request body as JSON.
func canvasBody(t *testing.T, v any) string {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("encode request body: %v", err)
	}
	return string(raw)
}

type canvasErrorBody struct {
	Error struct {
		Code      string         `json:"code"`
		Message   string         `json:"message"`
		Retryable *bool          `json:"retryable"`
		Action    string         `json:"action"`
		Details   map[string]any `json:"details"`
	} `json:"error"`
}

// wantCanvasRefusal asserts a refused canvas request by its status and its error code.
func wantCanvasRefusal(t *testing.T, rec *httptest.ResponseRecorder, status int, code string) {
	t.Helper()
	if rec.Code != status {
		t.Fatalf("status = %d, want %d; body=%s", rec.Code, status, rec.Body.String())
	}
	var body canvasErrorBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode error body %q: %v", rec.Body.String(), err)
	}
	if body.Error.Code != code {
		t.Fatalf("error.code = %q, want %q; body=%s", body.Error.Code, code, rec.Body.String())
	}
}

// canvasWriteBody is the answer to a save or a restore.
type canvasWriteBody struct {
	Data struct {
		CanvasID     string `json:"canvas_id"`
		Deduplicated bool   `json:"deduplicated"`
		Version      struct {
			VersionID  string  `json:"version_id"`
			VersionNo  int     `json:"version_no"`
			AuthorKind string  `json:"author_kind"`
			AuthorID   string  `json:"author_id"`
			OnBehalfOf *string `json:"on_behalf_of"`
			RunID      *string `json:"run_id"`
		} `json:"version"`
	} `json:"data"`
}

func decodeCanvasWrite(t *testing.T, rec *httptest.ResponseRecorder) canvasWriteBody {
	t.Helper()
	var body canvasWriteBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode write answer %q: %v", rec.Body.String(), err)
	}
	return body
}

// canvasRoute is one room canvas route as these tests send it. A write route needs wiki.write and a decision for a write. Only the
// head read and the save accept an agent decision (GetCanvas and SaveVersion).
type canvasRoute struct {
	name, method, path string
	write, agent       bool
	body               string
}

// canvasRoutes are the five routes MountCanvasRoutes installs, each with a body a valid request for it carries.
var canvasRoutes = []canvasRoute{
	{name: "head", method: http.MethodGet, path: canvasSpacePath, agent: true},
	{name: "list versions", method: http.MethodGet, path: canvasSpacePath + "/versions"},
	{name: "get version", method: http.MethodGet, path: canvasSpacePath + "/versions/version-1"},
	{name: "save", method: http.MethodPost, path: canvasSpacePath + "/versions", write: true, agent: true,
		body: `{"content":"# Room\n","idempotency_key":"save-key-1"}`},
	{name: "restore", method: http.MethodPost, path: canvasSpacePath + "/versions/version-1/restore", write: true,
		body: `{"idempotency_key":"restore-key-1"}`},
}

// TestCanvasRoutesAnswerNotFoundWhileTheFlagIsOffBeforeAnyCredential: with WIKISTORE_SPACE_CANVAS off every canvas route answers
// 404 space_canvas_disabled. The flag gate runs before the credential is read, so a request with no bearer and one with a valid
// bearer get the same answer.
func TestCanvasRoutesAnswerNotFoundWhileTheFlagIsOffBeforeAnyCredential(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), false)
	valid := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			for _, bearer := range []string{"", valid} {
				rec := serveCanvas(t, router, route.method, route.path, bearer, "", route.body)
				wantCanvasRefusal(t, rec, http.StatusNotFound, "space_canvas_disabled")
			}
		})
	}
}

func TestCanvasRoutesRequireAVerifiedBearer(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			rec := serveCanvas(t, router, route.method, route.path, "", "", route.body)
			wantCanvasRefusal(t, rec, http.StatusUnauthorized, "unauthorized")
		})
	}
}

// TestCanvasRoutesRequireTheScopeOfTheirAction: reads need wiki.read and writes need wiki.write, so a bearer that lacks the scope
// is refused before any decision is read.
func TestCanvasRoutesRequireTheScopeOfTheirAction(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	readOnly := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	writeOnly := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	for _, route := range canvasRoutes {
		bearer := readOnly
		if !route.write {
			bearer = writeOnly
		}
		t.Run(route.name, func(t *testing.T) {
			rec := serveCanvas(t, router, route.method, route.path, bearer, "", route.body)
			wantCanvasRefusal(t, rec, http.StatusForbidden, "insufficient_scope")
		})
	}
}

// TestCanvasRoutesRequireAControlDecision: a request with no Control decision is 400 space_decision_required, once its body is valid.
func TestCanvasRoutesRequireAControlDecision(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			rec := serveCanvas(t, router, route.method, route.path, bearer, "", route.body)
			wantCanvasRefusal(t, rec, http.StatusBadRequest, "space_decision_required")
		})
	}
}

// TestCanvasSaveRefusesABodyOverTheRequestLimit: a body over the 128 KiB cap is 413 canvas_too_large, with the cap in details.
func TestCanvasSaveRefusesABodyOverTheRequestLimit(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	oversize := `{"content":"` + strings.Repeat("a", canvas.MaxRequestBodyBytes) + `","idempotency_key":"save-key-1"}`
	rec := serveCanvas(t, router, http.MethodPost, canvasSpacePath+"/versions", bearer, "", oversize)
	wantCanvasRefusal(t, rec, http.StatusRequestEntityTooLarge, canvas.CodeTooLarge)
	if got := decodeCanvasErrorBody(t, rec).Error.Details["max_bytes"]; got != float64(canvas.MaxRequestBodyBytes) {
		t.Fatalf("details.max_bytes = %v, want %d", got, canvas.MaxRequestBodyBytes)
	}
}

// TestCanvasSaveRefusesAnUnknownBodyField: a field the contract does not name, such as an org, is refused, so no authority rides in
// the body. The refusal comes before the decision is read.
func TestCanvasSaveRefusesAnUnknownBodyField(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	body := `{"content":"# Room\n","idempotency_key":"save-key-1","org_id":"org-other"}`
	rec := serveCanvas(t, router, http.MethodPost, canvasSpacePath+"/versions", bearer, "", body)
	wantCanvasRefusal(t, rec, http.StatusBadRequest, canvas.CodeInvalidRequest)
}

func decodeCanvasErrorBody(t *testing.T, rec *httptest.ResponseRecorder) canvasErrorBody {
	t.Helper()
	var body canvasErrorBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode error body %q: %v", rec.Body.String(), err)
	}
	return body
}

// TestCanvasReadsRefuseADecisionWhoseDigestIsNotTheRequest: the handler recomputes the read digest from the verified claims, and a
// digest the decision does not match is 403 space_decision_digest_mismatch. The check runs after the not-found probe and before the
// store is read.
func TestCanvasReadsRefuseADecisionWhoseDigestIsNotTheRequest(t *testing.T) {
	store := fakeStoreWithCanvas()
	router := fakeCanvasRouter(t, store)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	claims := canvasHumanClaims(canvasTestOrg, canvasTestSpace, "read-key-1", false)
	claims.PayloadDigest = canvasWrongDigest()
	decision := signCanvasEnvelope(t, spacedecision.EnvelopeVersion, claims)
	for _, route := range canvasRoutes {
		if route.write {
			continue
		}
		t.Run(route.name, func(t *testing.T) {
			rec := serveCanvas(t, router, route.method, route.path, bearer, decision, "")
			wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDigestMismatch)
		})
	}
	store.wantStoreUntouched(t)
}

// TestCanvasRoutesRefuseADecisionIssuedForAnotherSpace: a decision is bound to the Space it names, so one issued for another Space
// is 403 space_decision_mismatch on the route it arrives at.
func TestCanvasRoutesRefuseADecisionIssuedForAnotherSpace(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	claims := canvasHumanClaims(canvasTestOrg, "space-room-2", "read-key-2", false)
	decision := canvasHumanToken(t, claims, canvasReadOp("read-key-2"))
	rec := serveCanvas(t, router, http.MethodGet, canvasSpacePath, bearer, decision, "")
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDecisionMismatch)
}

// TestCanvasRoutesRefuseAServicePrincipalEvenWithADecision: a decision is issued to a user. A service bearer that holds the read scope
// is refused on the route as a decision mismatch, even with a decision that otherwise binds to the route.
func TestCanvasRoutesRefuseAServicePrincipalEvenWithADecision(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasServiceBearer(t, "service:other-plane", canvasTestOrg, []string{"wiki.read"})
	claims := canvasHumanClaims(canvasTestOrg, canvasTestSpace, "read-key-3", false)
	decision := canvasHumanToken(t, claims, canvasReadOp("read-key-3"))
	rec := serveCanvas(t, router, http.MethodGet, canvasSpacePath, bearer, decision, "")
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDecisionMismatch)
}

// TestCanvasRoutesRefuseADecisionThatIsNotControlSigned: a decision the Control key set does not verify is 403 space_decision_invalid.
// Two forgeries are tried: a payload edited after signing, and an envelope that names a key id the service does not trust.
func TestCanvasRoutesRefuseADecisionThatIsNotControlSigned(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	claims := canvasHumanClaims(canvasTestOrg, canvasTestSpace, "read-key-4", false)
	signed := strings.Split(canvasHumanToken(t, claims, canvasReadOp("read-key-4")), ".")

	claims.OrgID = "org-other"
	edited := signed[0] + "." + signed[1] + "." + base64.RawURLEncoding.EncodeToString([]byte(canvasBody(t, claims))) + "." + signed[3]
	untrusted := signed[0] + "." + base64.RawURLEncoding.EncodeToString([]byte("unknown-control-key")) + "." + signed[2] + "." + signed[3]
	for _, tc := range []struct {
		name, decision string
	}{
		{"payload edited after signing", edited},
		{"untrusted key id", untrusted},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := serveCanvas(t, router, http.MethodGet, canvasSpacePath, bearer, tc.decision, "")
			wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDecisionInvalid)
		})
	}
}

// TestCanvasListGetVersionAndRestoreRefuseAnAgentDecision: only the head read and the save take an agent decision. The other three
// refuse it as a mismatch, an authority refusal decided after the not-found probe (D21n) and before the store is used.
func TestCanvasListGetVersionAndRestoreRefuseAnAgentDecision(t *testing.T) {
	store := fakeStoreWithCanvas()
	router := fakeCanvasRouter(t, store)
	defer store.wantStoreUntouched(t)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	read := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(canvasTestOrg, canvasTestSpace, "agent-read-key-1", false, nil))
	write := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(canvasTestOrg, canvasTestSpace, "agent-write-key-1", true, nil))
	for _, route := range canvasRoutes {
		if route.agent {
			continue
		}
		decision := read
		if route.write {
			decision = write
		}
		t.Run(route.name, func(t *testing.T) {
			rec := serveCanvas(t, router, route.method, route.path, bearer, decision, route.body)
			wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDecisionMismatch)
		})
	}
}

// canvasAuthorityFailures are decisions whose envelope binds to the route (signed, unexpired, this org, user, Space and audience)
// but whose authority or posture does not: each is refused only once the route knows the canvas exists (D21n). human is the human
// decision for the route; agent, when set, is an agent decision instead.
func canvasAuthorityFailures(t *testing.T, route canvasRoute) map[string]string {
	t.Helper()
	failures := map[string]string{
		"zero data retention decision": canvasRouteHumanToken(t, route, "order-zdr", func(c *spacedecision.Claims) {
			c.ZeroDataRetention = true
		}),
		"the other action": canvasHumanToken(t, canvasHumanClaims(canvasTestOrg, canvasTestSpace, "order-action", !route.write),
			canvasRouteOp(route, "order-action")),
		"no canvas kind": canvasRouteHumanToken(t, route, "order-kind", func(c *spacedecision.Claims) { c.SpaceKind = "dm" }),
	}
	// A decision signed over a digest no request computes.
	digest := canvasHumanClaims(canvasTestOrg, canvasTestSpace, "order-digest", route.write)
	digest.PayloadDigest = canvasWrongDigest()
	failures["digest mismatch"] = signCanvasEnvelope(t, spacedecision.EnvelopeVersion, digest)
	if route.agent {
		failures["agent zero data retention decision"] = canvasRouteRunToken(t, route, func(r *spacedecision.RunClaims) {
			r.ZeroDataRetention = true
		})
	} else {
		failures["agent decision on a human route"] = signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion,
			canvasRunClaims(canvasTestOrg, canvasTestSpace, "order-agent", false, nil))
	}
	return failures
}

// TestCanvasRoutesAnswerNotFoundBeforeAuthorityAndZDR (D21n): a decision addressed to this caller and Space, but wrong for the
// operation or issued under zero data retention, is answered canvas_not_found when the Space holds no canvas, on every route. A save
// is not found only when it names a head (expected_version_id) or the Space is fenced; a plain create goes on to authority.
func TestCanvasRoutesAnswerNotFoundBeforeAuthorityAndZDR(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		body := route.body
		if route.name == "save" {
			body = `{"expected_version_id":"version-1","content":"# Room\n","idempotency_key":"save-key-1"}`
		}
		for name, decision := range canvasAuthorityFailures(t, route) {
			t.Run(route.name+"/"+name, func(t *testing.T) {
				store := &fakeCanvasStore{}
				rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, decision, body)
				wantCanvasRefusal(t, rec, http.StatusNotFound, canvas.CodeCanvasNotFound)
				store.wantStoreUntouched(t)
			})
		}
	}

	// A create into a purged (fenced) Space is not found before its authority is looked at.
	save := canvasRoutes[3]
	store := &fakeCanvasStore{probe: canvas.Probe{Fenced: true}}
	claims := canvasHumanClaims(canvasTestOrg, canvasTestSpace, "fenced-create", true)
	claims.PayloadDigest = canvasWrongDigest()
	rec := serveCanvas(t, fakeCanvasRouter(t, store), save.method, save.path, bearer,
		signCanvasEnvelope(t, spacedecision.EnvelopeVersion, claims), save.body)
	wantCanvasRefusal(t, rec, http.StatusNotFound, canvas.CodeCanvasNotFound)
	store.wantStoreUntouched(t)
}

// TestCanvasVersionRoutesAnswerVersionNotFoundBeforeAuthority (D21n): the get-version and restore routes answer
// canvas_version_not_found for a version the active canvas does not hold, before any authority or ZDR refusal.
func TestCanvasVersionRoutesAnswerVersionNotFoundBeforeAuthority(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		if route.name != "get version" && route.name != "restore" {
			continue
		}
		for name, decision := range canvasAuthorityFailures(t, route) {
			t.Run(route.name+"/"+name, func(t *testing.T) {
				store := &fakeCanvasStore{probe: canvas.Probe{CanvasID: fakeCanvasID}}
				rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, decision, route.body)
				wantCanvasRefusal(t, rec, http.StatusNotFound, canvas.CodeVersionNotFound)
				store.wantStoreUntouched(t)
			})
		}
	}
}

// TestCanvasRoutesAnswerAuthorityWhenTheCanvasExists (D21n): with the canvas and version present, the same decisions get their own
// refusal: a mismatch or digest refusal as 403, and a decision issued under zero data retention as the catalogue 412. None reaches
// the store past the probe.
func TestCanvasRoutesAnswerAuthorityWhenTheCanvasExists(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	want := map[string]struct {
		status int
		code   string
	}{
		"digest mismatch":                    {http.StatusForbidden, spacedecision.CodeDigestMismatch},
		"zero data retention decision":       {http.StatusPreconditionFailed, canvasZDRCode},
		"the other action":                   {http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		"no canvas kind":                     {http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		"agent zero data retention decision": {http.StatusPreconditionFailed, canvasZDRCode},
		"agent decision on a human route":    {http.StatusForbidden, spacedecision.CodeDecisionMismatch},
	}
	for _, route := range canvasRoutes {
		for name, decision := range canvasAuthorityFailures(t, route) {
			t.Run(route.name+"/"+name, func(t *testing.T) {
				store := fakeStoreWithCanvas()
				rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, decision, route.body)
				wantCanvasRefusal(t, rec, want[name].status, want[name].code)
				store.wantStoreUntouched(t)
			})
		}
	}
}

// TestCanvasDecisionEnvelopeIsRefusedBeforeTheNotFoundProbe (D21n): a decision for another Space, an unsigned one and a service
// bearer are refused before the store is asked whether the canvas exists, so they never learn it.
func TestCanvasDecisionEnvelopeIsRefusedBeforeTheNotFoundProbe(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	other := canvasHumanToken(t, canvasHumanClaims(canvasTestOrg, "space-room-2", "envelope-1", false), canvasReadOp("envelope-1"))
	signed := strings.Split(canvasHumanToken(t, canvasHumanClaims(canvasTestOrg, canvasTestSpace, "envelope-2", false),
		canvasReadOp("envelope-2")), ".")
	forged := signed[0] + "." + signed[1] + "." + signed[2] + "." + base64.RawURLEncoding.EncodeToString(make([]byte, ed25519.SignatureSize))
	for _, tc := range []struct {
		name, bearer, decision, code string
	}{
		{"another Space", bearer, other, spacedecision.CodeDecisionMismatch},
		{"not Control-signed", bearer, forged, spacedecision.CodeDecisionInvalid},
		{"service bearer", canvasServiceBearer(t, "service:other-plane", canvasTestOrg, []string{"wiki.read"}),
			canvasHumanToken(t, canvasHumanClaims(canvasTestOrg, canvasTestSpace, "envelope-3", false), canvasReadOp("envelope-3")),
			spacedecision.CodeDecisionMismatch},
	} {
		t.Run(tc.name, func(t *testing.T) {
			store := &fakeCanvasStore{}
			rec := serveCanvas(t, fakeCanvasRouter(t, store), http.MethodGet, canvasSpacePath, tc.bearer, tc.decision, "")
			wantCanvasRefusal(t, rec, http.StatusForbidden, tc.code)
			if store.probed != 0 {
				t.Fatalf("the route probed the store %d times before refusing the envelope", store.probed)
			}
		})
	}
}

// TestCanvasCorrectDecisionsReachTheStore: the decisions the order tests break, sent intact over the fake store's canvas, pass
// every refusal on their route, so the refusals above are the decisions' own and not a broken fixture.
func TestCanvasCorrectDecisionsReachTheStore(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer,
				canvasRouteHumanToken(t, route, "intact-1", nil), route.body)
			if rec.Code != http.StatusOK && rec.Code != http.StatusCreated {
				t.Fatalf("an intact decision = %d; body=%s", rec.Code, rec.Body.String())
			}
			if route.agent {
				store := fakeStoreWithCanvas()
				rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, canvasRouteRunToken(t, route, nil), route.body)
				if rec.Code != http.StatusOK && rec.Code != http.StatusCreated {
					t.Fatalf("an intact agent decision = %d; body=%s", rec.Code, rec.Body.String())
				}
			}
		})
	}
}

// canvasTestRepo returns the canvas store over WIKI_TEST_DATABASE_URL, the disposable local *_test database the repo package's
// integration tests use. The canvas migrations (20261116102141 and 20261116112048) and the dataplane_app role must already be applied
// there. Without that URL the tests that call it are skipped.
func canvasTestRepo(t *testing.T) *canvas.Repo {
	t.Helper()
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database with the canvas migrations applied")
	}
	poolConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse WIKI_TEST_DATABASE_URL: %v", err)
	}
	host := poolConfig.ConnConfig.Host
	if host != "localhost" && host != "127.0.0.1" && host != "::1" {
		t.Fatalf("refusing non-local test database host %q", host)
	}
	if !strings.HasSuffix(poolConfig.ConnConfig.Database, "_test") {
		t.Fatalf("refusing database %q: name must end in _test", poolConfig.ConnConfig.Database)
	}
	pool, err := pgxpool.NewWithConfig(context.Background(), poolConfig)
	if err != nil {
		t.Fatalf("connect canvas test pool: %v", err)
	}
	t.Cleanup(pool.Close)
	return canvas.NewRepo(pool)
}

// canvasUniqueScope returns an org and a Space no other test uses, and purges that Space when the test ends. Purging also fences the
// Space, which is harmless for a ref that is never reused.
func canvasUniqueScope(t *testing.T, repo *canvas.Repo) (string, string) {
	t.Helper()
	id := strings.ReplaceAll(uuid.NewString(), "-", "")
	org, space := "org-canvas-"+id, "space-canvas-"+id
	t.Cleanup(func() { _, _ = repo.PurgeSpace(context.Background(), org, space) })
	return org, space
}

// wantAgentVersion asserts that a version was written by the agent run: the run is its author, and the human it acts for is on_behalf_of.
func wantAgentVersion(t *testing.T, body canvasWriteBody) {
	t.Helper()
	v := body.Data.Version
	if v.AuthorKind != "agent" || v.AuthorID != "run:"+canvasTestRunID || v.OnBehalfOf == nil || *v.OnBehalfOf != canvasTestUser ||
		v.RunID == nil || *v.RunID != canvasTestRunID {
		t.Fatalf("version author = (kind %q, id %q, on_behalf_of %v, run_id %v), want the agent run %s for %s",
			v.AuthorKind, v.AuthorID, v.OnBehalfOf, v.RunID, canvasTestRunID, canvasTestUser)
	}
}

// TestCanvasCreateAndSaveTakeTheOperationTheSpaceHolds (gated on WIKI_TEST_DATABASE_URL): a Space with no canvas takes a create
// decision. Once the canvas exists the handler derives a save, so a decision that signed a create is refused on its digest and the
// head stays where it was.
func TestCanvasCreateAndSaveTakeTheOperationTheSpaceHolds(t *testing.T) {
	repo := canvasTestRepo(t)
	org, space := canvasUniqueScope(t, repo)
	router := canvasRouter(t, repo, true)
	bearer := canvasBearer(t, org, []string{"wiki.read", "wiki.write"}, false)
	versions := "/v1/space-canvas/spaces/" + space + "/versions"

	const createContent = "# Room\n"
	create := canvasHumanClaims(org, space, "create-key-1", true)
	createOp := spacedecision.CanvasOp{Op: spacedecision.OpCreate, Schema: spacedecision.SchemaCanvasWrite,
		ContentDigest: canvas.ContentDigest(createContent), IdempotencyKey: "create-key-1"}
	rec := serveCanvas(t, router, http.MethodPost, versions, bearer, canvasHumanToken(t, create, createOp),
		canvasBody(t, map[string]any{"content": createContent, "idempotency_key": "create-key-1"}))
	if rec.Code != http.StatusCreated {
		t.Fatalf("create = %d, want 201; body=%s", rec.Code, rec.Body.String())
	}
	created := decodeCanvasWrite(t, rec)
	head, canvasID := created.Data.Version.VersionID, created.Data.CanvasID
	if canvasID == "" || head == "" || created.Data.Version.VersionNo != 1 {
		t.Fatalf("create answered canvas %q head %q version_no %d, want a canvas with a first version", canvasID, head, created.Data.Version.VersionNo)
	}

	const saveContent = "# Room\n\nSecond line.\n"
	save := canvasHumanClaims(org, space, "save-key-1", true)
	saveOp := spacedecision.CanvasOp{Op: spacedecision.OpSave, Schema: spacedecision.SchemaCanvasWrite, CanvasID: canvasID,
		ExpectedVersionID: head, ContentDigest: canvas.ContentDigest(saveContent), IdempotencyKey: "save-key-1"}
	rec = serveCanvas(t, router, http.MethodPost, versions, bearer, canvasHumanToken(t, save, saveOp),
		canvasBody(t, map[string]any{"expected_version_id": head, "content": saveContent, "idempotency_key": "save-key-1"}))
	if rec.Code != http.StatusCreated {
		t.Fatalf("save over the canvas = %d, want 201; body=%s", rec.Code, rec.Body.String())
	}
	if saved := decodeCanvasWrite(t, rec); saved.Data.Version.VersionNo != 2 {
		t.Fatalf("save answered version_no %d, want 2", saved.Data.Version.VersionNo)
	}

	stale := canvasHumanClaims(org, space, "create-key-2", true)
	staleOp := spacedecision.CanvasOp{Op: spacedecision.OpCreate, Schema: spacedecision.SchemaCanvasWrite,
		ContentDigest: canvas.ContentDigest(saveContent), IdempotencyKey: "create-key-2"}
	rec = serveCanvas(t, router, http.MethodPost, versions, bearer, canvasHumanToken(t, stale, staleOp),
		canvasBody(t, map[string]any{"content": saveContent, "idempotency_key": "create-key-2"}))
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDigestMismatch)
}

// TestCanvasRestoreWithoutACanvasIsNotFound (gated on WIKI_TEST_DATABASE_URL): a restore for a Space with no canvas answers 404 once the
// decision is accepted, before any digest is compared.
func TestCanvasRestoreWithoutACanvasIsNotFound(t *testing.T) {
	repo := canvasTestRepo(t)
	org, space := canvasUniqueScope(t, repo)
	router := canvasRouter(t, repo, true)
	bearer := canvasBearer(t, org, []string{"wiki.write"}, false)
	restore := canvasHumanClaims(org, space, "restore-key-1", true)
	restoreOp := spacedecision.CanvasOp{Op: spacedecision.OpRestore, Schema: spacedecision.SchemaCanvasWrite,
		RestoreFromVersionID: "version-1", IdempotencyKey: "restore-key-1"}
	rec := serveCanvas(t, router, http.MethodPost, "/v1/space-canvas/spaces/"+space+"/versions/version-1/restore", bearer,
		canvasHumanToken(t, restore, restoreOp), canvasBody(t, map[string]any{"idempotency_key": "restore-key-1"}))
	wantCanvasRefusal(t, rec, http.StatusNotFound, canvas.CodeCanvasNotFound)
}

// TestCanvasAgentDecisionIsAcceptedOnTheHeadReadAndTheSave (gated on WIKI_TEST_DATABASE_URL): an agent decision reads the head and saves
// over it, and each version it writes is stamped with the run and the human it acts for. A save whose body key differs from the key
// the decision names is refused on its digest, and stores nothing.
func TestCanvasAgentDecisionIsAcceptedOnTheHeadReadAndTheSave(t *testing.T) {
	repo := canvasTestRepo(t)
	org, space := canvasUniqueScope(t, repo)
	router := canvasRouter(t, repo, true)
	bearer := canvasBearer(t, org, []string{"wiki.read", "wiki.write"}, false)
	path := "/v1/space-canvas/spaces/" + space
	versions := path + "/versions"

	// The Space has no canvas yet, so the agent's first save creates it. Its arguments are the flattened save: expected version empty.
	const createContent = "# Agent room\n"
	createArgs := []spacedecision.RunArgument{{Name: "expected_version_id"}, {Name: "content", Value: createContent}, {Name: "summary"}}
	create := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(org, space, "agent-create-key-1", true, createArgs))
	rec := serveCanvas(t, router, http.MethodPost, versions, bearer, create,
		canvasBody(t, map[string]any{"content": createContent, "idempotency_key": "agent-create-key-1"}))
	if rec.Code != http.StatusCreated {
		t.Fatalf("agent create = %d, want 201; body=%s", rec.Code, rec.Body.String())
	}
	first := decodeCanvasWrite(t, rec)
	wantAgentVersion(t, first)

	read := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(org, space, "agent-read-key-1", false, nil))
	if rec := serveCanvas(t, router, http.MethodGet, path, bearer, read, ""); rec.Code != http.StatusOK {
		t.Fatalf("agent head read = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}

	head := first.Data.Version.VersionID
	const saveContent = "# Agent room, revised\n"
	saveArgs := []spacedecision.RunArgument{{Name: "expected_version_id", Value: head}, {Name: "content", Value: saveContent}, {Name: "summary"}}
	save := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(org, space, "agent-save-key-1", true, saveArgs))
	rec = serveCanvas(t, router, http.MethodPost, versions, bearer, save,
		canvasBody(t, map[string]any{"expected_version_id": head, "content": saveContent, "idempotency_key": "agent-save-key-1"}))
	if rec.Code != http.StatusCreated {
		t.Fatalf("agent save = %d, want 201; body=%s", rec.Code, rec.Body.String())
	}
	second := decodeCanvasWrite(t, rec)
	wantAgentVersion(t, second)

	// The decision names agent-save-key-2. The body names agent-save-key-3, so the digest check refuses it.
	next := second.Data.Version.VersionID
	mismatchArgs := []spacedecision.RunArgument{{Name: "expected_version_id", Value: next}, {Name: "content", Value: saveContent}, {Name: "summary"}}
	mismatch := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, canvasRunClaims(org, space, "agent-save-key-2", true, mismatchArgs))
	rec = serveCanvas(t, router, http.MethodPost, versions, bearer, mismatch,
		canvasBody(t, map[string]any{"expected_version_id": next, "content": saveContent, "idempotency_key": "agent-save-key-3"}))
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDigestMismatch)
}
