package http

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// WS05-S8: the SCIM surface and the admin token routes behind the REAL service-auth middleware,
// the REAL membership guard, the REAL gate and the REAL rate limiter, in the route shape
// setupRoutes uses. The service is a fake. Route-table pins run through NewServer.

const (
	scimTestOrg        = "org-scim-1"
	scimTestEdgeToken  = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
	scimTestBearer     = "scim_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	scimTestTokenID    = "7b2f0c6e-1a2b-4c3d-8e4f-500000000002"
	scimTestUserID     = "7b2f0c6e-1a2b-4c3d-8e4f-500000000001"
	scimTestMissingID  = "7b2f0c6e-1a2b-4c3d-8e4f-500000000099"
	scimTestUsersPath  = "/internal/scim/v2/Users"
	scimTestUserPath   = scimTestUsersPath + "/" + scimTestUserID
	scimTestConfigPath = "/internal/scim/v2/ServiceProviderConfig"
	scimTestTypesPath  = "/internal/scim/v2/ResourceTypes"
	scimTestSchemaPath = "/internal/scim/v2/Schemas"
	scimTestGroupsPath = "/internal/scim/v2/Groups"
	scimTestTokensPath = "/orgs/" + scimTestOrg + "/scim/tokens"
	scimTestStatusPath = "/orgs/" + scimTestOrg + "/scim/status"

	scimTestGatewayRegistry = `[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:identity:admin:self"]}
]`
	scimTestFullRegistry = `[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:identity:admin:self"]},
  {"principal":"verevon-scim-edge","audience":"org-core","token":%q,"scopes":["org:scim:any"]}
]`
)

var (
	scimTestNow   = time.Date(2026, time.November, 1, 9, 0, 0, 0, time.UTC)
	scimTestRoles = map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"}
)

func scimTestRegistry() string {
	return fmt.Sprintf(scimTestFullRegistry, modelPolicyGatewayToken, scimTestEdgeToken)
}

// fakeSCIM is a scimService that records calls and answers from canned values.
type fakeSCIM struct {
	mu         sync.Mutex
	calls      []string
	record     orgcore.SCIMUserRecord
	tokenErr   error
	userErr    error
	resolveErr error
	lastFilter *orgcore.SCIMFilter
	lastStart  int
	lastCount  int
	lastInput  orgcore.SCIMUserInput
	lastActive bool
}

func newFakeSCIM() *fakeSCIM {
	return &fakeSCIM{record: orgcore.SCIMUserRecord{
		SCIMID:    scimTestUserID,
		UserName:  "ada@corp.example.com",
		UserID:    "user-ada",
		Active:    true,
		Revision:  1,
		CreatedAt: scimTestNow,
		UpdatedAt: scimTestNow,
	}}
}

func (f *fakeSCIM) note(call string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, call)
}

func (f *fakeSCIM) called(call string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	count := 0
	for _, recorded := range f.calls {
		if recorded == call {
			count++
		}
	}
	return count
}

func (f *fakeSCIM) total() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func (f *fakeSCIM) CreateSCIMToken(_ context.Context, _, actorID, _, name string, _ int) (*orgcore.SCIMTokenIssued, error) {
	f.note("create_token")
	if f.tokenErr != nil {
		return nil, f.tokenErr
	}
	return &orgcore.SCIMTokenIssued{
		Token: orgcore.SCIMToken{
			ID: scimTestTokenID, Name: name, Prefix: "scim_bbbb", CreatedBy: actorID,
			CreatedAt: scimTestNow, ExpiresAt: scimTestNow.Add(30 * 24 * time.Hour),
		},
		BearerToken: scimTestBearer,
	}, nil
}

func (f *fakeSCIM) ListSCIMTokens(_ context.Context, _ string) ([]orgcore.SCIMToken, error) {
	f.note("list_tokens")
	if f.tokenErr != nil {
		return nil, f.tokenErr
	}
	return []orgcore.SCIMToken{{
		ID: scimTestTokenID, Name: "Entra", Prefix: "scim_bbbb", CreatedBy: "u-owner",
		CreatedAt: scimTestNow, ExpiresAt: scimTestNow.Add(30 * 24 * time.Hour),
	}}, nil
}

func (f *fakeSCIM) RevokeSCIMToken(_ context.Context, _, _, _, _ string) error {
	f.note("revoke_token")
	return f.tokenErr
}

func (f *fakeSCIM) GetSCIMStatus(_ context.Context, _ string) (*orgcore.SCIMStatus, error) {
	f.note("status")
	if f.tokenErr != nil {
		return nil, f.tokenErr
	}
	return &orgcore.SCIMStatus{
		Enabled: true, VerifiedDomains: 1, ActiveTokens: 1, MaxTokens: orgcore.MaxActiveSCIMTokens,
		ManagedUsers: 1, ActiveUsers: 1,
	}, nil
}

func (f *fakeSCIM) ResolveSCIMToken(_ context.Context, bearer string) (*orgcore.SCIMTokenAuth, error) {
	f.note("resolve")
	if f.resolveErr != nil {
		return nil, f.resolveErr
	}
	if bearer != scimTestBearer {
		return nil, orgcore.ErrSCIMUnauthorized
	}
	return &orgcore.SCIMTokenAuth{TokenID: scimTestTokenID, OrgID: scimTestOrg, CreatedBy: "u-owner"}, nil
}

func (f *fakeSCIM) ListSCIMUsers(_ context.Context, _ *orgcore.SCIMTokenAuth, filter *orgcore.SCIMFilter, startIndex, count int) ([]orgcore.SCIMUserRecord, int, error) {
	f.note("list_users")
	f.mu.Lock()
	f.lastFilter, f.lastStart, f.lastCount = filter, startIndex, count
	f.mu.Unlock()
	if f.userErr != nil {
		return nil, 0, f.userErr
	}
	return []orgcore.SCIMUserRecord{f.record}, 1, nil
}

func (f *fakeSCIM) GetSCIMUser(_ context.Context, _ *orgcore.SCIMTokenAuth, scimID string) (*orgcore.SCIMUserRecord, error) {
	f.note("get_user")
	if f.userErr != nil {
		return nil, f.userErr
	}
	if scimID != f.record.SCIMID {
		return nil, orgcore.ErrSCIMUserNotFound
	}
	rec := f.record
	return &rec, nil
}

func (f *fakeSCIM) CreateSCIMUser(_ context.Context, _ *orgcore.SCIMTokenAuth, in orgcore.SCIMUserInput) (*orgcore.SCIMUserRecord, error) {
	f.note("create_user")
	f.mu.Lock()
	f.lastInput = in
	f.mu.Unlock()
	if f.userErr != nil {
		return nil, f.userErr
	}
	rec := f.record
	rec.UserName = strings.ToLower(in.UserName)
	rec.ExternalID = in.ExternalID
	rec.Active = in.Active == nil || *in.Active
	return &rec, nil
}

func (f *fakeSCIM) SetSCIMUserActive(_ context.Context, _ *orgcore.SCIMTokenAuth, scimID string, active bool) (*orgcore.SCIMUserRecord, error) {
	f.note("set_active")
	f.mu.Lock()
	f.lastActive = active
	f.mu.Unlock()
	if f.userErr != nil {
		return nil, f.userErr
	}
	if scimID != f.record.SCIMID {
		return nil, orgcore.ErrSCIMUserNotFound
	}
	rec := f.record
	rec.Active = active
	return &rec, nil
}

func (f *fakeSCIM) DeleteSCIMUser(_ context.Context, _ *orgcore.SCIMTokenAuth, _ string) error {
	f.note("delete_user")
	return f.userErr
}

type scimHarness struct {
	router *gin.Engine
	server *Server
	fake   *fakeSCIM
}

// newSCIMHarness mounts the SCIM routes in the shape setupRoutes uses.
func newSCIMHarness(t *testing.T, enabled bool, roles map[string]string) *scimHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(scimTestRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	fake := newFakeSCIM()
	server := &Server{
		callerRoles: fakeCallerRoles{roles: roles},
		scimOn:      enabled,
		scimLimiter: newSCIMRateLimiter(scimRatePerMinute, time.Now),
		scim:        fake,
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	gate := server.requireSCIMEnabled()
	router.GET("/orgs/:id/scim/tokens", gate, server.listSCIMTokens)
	router.POST("/orgs/:id/scim/tokens", gate, guard, server.createSCIMToken)
	router.DELETE("/orgs/:id/scim/tokens/:tokenId", gate, guard, server.revokeSCIMToken)
	router.GET("/orgs/:id/scim/status", gate, server.getSCIMStatus)
	scim := router.Group("/internal/scim/v2", gate, server.scimAuthenticate())
	scim.GET("/ServiceProviderConfig", scimServiceProviderConfig)
	scim.GET("/ResourceTypes", scimResourceTypes)
	scim.GET("/Schemas", scimSchemas)
	scim.GET("/Users", server.scimListUsers)
	scim.POST("/Users", server.scimCreateUser)
	scim.GET("/Users/:id", server.scimGetUser)
	scim.PUT("/Users/:id", server.scimReplaceUser)
	scim.PATCH("/Users/:id", server.scimPatchUser)
	scim.DELETE("/Users/:id", server.scimDeleteUser)
	scim.Any("/Groups", scimNotImplemented)
	scim.Any("/Groups/:id", scimNotImplemented)
	return &scimHarness{router: router, server: server, fake: fake}
}

// edge sends a request the way the gateway's SCIM edge does: the edge principal
// and the IdP's bearer token, nothing that names an organisation.
func (h *scimHarness) edge(method, path string, body []byte, bearer string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	request.Header.Set("X-Service-Id", "verevon-scim-edge")
	request.Header.Set("X-Service-Token", scimTestEdgeToken)
	if bearer != "" {
		request.Header.Set("Authorization", "Bearer "+bearer)
	}
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/scim+json")
	}
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

// admin sends a signed owner or admin request through the gateway delegation.
func (h *scimHarness) admin(method, path, userID string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, scimTestOrg, "")
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func scimTestSchemasOf(t *testing.T, body map[string]any) []string {
	t.Helper()
	raw, _ := body["schemas"].([]any)
	out := make([]string, 0, len(raw))
	for _, item := range raw {
		value, _ := item.(string)
		out = append(out, value)
	}
	return out
}

func TestSCIMFlagIsStrict(t *testing.T) {
	for value, want := range map[string]bool{
		"1": true, "true": true,
		"": false, "0": false, "false": false, "TRUE": false, "yes": false, " 1": false,
	} {
		got := scimEnabled(func(string) string { return value })
		if got != want {
			t.Errorf("ORG_SCIM_ENABLED=%q: enabled=%v, want %v", value, got, want)
		}
	}
}

func TestSCIMEdgeCredentialIsAddedOnlyWhenUsable(t *testing.T) {
	base, err := parseServiceCredentials(fmt.Sprintf(scimTestGatewayRegistry, modelPolicyGatewayToken))
	if err != nil {
		t.Fatalf("parse base registry: %v", err)
	}
	for _, token := range []string{"", "short", modelPolicyGatewayToken} {
		if got := withSCIMEdgeCredential(base, token); len(got) != len(base) {
			t.Fatalf("token %q: the registry has %d credentials, want %d", token, len(got), len(base))
		}
	}
	got := withSCIMEdgeCredential(base, "  "+scimTestEdgeToken+"  ")
	if len(got) != len(base)+1 {
		t.Fatalf("usable token: %d credentials, want %d", len(got), len(base)+1)
	}
	edge := got[len(got)-1]
	if edge.Principal != "verevon-scim-edge" || edge.Audience != serviceCredentialAudience ||
		len(edge.Scopes) != 1 || edge.Scopes[0] != "org:scim:any" {
		t.Fatalf("edge credential = %+v", edge)
	}
	if len(base) != 1 {
		t.Fatalf("withSCIMEdgeCredential changed the registry it was given: %d credentials", len(base))
	}
}

func TestSCIMScopeIsPinnedToTheEdgePrincipal(t *testing.T) {
	cases := []struct {
		principal string
		accepted  bool
	}{
		{"verevon-scim-edge", true},
		{"verevon-gateway", false},
		{"billing-core", false},
		{"auth-core", false},
	}
	for _, tc := range cases {
		raw := `[{"principal":"` + tc.principal + `","audience":"org-core","token":"` +
			strings.Repeat("s", 40) + `","scopes":["org:scim:any"]}]`
		_, err := parseServiceCredentials(raw)
		if (err == nil) != tc.accepted {
			t.Errorf("principal %s holding org:scim:any: err=%v, want accepted=%v", tc.principal, err, tc.accepted)
		}
	}
}

func TestSCIMRoutesAreMountedAndDarkByDefault(t *testing.T) {
	// The environment holds the gateway only: NewServer adds the edge principal
	// itself, and only while ORG_SCIM_ENABLED is on.
	t.Setenv(serviceCredentialEnv, fmt.Sprintf(scimTestGatewayRegistry, modelPolicyGatewayToken))
	t.Setenv(scimFlagEnv, "")
	t.Setenv(scimEdgeTokenEnv, scimTestEdgeToken)

	off := NewServer(0, nil, nil, "", "")
	if off.scimOn {
		t.Fatal("the SCIM routes are on by default")
	}
	want := map[string]bool{
		"GET /orgs/:id/scim/tokens":                   false,
		"POST /orgs/:id/scim/tokens":                  false,
		"DELETE /orgs/:id/scim/tokens/:tokenId":       false,
		"GET /orgs/:id/scim/status":                   false,
		"GET /internal/scim/v2/ServiceProviderConfig": false,
		"GET /internal/scim/v2/ResourceTypes":         false,
		"GET /internal/scim/v2/Schemas":               false,
		"GET /internal/scim/v2/Users":                 false,
		"POST /internal/scim/v2/Users":                false,
		"GET /internal/scim/v2/Users/:id":             false,
		"PUT /internal/scim/v2/Users/:id":             false,
		"PATCH /internal/scim/v2/Users/:id":           false,
		"DELETE /internal/scim/v2/Users/:id":          false,
		"GET /internal/scim/v2/Groups":                false,
	}
	for _, route := range off.router.Routes() {
		if _, ok := want[route.Method+" "+route.Path]; ok {
			want[route.Method+" "+route.Path] = true
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not registered", key)
		}
	}

	// Flag off: the admin routes answer 404 even to a signed owner, and the edge
	// principal does not exist, so the SCIM surface refuses it before any route.
	admin := httptest.NewRequest(http.MethodGet, scimTestStatusPath, bytes.NewReader(nil))
	signModelPolicyDelegation(admin, nil, "u-owner", scimTestOrg, "")
	response := httptest.NewRecorder()
	off.router.ServeHTTP(response, admin)
	if response.Code != http.StatusNotFound {
		t.Fatalf("flag off, admin status: %d body=%s; want 404", response.Code, response.Body.String())
	}
	edge := httptest.NewRequest(http.MethodGet, scimTestUsersPath, bytes.NewReader(nil))
	edge.Header.Set("X-Service-Id", "verevon-scim-edge")
	edge.Header.Set("X-Service-Token", scimTestEdgeToken)
	edge.Header.Set("Authorization", "Bearer "+scimTestBearer)
	response = httptest.NewRecorder()
	off.router.ServeHTTP(response, edge)
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("flag off, edge principal: status=%d body=%s; want 401 (the principal is not registered)", response.Code, response.Body.String())
	}

	// Flag on without an auth-core credential: the edge is registered, the surface
	// answers 503 in the SCIM error shape, and the gateway principal is refused.
	t.Setenv(scimFlagEnv, "1")
	on := NewServer(0, nil, nil, "", "")
	if !on.scimOn {
		t.Fatal("ORG_SCIM_ENABLED=1 did not switch the routes on")
	}
	if on.scim != nil {
		t.Fatal("without an org-core service the SCIM surface must have no service")
	}
	edge = httptest.NewRequest(http.MethodGet, scimTestUsersPath, bytes.NewReader(nil))
	edge.Header.Set("X-Service-Id", "verevon-scim-edge")
	edge.Header.Set("X-Service-Token", scimTestEdgeToken)
	edge.Header.Set("Authorization", "Bearer "+scimTestBearer)
	response = httptest.NewRecorder()
	on.router.ServeHTTP(response, edge)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("flag on, no service: status=%d body=%s; want 503", response.Code, response.Body.String())
	}
	if schemas := scimTestSchemasOf(t, decodeBody(t, response)); len(schemas) != 1 || schemas[0] != scimSchemaError {
		t.Fatalf("503 is not a SCIM error: schemas=%v", schemas)
	}

	gateway := httptest.NewRequest(http.MethodGet, scimTestUsersPath, bytes.NewReader(nil))
	gateway.Header.Set("X-Service-Id", "verevon-gateway")
	gateway.Header.Set("X-Service-Token", modelPolicyGatewayToken)
	gateway.Header.Set("Authorization", "Bearer "+scimTestBearer)
	response = httptest.NewRecorder()
	on.router.ServeHTTP(response, gateway)
	if response.Code != http.StatusForbidden {
		t.Fatalf("gateway principal on the SCIM surface: status=%d body=%s; want 403", response.Code, response.Body.String())
	}
}

func TestSCIMGateAnswers404ForEveryRouteWhenOff(t *testing.T) {
	h := newSCIMHarness(t, false, scimTestRoles)
	for _, path := range []string{scimTestTokensPath, scimTestStatusPath} {
		if response := h.admin(http.MethodGet, path, "u-owner", nil); response.Code != http.StatusNotFound {
			t.Errorf("admin GET %s with the flag off: status=%d; want 404", path, response.Code)
		}
	}
	if response := h.edge(http.MethodGet, scimTestUsersPath, nil, scimTestBearer); response.Code != http.StatusNotFound {
		t.Errorf("SCIM GET Users with the flag off: status=%d; want 404", response.Code)
	}
	if h.fake.total() != 0 {
		t.Fatalf("the service was reached %d times with the flag off", h.fake.total())
	}
}

func TestSCIMAdminRoutesAreOwnerAndAdminOnly(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	body := []byte(`{"name":"Entra","expires_in_days":30}`)
	checks := []struct {
		method, path string
		body         []byte
	}{
		{http.MethodGet, scimTestTokensPath, nil},
		{http.MethodPost, scimTestTokensPath, body},
		{http.MethodDelete, scimTestTokensPath + "/" + scimTestTokenID, nil},
		{http.MethodGet, scimTestStatusPath, nil},
	}
	for _, user := range []string{"u-member", "u-viewer", "u-stranger"} {
		for _, tc := range checks {
			response := h.admin(tc.method, tc.path, user, tc.body)
			if response.Code != http.StatusForbidden {
				t.Fatalf("%s %s as %s: status=%d body=%s; want 403", tc.method, tc.path, user, response.Code, response.Body.String())
			}
		}
	}
	if h.fake.total() != 0 {
		t.Fatalf("a refused caller reached the service %d times", h.fake.total())
	}
	for _, user := range []string{"u-owner", "u-admin"} {
		if response := h.admin(http.MethodGet, scimTestStatusPath, user, nil); response.Code != http.StatusOK {
			t.Fatalf("status as %s: %d body=%s; want 200", user, response.Code, response.Body.String())
		}
	}
}

func TestSCIMTokenSecretIsShownOnceAndNeverListed(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	created := h.admin(http.MethodPost, scimTestTokensPath, "u-owner", []byte(`{"name":"Entra","expires_in_days":30}`))
	if created.Code != http.StatusCreated {
		t.Fatalf("create: status=%d body=%s; want 201", created.Code, created.Body.String())
	}
	data, _ := decodeBody(t, created)["data"].(map[string]any)
	bearer, _ := data["bearer_token"].(string)
	if !strings.HasPrefix(bearer, "scim_") {
		t.Fatalf("bearer_token = %q; want the scim_ prefix", bearer)
	}

	listed := h.admin(http.MethodGet, scimTestTokensPath, "u-owner", nil)
	if listed.Code != http.StatusOK {
		t.Fatalf("list: status=%d body=%s", listed.Code, listed.Body.String())
	}
	if strings.Contains(listed.Body.String(), bearer) || strings.Contains(listed.Body.String(), "bearer_token") {
		t.Fatalf("the token listing leaks the secret: %s", listed.Body.String())
	}
	listedData, _ := decodeBody(t, listed)["data"].(map[string]any)
	tokens, _ := listedData["tokens"].([]any)
	if len(tokens) != 1 {
		t.Fatalf("listed %d tokens, want 1", len(tokens))
	}
}

func TestSCIMTokenErrorsAreTypedEnvelopes(t *testing.T) {
	cases := []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{"validation", &orgcore.SCIMRequestError{Reason: "name"}, http.StatusBadRequest, "invalid_request"},
		{"no verified domain", orgcore.ErrSCIMNoVerifiedDomain, http.StatusUnprocessableEntity, "no_verified_domain"},
		{"token limit", orgcore.ErrSCIMTokenLimit, http.StatusConflict, "scim_token_limit"},
		{"unavailable", orgcore.ErrSCIMUnavailable, http.StatusServiceUnavailable, "scim_unavailable"},
		{"unexpected", errors.New("database offline"), http.StatusInternalServerError, "internal_error"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newSCIMHarness(t, true, scimTestRoles)
			h.fake.tokenErr = tc.err
			response := h.admin(http.MethodPost, scimTestTokensPath, "u-owner", []byte(`{"name":"Entra","expires_in_days":30}`))
			if response.Code != tc.status {
				t.Fatalf("status=%d body=%s; want %d", response.Code, response.Body.String(), tc.status)
			}
			code, _ := envelopeCodeAndDetails(t, response)
			if code != tc.code {
				t.Fatalf("code=%q, want %q", code, tc.code)
			}
			if strings.Contains(response.Body.String(), "database offline") {
				t.Fatal("an internal error leaks its cause to the client")
			}
		})
	}
}

func TestSCIMRevokeOfAnUnknownTokenIsNotFound(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	h.fake.tokenErr = orgcore.ErrSCIMTokenNotFound
	response := h.admin(http.MethodDelete, scimTestTokensPath+"/"+scimTestMissingID, "u-owner", nil)
	if response.Code != http.StatusNotFound {
		t.Fatalf("status=%d body=%s; want 404", response.Code, response.Body.String())
	}
	if code, _ := envelopeCodeAndDetails(t, response); code != "scim_token_not_found" {
		t.Fatalf("code=%q, want scim_token_not_found", code)
	}
}

func TestSCIMSurfaceRefusesMissingOrForeignBearers(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	for _, bearer := range []string{"", "not-a-scim-token", "scim_unknown"} {
		response := h.edge(http.MethodGet, scimTestUsersPath, nil, bearer)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("bearer %q: status=%d; want 401", bearer, response.Code)
		}
		if got := response.Header().Get("Content-Type"); got != scimContentType {
			t.Fatalf("bearer %q: content type %q; want %q", bearer, got, scimContentType)
		}
		if response.Header().Get("WWW-Authenticate") == "" {
			t.Fatalf("bearer %q: no WWW-Authenticate challenge", bearer)
		}
		body := decodeBody(t, response)
		if body["status"] != "401" {
			t.Fatalf("bearer %q: status field = %v; want \"401\"", bearer, body["status"])
		}
		if schemas := scimTestSchemasOf(t, body); len(schemas) != 1 || schemas[0] != scimSchemaError {
			t.Fatalf("bearer %q: schemas=%v; want the SCIM Error schema", bearer, schemas)
		}
	}
	if h.fake.called("list_users") != 0 {
		t.Fatal("a refused bearer reached the user list")
	}
}

func TestSCIMRateLimitAnswers429AndRecovers(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	current := scimTestNow
	h.server.scimLimiter = newSCIMRateLimiter(2, func() time.Time { return current })
	for i := 1; i <= 2; i++ {
		if response := h.edge(http.MethodGet, scimTestConfigPath, nil, scimTestBearer); response.Code != http.StatusOK {
			t.Fatalf("request %d: status=%d; want 200", i, response.Code)
		}
	}
	limited := h.edge(http.MethodGet, scimTestConfigPath, nil, scimTestBearer)
	if limited.Code != http.StatusTooManyRequests {
		t.Fatalf("third request: status=%d; want 429", limited.Code)
	}
	retry, err := strconv.Atoi(limited.Header().Get("Retry-After"))
	if err != nil || retry < 1 || retry > 61 {
		t.Fatalf("Retry-After = %q; want 1 to 61 seconds", limited.Header().Get("Retry-After"))
	}
	current = scimTestNow.Add(time.Minute)
	if response := h.edge(http.MethodGet, scimTestConfigPath, nil, scimTestBearer); response.Code != http.StatusOK {
		t.Fatalf("after the window: status=%d; want 200", response.Code)
	}
}

func TestSCIMDiscoveryDocumentsAreStaticAndHonest(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	config := h.edge(http.MethodGet, scimTestConfigPath, nil, scimTestBearer)
	if config.Code != http.StatusOK {
		t.Fatalf("ServiceProviderConfig: status=%d body=%s", config.Code, config.Body.String())
	}
	doc := decodeBody(t, config)
	for key, want := range map[string]bool{"patch": true, "filter": true, "bulk": false, "changePassword": false, "sort": false, "etag": false} {
		section, _ := doc[key].(map[string]any)
		if section["supported"] != want {
			t.Errorf("%s.supported = %v, want %v", key, section["supported"], want)
		}
	}
	if filter, _ := doc["filter"].(map[string]any); filter["maxResults"] != float64(100) {
		t.Errorf("filter.maxResults = %v, want 100", filter["maxResults"])
	}

	types := decodeBody(t, h.edge(http.MethodGet, scimTestTypesPath, nil, scimTestBearer))
	resources, _ := types["Resources"].([]any)
	if len(resources) != 1 {
		t.Fatalf("ResourceTypes lists %d resources, want 1", len(resources))
	}
	if first, _ := resources[0].(map[string]any); first["endpoint"] != "/Users" {
		t.Fatalf("ResourceTypes endpoint = %v, want /Users", first["endpoint"])
	}
	if schemas := h.edge(http.MethodGet, scimTestSchemaPath, nil, scimTestBearer); schemas.Code != http.StatusOK {
		t.Fatalf("Schemas: status=%d", schemas.Code)
	}

	for _, method := range []string{http.MethodGet, http.MethodPost} {
		response := h.edge(method, scimTestGroupsPath, []byte(`{}`), scimTestBearer)
		if response.Code != http.StatusNotImplemented {
			t.Fatalf("%s Groups: status=%d; want 501", method, response.Code)
		}
		if body := decodeBody(t, response); body["scimType"] != nil {
			t.Fatalf("%s Groups: unexpected scimType %v", method, body["scimType"])
		}
	}
}

func TestSCIMCreateUserReturnsLocationAndIdentityKeysOnly(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	body := []byte(`{"schemas":["urn:ietf:params:scim:schemas:core:2.0:User"],` +
		`"userName":"Ada@Corp.Example.com","externalId":"ext-1","active":"True",` +
		`"displayName":"Ada Lovelace","name":{"givenName":"Ada"},` +
		`"roles":[{"value":"owner"}],"groups":[{"value":"admins"}]}`)
	response := h.edge(http.MethodPost, scimTestUsersPath, body, scimTestBearer)
	if response.Code != http.StatusCreated {
		t.Fatalf("status=%d body=%s; want 201", response.Code, response.Body.String())
	}
	if location := response.Header().Get("Location"); location != scimUsersLocation+scimTestUserID {
		t.Fatalf("Location = %q, want %q", location, scimUsersLocation+scimTestUserID)
	}
	doc := decodeBody(t, response)
	if doc["userName"] != "ada@corp.example.com" || doc["active"] != true || doc["externalId"] != "ext-1" {
		t.Fatalf("resource = %v", doc)
	}
	for _, key := range []string{"displayName", "name", "roles", "groups"} {
		if _, present := doc[key]; present {
			t.Fatalf("the SCIM response echoes %q; only the identity keys are kept", key)
		}
	}
	if h.fake.lastInput.UserName != "Ada@Corp.Example.com" || h.fake.lastInput.Active == nil || !*h.fake.lastInput.Active {
		t.Fatalf("service input = %+v", h.fake.lastInput)
	}
}

func TestSCIMCreateUserMapsEachFailureToAScimError(t *testing.T) {
	cases := []struct {
		name       string
		body       string
		userErr    error
		status     int
		scimType   string
		retryAfter string
	}{
		{"malformed json", `{"userName":`, nil, http.StatusBadRequest, "invalidSyntax", ""},
		{"active not boolean", `{"userName":"ada@corp.example.com","active":"maybe"}`, nil, http.StatusBadRequest, "invalidValue", ""},
		{"uniqueness", `{"userName":"ada@corp.example.com"}`, orgcore.ErrSCIMUniqueness, http.StatusConflict, "uniqueness", ""},
		{"domain not verified", `{"userName":"ada@corp.example.com"}`, orgcore.ErrSCIMDomainNotVerified, http.StatusBadRequest, "invalidValue", ""},
		{"bad userName", `{"userName":"nope"}`, &orgcore.SCIMRequestError{Reason: "userName"}, http.StatusBadRequest, "invalidValue", ""},
		{"membership conflict", `{"userName":"ada@corp.example.com"}`, orgcore.ErrSCIMMembershipConflict, http.StatusServiceUnavailable, "", "5"},
		{"auth-core unavailable", `{"userName":"ada@corp.example.com"}`, orgcore.ErrSSOUnavailable, http.StatusServiceUnavailable, "", "30"},
		{"unexpected", `{"userName":"ada@corp.example.com"}`, errors.New("pq: connection refused"), http.StatusInternalServerError, "", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newSCIMHarness(t, true, scimTestRoles)
			h.fake.userErr = tc.userErr
			response := h.edge(http.MethodPost, scimTestUsersPath, []byte(tc.body), scimTestBearer)
			if response.Code != tc.status {
				t.Fatalf("status=%d body=%s; want %d", response.Code, response.Body.String(), tc.status)
			}
			body := decodeBody(t, response)
			if got, _ := body["scimType"].(string); got != tc.scimType {
				t.Fatalf("scimType=%q, want %q", got, tc.scimType)
			}
			if got := response.Header().Get("Retry-After"); got != tc.retryAfter {
				t.Fatalf("Retry-After=%q, want %q", got, tc.retryAfter)
			}
			if strings.Contains(response.Body.String(), "connection refused") {
				t.Fatal("an internal error leaks its cause to the client")
			}
		})
	}
}

func TestSCIMPatchAndPutDriveActiveThroughTheService(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	patchOpen := `{"schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],"Operations":[`

	// Deactivate with the IdP's path-less object form.
	response := h.edge(http.MethodPatch, scimTestUserPath,
		[]byte(patchOpen+`{"op":"replace","value":{"active":false}}]}`), scimTestBearer)
	if response.Code != http.StatusOK || decodeBody(t, response)["active"] != false {
		t.Fatalf("deactivate: status=%d body=%s", response.Code, response.Body.String())
	}
	if h.fake.called("set_active") != 1 || h.fake.lastActive {
		t.Fatalf("deactivate did not reach the service with active=false (calls=%d, active=%v)", h.fake.called("set_active"), h.fake.lastActive)
	}

	// Reactivate with the case-insensitive op and a string boolean.
	response = h.edge(http.MethodPatch, scimTestUserPath,
		[]byte(patchOpen+`{"op":"Replace","path":"active","value":"True"}]}`), scimTestBearer)
	if response.Code != http.StatusOK || decodeBody(t, response)["active"] != true {
		t.Fatalf("reactivate: status=%d body=%s", response.Code, response.Body.String())
	}
	if h.fake.called("set_active") != 2 || !h.fake.lastActive {
		t.Fatalf("reactivate did not reach the service with active=true")
	}

	// Any other attribute is accepted and ignored: the identity key does not move.
	response = h.edge(http.MethodPatch, scimTestUserPath,
		[]byte(patchOpen+`{"op":"replace","path":"userName","value":"other@corp.example.com"}]}`), scimTestBearer)
	if response.Code != http.StatusOK {
		t.Fatalf("userName change: status=%d body=%s; want 200", response.Code, response.Body.String())
	}
	if h.fake.called("set_active") != 2 || h.fake.called("get_user") == 0 {
		t.Fatal("a userName change must read the user and leave the active state alone")
	}

	response = h.edge(http.MethodPut, scimTestUserPath, []byte(`{"active":false}`), scimTestBearer)
	if response.Code != http.StatusOK || h.fake.called("set_active") != 3 || h.fake.lastActive {
		t.Fatalf("PUT active=false: status=%d body=%s", response.Code, response.Body.String())
	}
	response = h.edge(http.MethodPut, scimTestUserPath, []byte(`{"userName":"other@corp.example.com"}`), scimTestBearer)
	if response.Code != http.StatusOK || h.fake.called("set_active") != 3 {
		t.Fatalf("PUT without active: status=%d, set_active calls=%d", response.Code, h.fake.called("set_active"))
	}
	response = h.edge(http.MethodPut, scimTestUserPath, []byte(`{"active":"maybe"}`), scimTestBearer)
	if response.Code != http.StatusBadRequest || h.fake.called("set_active") != 3 {
		t.Fatalf("PUT with a bad active: status=%d, set_active calls=%d", response.Code, h.fake.called("set_active"))
	}
	if body := decodeBody(t, response); body["scimType"] != "invalidValue" {
		t.Fatalf("PUT with a bad active: scimType=%v", body["scimType"])
	}
}

func TestSCIMDeleteAnswers204AndMapsTheLastOwnerRefusal(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	response := h.edge(http.MethodDelete, scimTestUserPath, nil, scimTestBearer)
	if response.Code != http.StatusNoContent || response.Body.Len() != 0 {
		t.Fatalf("delete: status=%d body=%q; want an empty 204", response.Code, response.Body.String())
	}
	if h.fake.called("delete_user") != 1 {
		t.Fatalf("delete reached the service %d times", h.fake.called("delete_user"))
	}

	h.fake.userErr = orgcore.ErrSCIMLastOwner
	conflict := h.edge(http.MethodDelete, scimTestUserPath, nil, scimTestBearer)
	if conflict.Code != http.StatusConflict {
		t.Fatalf("last owner: status=%d body=%s; want 409", conflict.Code, conflict.Body.String())
	}
	if body := decodeBody(t, conflict); body["scimType"] != "mutability" {
		t.Fatalf("last owner: scimType=%v; want mutability", body["scimType"])
	}
}

func TestSCIMGetUserHidesTombstonesAndUnknownIds(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	if response := h.edge(http.MethodGet, scimTestUserPath, nil, scimTestBearer); response.Code != http.StatusOK {
		t.Fatalf("live user: status=%d body=%s", response.Code, response.Body.String())
	}
	if response := h.edge(http.MethodGet, scimTestUsersPath+"/"+scimTestMissingID, nil, scimTestBearer); response.Code != http.StatusNotFound {
		t.Fatalf("unknown id: status=%d; want 404", response.Code)
	}
	deleted := scimTestNow
	h.fake.record.DeletedAt = &deleted
	response := h.edge(http.MethodGet, scimTestUserPath, nil, scimTestBearer)
	if response.Code != http.StatusNotFound {
		t.Fatalf("tombstone: status=%d; want 404", response.Code)
	}
	if body := decodeBody(t, response); body["scimType"] != nil && body["scimType"] != "" {
		t.Fatalf("tombstone: unexpected scimType %v", body["scimType"])
	}
}

func TestSCIMListFiltersAndPagesThroughTheService(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	query := url.Values{"filter": {`userName eq "ada@corp.example.com"`}, "startIndex": {"3"}, "count": {"500"}}
	response := h.edge(http.MethodGet, scimTestUsersPath+"?"+query.Encode(), nil, scimTestBearer)
	if response.Code != http.StatusOK {
		t.Fatalf("list: status=%d body=%s", response.Code, response.Body.String())
	}
	if h.fake.lastFilter == nil || h.fake.lastFilter.Attribute != "userName" || h.fake.lastFilter.Value != "ada@corp.example.com" {
		t.Fatalf("filter = %+v", h.fake.lastFilter)
	}
	if h.fake.lastStart != 3 || h.fake.lastCount != 500 {
		t.Fatalf("paging = start %d count %d; want 3 and 500", h.fake.lastStart, h.fake.lastCount)
	}
	doc := decodeBody(t, response)
	if doc["totalResults"] != float64(1) || doc["startIndex"] != float64(3) || doc["itemsPerPage"] != float64(1) {
		t.Fatalf("list envelope = %v", doc)
	}
	if resources, _ := doc["Resources"].([]any); len(resources) != 1 {
		t.Fatalf("Resources has %d entries, want 1", len(resources))
	}
}

func TestSCIMListRefusesFiltersOtherThanUserNameAndExternalId(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	for _, filter := range []string{`title eq "x"`, `userName co "x"`, `userName eq ada@corp.example.com`} {
		query := url.Values{"filter": {filter}}
		response := h.edge(http.MethodGet, scimTestUsersPath+"?"+query.Encode(), nil, scimTestBearer)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("filter %q: status=%d body=%s; want 400", filter, response.Code, response.Body.String())
		}
		if body := decodeBody(t, response); body["scimType"] != "invalidFilter" {
			t.Fatalf("filter %q: scimType=%v; want invalidFilter", filter, body["scimType"])
		}
	}
	if h.fake.called("list_users") != 0 {
		t.Fatal("a refused filter reached the service")
	}
}

func TestSCIMWithoutAnOrgCoreServiceAnswers503(t *testing.T) {
	h := newSCIMHarness(t, true, scimTestRoles)
	h.server.scim = nil
	if response := h.edge(http.MethodGet, scimTestUsersPath, nil, scimTestBearer); response.Code != http.StatusServiceUnavailable {
		t.Fatalf("SCIM without a service: status=%d; want 503", response.Code)
	}
	if response := h.admin(http.MethodGet, scimTestStatusPath, "u-owner", nil); response.Code != http.StatusServiceUnavailable {
		t.Fatalf("admin without a service: status=%d; want 503", response.Code)
	}
}
