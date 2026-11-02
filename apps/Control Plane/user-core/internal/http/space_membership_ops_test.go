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

// WS04-S4 / WS04-S7: the guards, the strict body decoding and the typed error
// envelope of the room writer and archive routes. The real routes (registered
// through the P-UC1 seam) and the real guards run; Postgres is not involved,
// so a nil or database-less repository stands in. Behaviour behind the guard
// is covered by the TEST_DATABASE_URL tests in internal/spaces.

type roomCaller struct {
	authMethod string
	serviceID  string
	delegated  bool
	scopes     []string
	userID     string
	orgID      string
}

func gatewayActor(scopes ...string) roomCaller {
	return roomCaller{
		authMethod: "service_principal", serviceID: verevonGatewayPrincipal, delegated: true,
		scopes: scopes, userID: "user-1", orgID: "org-1",
	}
}

func roomRequest(server *Server, caller roomCaller, method, target, body string) *httptest.ResponseRecorder {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.Use(func(c *gin.Context) {
		c.Set("auth_method", caller.authMethod)
		c.Set("service_id", caller.serviceID)
		c.Set("delegation_verified", caller.delegated)
		c.Set("service_scopes", caller.scopes)
		c.Set("user_id", caller.userID)
		c.Set("org_id", caller.orgID)
	})
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(server, group)
	}
	request := httptest.NewRequest(method, target, bytes.NewBufferString(body))
	request.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, request)
	return w
}

func roomErrorCode(t *testing.T, w *httptest.ResponseRecorder) string {
	t.Helper()
	var body struct {
		Error struct {
			Code    string         `json:"code"`
			Message string         `json:"message"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("not a typed error envelope: %v (%s)", err, w.Body.String())
	}
	if body.Error.Details == nil {
		t.Fatalf("details must be an object, never null: %s", w.Body.String())
	}
	return body.Error.Code
}

const (
	membersPath    = "/api/v1/internal/spaces/space-1/members"
	memberPath     = "/api/v1/internal/spaces/space-1/members/user-2"
	candidatesPath = "/api/v1/internal/spaces/space-1/audience-candidates"
	archivePath    = "/api/v1/internal/spaces/space-1/archive"
	unarchivePath  = "/api/v1/internal/spaces/space-1/unarchive"
)

type roomActRoute struct {
	method, path, body string
}

var roomActRoutes = []roomActRoute{
	{http.MethodPost, membersPath, `{"subject_id":"user-2","role":"editor"}`},
	{http.MethodPatch, memberPath, `{"role":"viewer"}`},
	{http.MethodDelete, memberPath, ``},
	{http.MethodPost, archivePath, ``},
	{http.MethodPost, unarchivePath, ``},
}

func TestRoomWriterRoutesRegisterThroughTheSeam(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(&Server{}, group)
	}
	counts := routeCounts(engine.Routes())
	prefix := "/api/v1/internal/spaces"
	for _, route := range []string{
		"POST " + prefix + "/:space_ref/members",
		"PATCH " + prefix + "/:space_ref/members/:subject_id",
		"DELETE " + prefix + "/:space_ref/members/:subject_id",
		"GET " + prefix + "/:space_ref/audience-candidates",
		"POST " + prefix + "/:space_ref/archive",
		"POST " + prefix + "/:space_ref/unarchive",
	} {
		if counts[route] != 1 {
			t.Errorf("route %q registered %d times", route, counts[route])
		}
	}
}

// Every ACT route (membership writer and archive) is refused for anything but
// the gateway workload with a verified delegation, the writer scope, and a
// delegated user and organization. The credential is the kill switch.
func TestRoomActRoutesRefuseEveryCallerButTheDelegatedGatewayWithTheWriterScope(t *testing.T) {
	cases := []struct {
		name   string
		caller roomCaller
	}{
		{"wrong principal", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.serviceID = "some-other-service"
			return c
		}()},
		{"application lifecycle principal", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.serviceID = applicationSpaceLifecyclePrincipal
			return c
		}()},
		{"gateway without the writer scope", gatewayActor("spaces:resolve", "spaces:membership:read")},
		{"gateway with no scopes at all", gatewayActor()},
		{"gateway with the old write scope only", gatewayActor("spaces:membership:write")},
		{"missing delegation", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.delegated = false
			return c
		}()},
		{"delegation without a user", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.userID = " "
			return c
		}()},
		{"delegation without an organization", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.orgID = ""
			return c
		}()},
		{"not a service principal", func() roomCaller {
			c := gatewayActor("spaces:membership:act")
			c.authMethod = "jwt"
			return c
		}()},
	}
	for _, route := range roomActRoutes {
		for _, tc := range cases {
			w := roomRequest(&Server{}, tc.caller, route.method, route.path, route.body)
			if w.Code != http.StatusForbidden {
				t.Errorf("%s %s as %s: want 403, got %d (%s)", route.method, route.path, tc.name, w.Code, w.Body.String())
				continue
			}
			if code := roomErrorCode(t, w); code != "room_delegation_required" {
				t.Errorf("%s %s as %s: code %q", route.method, route.path, tc.name, code)
			}
		}
	}
}

// A correct caller passes the guard. With no repository wired the route
// answers the typed 503 space_authority_unavailable, never a 200 and never an
// untyped body: the writer fails closed.
func TestRoomActRoutesWithoutARepositoryFailClosedWithTheTypedOutage(t *testing.T) {
	for _, route := range roomActRoutes {
		w := roomRequest(&Server{}, gatewayActor("spaces:membership:act"), route.method, route.path, route.body)
		if w.Code != http.StatusServiceUnavailable {
			t.Errorf("%s %s: want 503, got %d (%s)", route.method, route.path, w.Code, w.Body.String())
			continue
		}
		if code := roomErrorCode(t, w); code != spaces.CodeSpaceAuthorityUnavailable {
			t.Errorf("%s %s: code %q", route.method, route.path, code)
		}
	}
}

// A repository without a database stands in for the writer: the body is
// decoded and validated before any database use, and a valid request that
// reaches the transaction gets the typed outage.
func TestRoomActRoutesDecodeTheBodyStrictly(t *testing.T) {
	server := &Server{}
	server.SetSpaceRepository(spaces.NewRepository(nil))
	actor := gatewayActor("spaces:membership:act")

	bad := []struct {
		name  string
		route roomActRoute
	}{
		{"add: unknown field", roomActRoute{http.MethodPost, membersPath, `{"subject_id":"user-2","role":"editor","org_id":"org-evil"}`}},
		{"add: not json", roomActRoute{http.MethodPost, membersPath, `{"subject_id":`}},
		{"add: empty body", roomActRoute{http.MethodPost, membersPath, ``}},
		{"add: trailing document", roomActRoute{http.MethodPost, membersPath, `{"subject_id":"user-2"}{"subject_id":"user-3"}`}},
		{"add: wrong type", roomActRoute{http.MethodPost, membersPath, `{"subject_id":7}`}},
		{"add: missing subject", roomActRoute{http.MethodPost, membersPath, `{"role":"editor"}`}},
		{"add: unknown role", roomActRoute{http.MethodPost, membersPath, `{"subject_id":"user-2","role":"admin"}`}},
		{"add: oversized idempotency key", roomActRoute{http.MethodPost, membersPath, `{"subject_id":"user-2","idempotency_key":"` + strings.Repeat("k", 129) + `"}`}},
		{"change: unknown field", roomActRoute{http.MethodPatch, memberPath, `{"role":"viewer","actor_id":"user-9"}`}},
		{"change: empty body", roomActRoute{http.MethodPatch, memberPath, ``}},
		{"change: unknown role", roomActRoute{http.MethodPatch, memberPath, `{"role":"superuser"}`}},
		{"change: missing role", roomActRoute{http.MethodPatch, memberPath, `{}`}},
		{"remove: unknown field", roomActRoute{http.MethodDelete, memberPath, `{"force":true}`}},
		{"archive: unknown field", roomActRoute{http.MethodPost, archivePath, `{"state":"archived"}`}},
		{"unarchive: unknown field", roomActRoute{http.MethodPost, unarchivePath, `{"reason":"x"}`}},
		{"archive: oversized idempotency key", roomActRoute{http.MethodPost, archivePath, `{"idempotency_key":"` + strings.Repeat("k", 129) + `"}`}},
	}
	for _, tc := range bad {
		w := roomRequest(server, actor, tc.route.method, tc.route.path, tc.route.body)
		if w.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code := roomErrorCode(t, w); code != spaces.CodeInvalidRequest {
			t.Errorf("%s: code %q", tc.name, code)
		}
	}

	good := []struct {
		name  string
		route roomActRoute
	}{
		{"add", roomActRoute{http.MethodPost, membersPath, `{"subject_id":"user-2","role":"editor","idempotency_key":"k-1"}`}},
		{"change", roomActRoute{http.MethodPatch, memberPath, `{"role":"viewer","expected_revision":3}`}},
		{"remove with no body", roomActRoute{http.MethodDelete, memberPath, ``}},
		{"remove with a key", roomActRoute{http.MethodDelete, memberPath, `{"idempotency_key":"k-2"}`}},
		{"archive with no body", roomActRoute{http.MethodPost, archivePath, ``}},
		{"unarchive with a key", roomActRoute{http.MethodPost, unarchivePath, `{"idempotency_key":"k-3"}`}},
	}
	for _, tc := range good {
		w := roomRequest(server, actor, tc.route.method, tc.route.path, tc.route.body)
		if w.Code != http.StatusServiceUnavailable {
			t.Errorf("%s: a valid request must reach the repository (typed 503 without a database), got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code := roomErrorCode(t, w); code != spaces.CodeSpaceAuthorityUnavailable {
			t.Errorf("%s: code %q", tc.name, code)
		}
	}
}

func TestAudienceCandidatesNeedsTheApplicationLifecyclePrincipalAndTheReadScope(t *testing.T) {
	lifecycle := func(scopes ...string) roomCaller {
		return roomCaller{authMethod: "service_principal", serviceID: applicationSpaceLifecyclePrincipal, scopes: scopes}
	}
	refused := []struct {
		name   string
		caller roomCaller
	}{
		{"the gateway, even with the read scope", gatewayActor("spaces:membership:read")},
		{"lifecycle without the read scope", lifecycle("spaces:register")},
		{"lifecycle holding only the writer scope", lifecycle("spaces:membership:act")},
		{"lifecycle holding the old write scope", lifecycle("spaces:membership:write")},
		{"a user session", roomCaller{authMethod: "jwt", serviceID: applicationSpaceLifecyclePrincipal, scopes: []string{"spaces:membership:read"}}},
	}
	for _, tc := range refused {
		w := roomRequest(&Server{}, tc.caller, http.MethodGet, candidatesPath, "")
		if w.Code != http.StatusForbidden {
			t.Errorf("%s: want 403, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code := roomErrorCode(t, w); code != "audience_reader_required" {
			t.Errorf("%s: code %q", tc.name, code)
		}
	}
	// A correct caller passes the guard and reaches the repository (typed 503
	// without one). No delegation is needed: this is a service read.
	w := roomRequest(&Server{}, lifecycle("spaces:membership:read"), http.MethodGet, candidatesPath, "")
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("want 503 without a repository, got %d (%s)", w.Code, w.Body.String())
	}
	if code := roomErrorCode(t, w); code != spaces.CodeSpaceAuthorityUnavailable {
		t.Fatalf("code %q", code)
	}
}

func TestWriteRoomErrorKeepsTypedStatusAndHidesUntypedCauses(t *testing.T) {
	gin.SetMode(gin.TestMode)
	run := func(err error) *httptest.ResponseRecorder {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = httptest.NewRequest(http.MethodPost, "/x", nil)
		writeRoomError(c, err)
		return w
	}
	typed := run(&spaces.RoomError{Status: http.StatusConflict, Code: spaces.CodeRoomMemberChanged, Message: "changed", Details: map[string]any{"current_revision": 7}})
	if typed.Code != http.StatusConflict || roomErrorCode(t, typed) != spaces.CodeRoomMemberChanged {
		t.Fatalf("typed room error: %d %s", typed.Code, typed.Body.String())
	}
	if !strings.Contains(typed.Body.String(), `"current_revision":7`) {
		t.Fatalf("details were dropped: %s", typed.Body.String())
	}
	untyped := run(bytes.ErrTooLarge)
	if untyped.Code != http.StatusServiceUnavailable || roomErrorCode(t, untyped) != spaces.CodeSpaceAuthorityUnavailable {
		t.Fatalf("untyped error: %d %s", untyped.Code, untyped.Body.String())
	}
	if strings.Contains(untyped.Body.String(), bytes.ErrTooLarge.Error()) {
		t.Fatalf("an internal cause leaked into the body: %s", untyped.Body.String())
	}
}
