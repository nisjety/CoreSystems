package http

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS04-S8 (pure tier): the join, settings, directory and directory-policy
// routes. The database-backed legs are in space_room_directory_live_test.go.

func roomActorCtx() map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": verevonGatewayPrincipal,
		"delegation_verified": true, "service_scopes": []string{spaceMembershipActScope},
		"org_id": "org-1", "user_id": "user-1",
	}
}

func roomDirectoryReaderCtx() map[string]any {
	ctx := roomActorCtx()
	ctx["service_scopes"] = []string{directoryReadScope}
	return ctx
}

func policyWriterCtx() map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": controlSpacePolicyPrincipal,
		"service_scopes": []string{"spaces:policy:write"},
	}
}

// directoryEngine mounts the REAL routes (core + feature registrars) behind a
// stand-in for the service-auth layer.
func roomDirectoryEngine(t *testing.T, repo *spaces.Repository, ctx map[string]any) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	engine := gin.New()
	setCtx := func(c *gin.Context) {
		for k, v := range ctx {
			c.Set(k, v)
		}
		c.Next()
	}
	spacesGroup := engine.Group("/api/v1/internal/spaces")
	spacesGroup.Use(setCtx)
	server.registerSpaceRoutes(spacesGroup)
	internal := engine.Group("/api/v1/internal")
	internal.Use(setCtx)
	server.registerFeatureRoutes(engine.Group("/api/v1"), internal)
	return engine
}

func roomDirectoryDo(engine *gin.Engine, method, target, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(method, target, bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	return rec
}

func errorCode(t *testing.T, rec *httptest.ResponseRecorder) string {
	t.Helper()
	var body struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &body)
	return body.Error.Code
}

func TestDirectoryRoutesAreOnTheEngineOnceEach(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	for _, route := range []string{
		"POST /api/v1/internal/spaces/:space_ref/join",
		"PATCH /api/v1/internal/spaces/:space_ref/settings",
		"PUT /api/v1/internal/spaces/directory-policy",
		"GET /api/v1/internal/directory/rooms",
	} {
		if counts[route] != 1 {
			t.Errorf("%s registered %d times, want 1", route, counts[route])
		}
	}
}

func TestJoinAndSettingsNeedTheGatewayDelegationAndTheMembershipActScope(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":         {},
		"other principal": {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true, "service_scopes": []string{spaceMembershipActScope}, "org_id": "o", "user_id": "u"},
		"no delegation":   {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "service_scopes": []string{spaceMembershipActScope}, "org_id": "o", "user_id": "u"},
		"missing scope":   {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": true, "service_scopes": []string{"spaces:issue"}, "org_id": "o", "user_id": "u"},
		"no acting user":  {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": true, "service_scopes": []string{spaceMembershipActScope}, "org_id": "o"},
		"no organization": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": true, "service_scopes": []string{spaceMembershipActScope}, "user_id": "u"},
	} {
		t.Run(name, func(t *testing.T) {
			engine := roomDirectoryEngine(t, &spaces.Repository{}, ctx)
			for _, call := range []struct{ method, target string }{
				{http.MethodPost, "/api/v1/internal/spaces/room-1/join"},
				{http.MethodPatch, "/api/v1/internal/spaces/room-1/settings"},
			} {
				rec := roomDirectoryDo(engine, call.method, call.target, `{"visibility":"private"}`)
				if rec.Code != http.StatusForbidden || errorCode(t, rec) != "room_delegation_required" {
					t.Errorf("%s %s: %d %s", call.method, call.target, rec.Code, rec.Body.String())
				}
			}
		})
	}
}

func TestJoinAndSettingsBodiesAreStrictTypedAndFailClosedWithoutARepository(t *testing.T) {
	engine := roomDirectoryEngine(t, &spaces.Repository{}, roomActorCtx())
	for name, tc := range map[string]struct{ method, target, body string }{
		"join, unknown field":        {http.MethodPost, "/api/v1/internal/spaces/room-1/join", `{"idempotency_key":"k","role":"owner"}`},
		"join, not json":             {http.MethodPost, "/api/v1/internal/spaces/room-1/join", `nope`},
		"join, trailing document":    {http.MethodPost, "/api/v1/internal/spaces/room-1/join", `{} {}`},
		"settings, unknown field":    {http.MethodPatch, "/api/v1/internal/spaces/room-1/settings", `{"visibility":"private","org_id":"org-9"}`},
		"settings, empty body":       {http.MethodPatch, "/api/v1/internal/spaces/room-1/settings", ``},
		"settings, wrong field type": {http.MethodPatch, "/api/v1/internal/spaces/room-1/settings", `{"visibility":3}`},
	} {
		t.Run(name, func(t *testing.T) {
			rec := roomDirectoryDo(engine, tc.method, tc.target, tc.body)
			if rec.Code != http.StatusBadRequest || errorCode(t, rec) != spaces.CodeInvalidRequest {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}
	// A join without a body is allowed (the key is optional); without a repository it fails closed.
	nilRepo := roomDirectoryEngine(t, nil, roomActorCtx())
	for _, call := range []struct{ method, target, body string }{
		{http.MethodPost, "/api/v1/internal/spaces/room-1/join", ``},
		{http.MethodPatch, "/api/v1/internal/spaces/room-1/settings", `{"visibility":"private"}`},
	} {
		rec := roomDirectoryDo(nilRepo, call.method, call.target, call.body)
		if rec.Code != http.StatusServiceUnavailable || errorCode(t, rec) != spaces.CodeSpaceAuthorityUnavailable {
			t.Errorf("%s: %d %s", call.method, rec.Code, rec.Body.String())
		}
	}
	// A bad settings value is a typed 400 from the repository's own validation.
	rec := roomDirectoryDo(engine, http.MethodPatch, "/api/v1/internal/spaces/room-1/settings", `{"join_role":"owner"}`)
	if rec.Code != http.StatusBadRequest || errorCode(t, rec) != spaces.CodeInvalidRequest {
		t.Fatalf("join_role owner: %d %s", rec.Code, rec.Body.String())
	}
}

func TestDirectoryRoomsNeedsTheReadScopeAndAnOrganizationMember(t *testing.T) {
	const target = "/api/v1/internal/directory/rooms"
	for name, ctx := range map[string]map[string]any{
		"no auth":                   {},
		"membership act scope only": roomActorCtx(),
		"no delegation":             {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "service_scopes": []string{directoryReadScope}, "org_id": "o", "user_id": "u"},
	} {
		t.Run(name, func(t *testing.T) {
			rec := roomDirectoryDo(roomDirectoryEngine(t, &spaces.Repository{}, ctx), http.MethodGet, target, "")
			if rec.Code != http.StatusForbidden || errorCode(t, rec) != "directory_delegation_required" {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}
	// Admitted but no repository: typed 503, never a 500 or an empty 200.
	rec := roomDirectoryDo(roomDirectoryEngine(t, nil, roomDirectoryReaderCtx()), http.MethodGet, target, "")
	if rec.Code != http.StatusServiceUnavailable || errorCode(t, rec) != "directory_unavailable" {
		t.Fatalf("no repository: %d %s", rec.Code, rec.Body.String())
	}
}

func TestDirectoryPolicyNeedsThePolicyPrincipalAndBothFields(t *testing.T) {
	const target = "/api/v1/internal/spaces/directory-policy"
	for name, ctx := range map[string]map[string]any{
		"gateway":      roomActorCtx(),
		"no auth":      {},
		"wrong scope":  {"auth_method": "service_principal", "service_id": controlSpacePolicyPrincipal, "service_scopes": []string{"spaces:issue"}},
		"other policy": {"auth_method": "service_principal", "service_id": "orchestrator-core", "service_scopes": []string{"spaces:policy:write"}},
	} {
		t.Run(name, func(t *testing.T) {
			rec := roomDirectoryDo(roomDirectoryEngine(t, &spaces.Repository{}, ctx), http.MethodPut, target, `{"org_id":"org-1","org_public_rooms_enabled":true}`)
			if rec.Code != http.StatusForbidden {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}

	engine := roomDirectoryEngine(t, &spaces.Repository{}, policyWriterCtx())
	for name, tc := range map[string]struct{ body, field string }{
		"no org":         {`{"org_public_rooms_enabled":true}`, "org_id"},
		"blank org":      {`{"org_id":"  ","org_public_rooms_enabled":true}`, "org_id"},
		"switch missing": {`{"org_id":"org-1"}`, "org_public_rooms_enabled"},
		"switch null":    {`{"org_id":"org-1","org_public_rooms_enabled":null}`, "org_public_rooms_enabled"},
	} {
		t.Run(name, func(t *testing.T) {
			rec := roomDirectoryDo(engine, http.MethodPut, target, tc.body)
			if rec.Code != http.StatusBadRequest || errorCode(t, rec) != spaces.CodeInvalidRequest || !strings.Contains(rec.Body.String(), tc.field) {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}
	// An explicit false is a real value, not a missing field.
	rec := roomDirectoryDo(roomDirectoryEngine(t, nil, policyWriterCtx()), http.MethodPut, target, `{"org_id":"org-1","org_public_rooms_enabled":false}`)
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("an explicit false passes validation and reaches the repository: %d %s", rec.Code, rec.Body.String())
	}
	if rec := roomDirectoryDo(engine, http.MethodPut, target, `{"org_id":"org-1","org_public_rooms_enabled":true,"x":1}`); rec.Code != http.StatusBadRequest {
		t.Fatalf("unknown field: %d", rec.Code)
	}
}

func TestDirectoryAndRoomWriterScopeMapping(t *testing.T) {
	const prefix = "/api/v1/internal/"
	cases := []struct {
		name, method, path, want string
		delegated                bool
	}{
		{"join", http.MethodPost, prefix + "spaces/room-1/join", spaceMembershipActScope, true},
		{"settings", http.MethodPatch, prefix + "spaces/room-1/settings", spaceMembershipActScope, true},
		{"directory policy", http.MethodPut, prefix + "spaces/directory-policy", "spaces:policy:write", false},
		{"directory rooms", http.MethodGet, prefix + "directory/rooms", directoryReadScope, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil))
			if got != tc.want {
				t.Fatalf("%s %s => %q, want %q", tc.method, tc.path, got, tc.want)
			}
			if got == "users:write:self" || got == "users:read:self" {
				t.Fatalf("%s fell through to a user self scope", tc.name)
			}
			if serviceScopeRequiresVerifiedDelegation(got) != tc.delegated {
				t.Fatalf("%s: delegated = %v, want %v", tc.name, !tc.delegated, tc.delegated)
			}
		})
	}
	// Neighbouring shapes keep their own mapping: a join of a nested path or a GET join is not the act scope.
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, prefix+"spaces/room-1/join", nil)); got == spaceMembershipActScope {
		t.Errorf("a GET join must not map to the act scope, got %q", got)
	}
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, prefix+"spaces/room-1/settings", nil)); got == spaceMembershipActScope {
		t.Errorf("a POST settings must not map to the act scope, got %q", got)
	}
}
