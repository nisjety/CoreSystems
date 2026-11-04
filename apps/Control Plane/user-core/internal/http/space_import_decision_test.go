package http

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS14-S1: the unified import-decision route. The pure tier covers
// registration, scope, the guard, body shape and the typed 403 bodies; the
// dispatch legs need a database and are at the end of the file.

const importDecisionPath = "/api/v1/internal/spaces/import-decision"

func validImportDecisionBody() map[string]any {
	return map[string]any{"space_ref": "room-1", "source_type": "space_upload", "idempotency_key": "import-1"}
}

func TestImportDecisionRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+importDecisionPath] != 1 {
		t.Fatalf("import-decision registered %d times, want 1", counts["POST "+importDecisionPath])
	}
	for _, path := range []string{importDecisionPath, importDecisionPath + "/"} {
		if scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, path, nil)); scope != "spaces:issue" {
			t.Fatalf("%s scope = %q, want spaces:issue", path, scope)
		}
	}
	if !serviceScopeRequiresVerifiedDelegation("spaces:issue") {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	// The personal route is untouched, and a GET is not claimed by the rule.
	if scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/personal-import-decision", nil)); scope != "spaces:issue" {
		t.Fatalf("personal-import-decision scope = %q", scope)
	}
	for _, rule := range spaceFeatureScopeRules {
		if _, ok := rule(http.MethodGet, importDecisionPath); ok {
			t.Error("a GET of import-decision must not be claimed by the issuer rule")
		}
	}
}

func TestImportDecisionRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"imports core":        {"auth_method": "service_principal", "service_id": importsCorePrincipal, "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			if rec := postJSON(engine, importDecisionPath, validImportDecisionBody()); rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestImportDecisionRejectsMissingFieldsAndFailsClosedWithoutARepository(t *testing.T) {
	if rec := postJSON(spaceIssuerHarness(t, nil, gatewayContext()), importDecisionPath, validImportDecisionBody()); rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no repository: status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{"space_ref", "source_type", "idempotency_key"} {
		t.Run("missing "+field, func(t *testing.T) {
			body := validImportDecisionBody()
			delete(body, field)
			rec := postJSON(engine, importDecisionPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "token") {
				t.Fatalf("a 400 carries no token: %s", rec.Body.String())
			}
		})
	}
}

func TestSharedImportDisabledAnswersTheStable404(t *testing.T) {
	gin.SetMode(gin.TestMode)
	rec := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(rec)
	c.Request = httptest.NewRequest(http.MethodPost, importDecisionPath, nil)
	writeSharedImportDisabled(c)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", rec.Code)
	}
	var body map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil || body["code"] != "shared_import_disabled" {
		t.Fatalf("body = %s", rec.Body.String())
	}
}

func TestSharedImportIssuerDenialBodiesCarryTheirCode(t *testing.T) {
	gin.SetMode(gin.TestMode)
	write := func(err error) (int, map[string]any) {
		rec := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(rec)
		c.Request = httptest.NewRequest(http.MethodPost, importDecisionPath, nil)
		writeSharedImportIssuerDenial(c, err, "Space import is not authorized")
		var body map[string]any
		_ = json.Unmarshal(rec.Body.Bytes(), &body)
		return rec.Code, body
	}

	status, body := write(spaces.ErrSharedImportSourceType)
	if status != http.StatusForbidden || body["code"] != "source_type_not_allowed" || body["error"] != "Space import is not authorized" {
		t.Fatalf("source type refusal = %d %v", status, body)
	}
	status, body = write(errors.Join(errors.New("ctx"), spaces.ErrSharedImportSourceType))
	if status != http.StatusForbidden || body["code"] != "source_type_not_allowed" {
		t.Fatalf("wrapped source type refusal = %d %v", status, body)
	}

	// Authority denials keep the vocabulary of every other issuer.
	evidence := validSharedImportHTTPEvidence()
	for name, tc := range map[string]struct {
		mutate func(*spaces.PersonalThreadDecisionEvidence)
		code   spaces.DenialCode
	}{
		"viewer":          {func(e *spaces.PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, spaces.DenialRoleBelowFloor},
		"entitlement off": {func(e *spaces.PersonalThreadDecisionEvidence) { e.ImportWriteEntitled = false }, spaces.DenialEntitlementOff},
		"zdr":             {func(e *spaces.PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, spaces.DenialZDRForbidsEffect},
	} {
		t.Run(name, func(t *testing.T) {
			e := evidence
			tc.mutate(&e)
			_, err := spaces.IssueSharedImportDecision(e, spaces.PersonalImportDecisionRequest{
				DecisionRef: "d", IdempotencyKey: "k", SourceType: spaces.SpaceUploadSourceType, Nonce: "n",
			}, time.Now().UTC())
			status, body := write(err)
			if status != http.StatusForbidden || body["code"] != string(tc.code) {
				t.Fatalf("%s = %d %v, want 403 with code %s", name, status, body, tc.code)
			}
			if _, hasToken := body["token"]; hasToken {
				t.Fatal("a refusal carries no token")
			}
		})
	}
	// An untyped failure keeps the plain 403.
	status, body = write(errors.New("invalid request"))
	if status != http.StatusForbidden || body["code"] != nil {
		t.Fatalf("untyped refusal = %d %v", status, body)
	}
}

func validSharedImportHTTPEvidence() spaces.PersonalThreadDecisionEvidence {
	return spaces.PersonalThreadDecisionEvidence{
		Membership: spaces.CurrentMembership{
			SpaceRef: "room-1", OrgID: "org-1", SubjectID: "user-1", Kind: spaces.KindRoom, Role: "editor",
			Revisions: spaces.AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 2, Entitlement: 3},
		},
		RecipientAudienceRef: "audience:room-1:2", RecipientAudienceHash: "sha256:test-audience",
		ImportWriteEntitled: true, ResourceAuthorizationRef: "control:room-1:ingestion-import:7",
		Privacy: spaces.PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
}

// Live dispatch legs (TEST_DATABASE_URL via d12Pool).

func importHarness(t *testing.T, repo *spaces.Repository, shared bool, org, user string) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	server.SetSharedImportDecisions(shared)
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

func TestImportDecisionDispatchesPersonalVersusSharedLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws14s1")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ImportWriteEntitled = true })
	body := func(source string) map[string]any {
		return map[string]any{"space_ref": room.Room, "source_type": source, "idempotency_key": "import-1"}
	}

	// Flag off (the default): a shared Space answers the stable 404.
	off := importHarness(t, repo, false, room.Org, room.Editor)
	rec := postJSON(off, importDecisionPath, body("space_upload"))
	if rec.Code != http.StatusNotFound || !strings.Contains(rec.Body.String(), "shared_import_disabled") {
		t.Fatalf("flag off: %d %s", rec.Code, rec.Body.String())
	}

	// Flag on: an editor in the audience gets an ordinary import decision.
	on := importHarness(t, repo, true, room.Org, room.Editor)
	rec = postJSON(on, importDecisionPath, body("space_upload"))
	if rec.Code != http.StatusOK {
		t.Fatalf("flag on: %d %s", rec.Code, rec.Body.String())
	}
	var issued d12DecisionBody
	if err := json.Unmarshal(rec.Body.Bytes(), &issued); err != nil {
		t.Fatal(err)
	}
	decision := d12VerifyToken(t, issued.Data.Token, public)
	if decision.ActionID != "ingestion.import.write" || decision.ImportSourceType != "space_upload" || decision.SpaceRef != room.Room {
		t.Fatalf("decision = %+v", decision)
	}

	// A connector source is refused with its own code; a viewer and an outsider are refused with theirs.
	rec = postJSON(on, importDecisionPath, body("notion"))
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "source_type_not_allowed") {
		t.Fatalf("connector source: %d %s", rec.Code, rec.Body.String())
	}
	viewer := importHarness(t, repo, true, room.Org, room.Viewer)
	rec = postJSON(viewer, importDecisionPath, body("space_upload"))
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), string(spaces.DenialRoleBelowFloor)) {
		t.Fatalf("viewer: %d %s", rec.Code, rec.Body.String())
	}
	outside := importHarness(t, repo, true, room.Org, room.Outside)
	if rec := postJSON(outside, importDecisionPath, body("space_upload")); rec.Code != http.StatusForbidden || strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("member outside the audience: %d %s", rec.Code, rec.Body.String())
	}
}

func TestImportExecutionDecisionDispatchesToSharedWhenTheFlagIsOnLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws14s1x")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ImportWriteEntitled = true })

	engineFor := func(shared bool) *gin.Engine {
		gin.SetMode(gin.TestMode)
		server := NewServer(nil, nil, nil, nil, "0")
		server.SetSpaceRepository(repo)
		server.SetSharedImportDecisions(shared)
		engine := gin.New()
		group := engine.Group("/api/v1/internal/spaces")
		group.Use(func(c *gin.Context) {
			c.Set("auth_method", "service_principal")
			c.Set("service_id", importsCorePrincipal)
			c.Next()
		})
		server.registerSpaceRoutes(group)
		return engine
	}
	body := map[string]any{"intent": map[string]any{
		"org_id": room.Org, "space_ref": room.Room, "subject_id": room.Editor,
		"action_schema_hash": "sha256:ingestion-import-v1", "payload_digest": "sha256:durable-import-payload",
		"idempotency_key": "import-1", "source_type": "space_upload",
	}}
	path := "/api/v1/internal/spaces/import-execution-decision"

	// Flag off: the shared Space is not served by the personal resolver.
	if rec := postJSON(engineFor(false), path, body); rec.Code == http.StatusOK {
		t.Fatalf("flag off must not issue a shared execution decision: %s", rec.Body.String())
	}
	rec := postJSON(engineFor(true), path, body)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("flag on: %d %s", rec.Code, rec.Body.String())
	}
	// A member removed since the job started is refused at effect time.
	if _, err := pool.Exec(context.Background(), `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Editor); err != nil {
		t.Fatal(err)
	}
	if rec := postJSON(engineFor(true), path, body); rec.Code == http.StatusOK || strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("removed member: %d %s", rec.Code, rec.Body.String())
	}
}
