package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// F-f10-18: POST /api/v1/internal/spaces/skill-write-decision (pure tier). The
// SPACE_SKILL_WRITE_DECISIONS switch, the dedicated delegated scope and the
// checks made before any repository read. Issuance itself needs the database.

const spaceSkillPath = "/api/v1/internal/spaces/skill-write-decision"

var spaceSkillDigest = "sha256:" + strings.Repeat("a", 64)

func spaceSkillGatewayContext() map[string]any {
	ctx := gatewayContext()
	ctx["service_scopes"] = []string{spaceSkillWriteIssueScope}
	return ctx
}

func spaceSkillHarness(t *testing.T, enabled bool, repo *spaces.Repository, ctx map[string]any) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	server.SetSpaceSkillWriteDecisions(enabled)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	group.Use(func(c *gin.Context) {
		for key, value := range ctx {
			c.Set(key, value)
		}
		c.Next()
	})
	server.registerSpaceRoutes(group)
	return engine
}

func validSpaceSkillBody() map[string]any {
	return map[string]any{"space_ref": "room-1", "operation": "update", "skill_id": "skill-1", "request_digest": spaceSkillDigest}
}

func TestSpaceSkillRouteIsRegisteredOnceUnderItsOwnDelegatedScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+spaceSkillPath] != 1 {
		t.Fatalf("skill-write-decision registered %d times, want 1", counts["POST "+spaceSkillPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, spaceSkillPath, nil))
	if scope != "spaces:skill-write:issue" {
		t.Fatalf("scope = %q, want spaces:skill-write:issue", scope)
	}
	if scope == "spaces:issue" {
		t.Fatal("the skill write issuer must not share the generic issue scope")
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("the skill write scope must require a verified gateway delegation")
	}
	// No other path is mapped to it.
	other := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/memory-decision", nil))
	if other == scope {
		t.Fatal("the memory decision route resolved to the skill write scope")
	}
}

// The switch defaults off: a fresh server answers 404 with the typed code and
// reads nothing, even with no repository wired (the off path never reaches the
// 503 repository check).
func TestSpaceSkillRouteIsDarkWithTheSwitchOff(t *testing.T) {
	for name, repo := range map[string]*spaces.Repository{"no repository": nil, "repository": {}} {
		t.Run(name, func(t *testing.T) {
			engine := spaceSkillHarness(t, false, repo, spaceSkillGatewayContext())
			rec := postJSON(engine, spaceSkillPath, validSpaceSkillBody())
			if rec.Code != http.StatusNotFound {
				t.Fatalf("status = %d, want 404 (%s)", rec.Code, rec.Body.String())
			}
			var body map[string]any
			_ = json.Unmarshal(rec.Body.Bytes(), &body)
			if body["code"] != "space_skill_write_decisions_disabled" {
				t.Fatalf("code = %v (%s)", body["code"], rec.Body.String())
			}
		})
	}
}

func TestSpaceSkillRouteRequiresTheGatewayDelegationAndItsScope(t *testing.T) {
	delegated := func(mutate func(map[string]any)) map[string]any {
		ctx := spaceSkillGatewayContext()
		mutate(ctx)
		return ctx
	}
	for name, ctx := range map[string]map[string]any{
		"no auth":       {},
		"other service": delegated(func(c map[string]any) { c["service_id"] = "session-core" }),
		"gateway, unverified": delegated(func(c map[string]any) {
			c["delegation_verified"] = false
		}),
		"gateway holding only spaces:issue": delegated(func(c map[string]any) {
			c["service_scopes"] = []string{"spaces:issue"}
		}),
		"gateway holding no scope": delegated(func(c map[string]any) { delete(c, "service_scopes") }),
		"no delegated user":        delegated(func(c map[string]any) { c["user_id"] = "" }),
		"no delegated org":         delegated(func(c map[string]any) { c["org_id"] = "" }),
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceSkillHarness(t, true, &spaces.Repository{}, ctx)
			rec := postJSON(engine, spaceSkillPath, validSpaceSkillBody())
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestSpaceSkillRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceSkillHarness(t, true, nil, spaceSkillGatewayContext())
	rec := postJSON(engine, spaceSkillPath, validSpaceSkillBody())
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

// Shape is refused with 400 before any membership read (the empty repository
// would fail a read, so a 400 proves none happened). A role in the body is
// ignored: the route takes none from the client.
func TestSpaceSkillRouteRejectsBadShapesBeforeReadingAuthority(t *testing.T) {
	engine := spaceSkillHarness(t, true, &spaces.Repository{}, spaceSkillGatewayContext())
	with := func(key string, value any) map[string]any {
		body := validSpaceSkillBody()
		if value == nil {
			delete(body, key)
		} else {
			body[key] = value
		}
		return body
	}
	for name, body := range map[string]map[string]any{
		"no space_ref":            with("space_ref", nil),
		"no operation":            with("operation", nil),
		"no request_digest":       with("request_digest", nil),
		"unknown operation":       with("operation", "purge"),
		"update without an id":    with("skill_id", nil),
		"padded skill id":         with("skill_id", "skill-1 "),
		"digest without a prefix": with("request_digest", strings.Repeat("a", 64)),
		"skill text as digest":    with("request_digest", "always answer in French"),
		"create with a skill id": func() map[string]any {
			b := validSpaceSkillBody()
			b["operation"] = "create"
			return b
		}(),
	} {
		t.Run(name, func(t *testing.T) {
			rec := postJSON(engine, spaceSkillPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}
