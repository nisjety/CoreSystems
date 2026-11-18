package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS19-S2: POST /api/v1/internal/spaces/canvas-decision (pure tier). The
// dispatch legs (a non-recipient's refusal, an archived-room write's typed
// 409, and an editor's issued decision) need a database and are at the end of
// the file, gated on TEST_DATABASE_URL like the other live tests in this
// package.

const canvasDecisionPath = "/api/v1/internal/spaces/canvas-decision"

func canvasBodyDigest() string { return "sha256:" + strings.Repeat("a", 64) }

func validCanvasDecisionBody() map[string]any {
	return map[string]any{"space_ref": "room-1", "op": "read", "idempotency_key": "canvas-read-1"}
}

func TestCanvasDecisionRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+canvasDecisionPath] != 1 {
		t.Fatalf("canvas-decision registered %d times, want 1", counts["POST "+canvasDecisionPath])
	}
	// Unlike scopes_space_import_decision.go, this route's scope rule
	// (scopes_space_canvas_decision.go) matches the exact path only, with no
	// trailing-slash tolerance; it is checked as it is, not as the sibling
	// route happens to behave.
	if scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, canvasDecisionPath, nil)); scope != "spaces:issue" {
		t.Fatalf("%s scope = %q, want spaces:issue", canvasDecisionPath, scope)
	}
	if !serviceScopeRequiresVerifiedDelegation("spaces:issue") {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	// A GET of the same path is not claimed by the issuer rule.
	for _, rule := range spaceFeatureScopeRules {
		if _, ok := rule(http.MethodGet, canvasDecisionPath); ok {
			t.Error("a GET of canvas-decision must not be claimed by the issuer rule")
		}
	}
}

func TestCanvasDecisionRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"imports core":        {"auth_method": "service_principal", "service_id": importsCorePrincipal, "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			if rec := postJSON(engine, canvasDecisionPath, validCanvasDecisionBody()); rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestCanvasDecisionRejectsMissingFieldsAndFailsClosedWithoutARepository(t *testing.T) {
	if rec := postJSON(spaceIssuerHarness(t, nil, gatewayContext()), canvasDecisionPath, validCanvasDecisionBody()); rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no repository: status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{"space_ref", "op", "idempotency_key"} {
		t.Run("missing "+field, func(t *testing.T) {
			body := validCanvasDecisionBody()
			delete(body, field)
			rec := postJSON(engine, canvasDecisionPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			var parsed map[string]any
			if err := json.Unmarshal(rec.Body.Bytes(), &parsed); err != nil || parsed["code"] != spaceCanvasInvalidCode {
				t.Fatalf("body = %s, want code %s", rec.Body.String(), spaceCanvasInvalidCode)
			}
			if strings.Contains(rec.Body.String(), "token") {
				t.Fatalf("a 400 carries no token: %s", rec.Body.String())
			}
		})
	}
}

// The operation set is closed and each operation's fields are checked before
// any authority is read, so a malformed shape is a 400 even without a usable
// repository. The production rule (canvas_decision.go CanvasDecisionRequest.Validate)
// is: create forbids canvas_id/expected_version_id/restore_from_version_id and
// requires content_digest; save requires content_digest; restore requires
// restore_from_version_id and forbids content_digest.
func TestCanvasDecisionRejectsMalformedOperationShapes(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	cases := map[string]map[string]any{
		"unknown op": {
			"space_ref": "room-1", "op": "update", "idempotency_key": "k",
		},
		"create carrying a canvas_id": {
			"space_ref": "room-1", "op": "create", "canvas_id": "canvas-1",
			"content_digest": canvasBodyDigest(), "idempotency_key": "k",
		},
		"save without a digest": {
			"space_ref": "room-1", "op": "save", "canvas_id": "canvas-1",
			"expected_version_id": "version-2", "idempotency_key": "k",
		},
		"restore without a restore source": {
			"space_ref": "room-1", "op": "restore", "canvas_id": "canvas-1",
			"expected_version_id": "version-2", "idempotency_key": "k",
		},
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			rec := postJSON(engine, canvasDecisionPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			var parsed map[string]any
			if err := json.Unmarshal(rec.Body.Bytes(), &parsed); err != nil || parsed["code"] != spaceCanvasInvalidCode {
				t.Fatalf("body = %s, want code %s", rec.Body.String(), spaceCanvasInvalidCode)
			}
		})
	}
}

// Live dispatch legs (TEST_DATABASE_URL via d12Pool; skipped without it).

// canvasLiveHarness mounts the real Space route group behind the gateway
// context for one org/user pair, the way importHarness does for the import
// route.
func canvasLiveHarness(t *testing.T, repo *spaces.Repository, org, user string) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = org, user
	group.Use(func(c *gin.Context) {
		for k, v := range ctx {
			c.Set(k, v)
		}
		c.Next()
	})
	server.registerSpaceRoutes(group)
	return engine
}

type canvasDecisionBody struct {
	Data struct {
		Decision  spaces.Decision `json:"decision"`
		Token     string          `json:"token"`
		SpaceKind string          `json:"space_kind"`
	} `json:"data"`
}

func TestCanvasDecisionDispatchesLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws19s2")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.CanvasEntitled = true })

	readBody := map[string]any{"space_ref": room.Room, "op": "read", "idempotency_key": "canvas-read-1"}
	writeBody := map[string]any{
		"space_ref": room.Room, "op": "create", "content_digest": canvasBodyDigest(), "idempotency_key": "canvas-create-1",
	}

	// An editor in the audience gets an ordinary canvas read decision with the
	// space_kind and a verifiable token.
	editor := canvasLiveHarness(t, repo, room.Org, room.Editor)
	rec := postJSON(editor, canvasDecisionPath, readBody)
	if rec.Code != http.StatusOK {
		t.Fatalf("editor read: %d %s", rec.Code, rec.Body.String())
	}
	var issued canvasDecisionBody
	if err := json.Unmarshal(rec.Body.Bytes(), &issued); err != nil {
		t.Fatal(err)
	}
	if issued.Data.SpaceKind != "room" || issued.Data.Token == "" {
		t.Fatalf("editor read body = %s", rec.Body.String())
	}
	decision := d12VerifyToken(t, issued.Data.Token, public)
	if decision.ActionID != "data.canvas.read" || decision.SpaceRef != room.Room || decision.SubjectID != room.Editor {
		t.Fatalf("decision = %+v", decision)
	}

	// An editor also gets an ordinary canvas write decision.
	rec = postJSON(editor, canvasDecisionPath, writeBody)
	if rec.Code != http.StatusOK {
		t.Fatalf("editor write: %d %s", rec.Code, rec.Body.String())
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &issued); err != nil {
		t.Fatal(err)
	}
	decision = d12VerifyToken(t, issued.Data.Token, public)
	if decision.ActionID != "data.canvas.write" {
		t.Fatalf("write decision = %+v", decision)
	}

	// A non-recipient (a room member left out of the published audience) is
	// refused with no token, for either operation.
	outside := canvasLiveHarness(t, repo, room.Org, room.Outside)
	for name, body := range map[string]map[string]any{"read": readBody, "write": writeBody} {
		rec = postJSON(outside, canvasDecisionPath, body)
		if rec.Code != http.StatusForbidden || strings.Contains(rec.Body.String(), `"token"`) {
			t.Fatalf("non-recipient %s: %d %s", name, rec.Code, rec.Body.String())
		}
	}

	// Archiving the room: the write answers the typed 409 room_archived with
	// no token, and the read still succeeds.
	if _, err := pool.Exec(context.Background(),
		`UPDATE registered_spaces SET registration_state='archived', archived_at=NOW(), archived_by=$2 WHERE space_ref=$1`,
		room.Room, room.Owner,
	); err != nil {
		t.Fatal(err)
	}
	rec = postJSON(editor, canvasDecisionPath, writeBody)
	if rec.Code != http.StatusConflict {
		t.Fatalf("archived-room write: want 409, got %d (%s)", rec.Code, rec.Body.String())
	}
	var denial struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &denial); err != nil || denial.Error.Code != spaces.CodeRoomArchived {
		t.Fatalf("archived-room write: want typed room_archived, got %s", rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("an archived-room refusal carried a token: %s", rec.Body.String())
	}
	rec = postJSON(editor, canvasDecisionPath, readBody)
	if rec.Code != http.StatusOK {
		t.Fatalf("archived-room read: want 200, got %d (%s)", rec.Code, rec.Body.String())
	}
}
