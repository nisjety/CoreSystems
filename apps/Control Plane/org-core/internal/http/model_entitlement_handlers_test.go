package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/modelentitlement"
	"github.com/gin-gonic/gin"
)

const (
	modelPolicyGatewayToken      = "ggggggggggggggggggggggggggggggggggg"
	modelPolicyInferenceToken    = "iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii"
	modelPolicyModelGatewayToken = "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm"
	modelPolicyBillingToken      = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	modelPolicyOrg               = "org-policy-1"
	modelPolicyRegistryTemplate  = `[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]},
  {"principal":"inference-core","audience":"org-core","token":%q,"scopes":["org:model-entitlement:read:any"]},
  {"principal":"model-gateway","audience":"org-core","token":%q,"scopes":["org:read:any","org:model-entitlement:read:any"]},
  {"principal":"billing-core","audience":"org-core","token":%q,"scopes":["org:read:any"]}
]`
)

var modelPolicyNonce atomic.Int64

// memoryModelPolicyStore is a Store with the repository's revision compare and
// set, so the real service and the real handlers run without a database.
type memoryModelPolicyStore struct {
	mu     sync.Mutex
	set    modelentitlement.PolicySet
	writes int
	err    error
}

func (m *memoryModelPolicyStore) Get(_ context.Context, orgID string) (*modelentitlement.PolicySet, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.err != nil {
		return nil, m.err
	}
	copied := m.set
	copied.OrganizationID = orgID
	if copied.Roles == nil {
		copied.Roles = map[string]modelentitlement.Policy{}
	}
	return &copied, nil
}

func (m *memoryModelPolicyStore) Put(_ context.Context, orgID, actorID string, expected int64, req modelentitlement.PutRequest) (*modelentitlement.PolicySet, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if expected != m.set.Revision {
		return nil, &modelentitlement.RevisionConflictError{Current: m.set.Revision}
	}
	m.writes++
	now := time.Now().UTC()
	m.set = modelentitlement.PolicySet{OrganizationID: orgID, Revision: expected + 1, OrgDefault: req.OrgDefault, Roles: req.Roles, UpdatedAt: &now, UpdatedBy: actorID}
	copied := m.set
	return &copied, nil
}

type fakeCallerRoles struct {
	roles map[string]string
	err   error
}

func (f fakeCallerRoles) CallerRole(_ context.Context, _, userID string) (string, error) {
	return f.roles[userID], f.err
}

type modelPolicyMembers map[string]string

func (m modelPolicyMembers) IsActiveMember(_ context.Context, _, userID string) (bool, error) {
	return m[userID] != "", nil
}

// fakeResolver is a modelEntitlementService that records what the route asked
// for and answers with a canned resolution or error.
type fakeResolver struct {
	mu         sync.Mutex
	resolution modelentitlement.Resolution
	err        error
	calls      int
	gotOrg     string
	gotUser    string
}

func (f *fakeResolver) Get(context.Context, string) (*modelentitlement.PolicySet, error) {
	return nil, errors.New("not used")
}

func (f *fakeResolver) Put(context.Context, string, string, modelentitlement.PutRequest) (*modelentitlement.PolicySet, error) {
	return nil, errors.New("not used")
}

func (f *fakeResolver) Resolve(_ context.Context, orgID, userID string) (modelentitlement.Resolution, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	f.gotOrg, f.gotUser = orgID, userID
	return f.resolution, f.err
}

func (f *fakeResolver) snapshot() (calls int, org, user string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls, f.gotOrg, f.gotUser
}

func modelPolicyRegistry() string {
	return fmt.Sprintf(modelPolicyRegistryTemplate, modelPolicyGatewayToken, modelPolicyInferenceToken, modelPolicyModelGatewayToken, modelPolicyBillingToken)
}

// modelPolicyServer builds org-core's real server (real route table, real
// service-auth middleware) with the model policy service replaced by a fake.
func modelPolicyServer(t *testing.T, service modelEntitlementService) *Server {
	t.Helper()
	t.Setenv(serviceCredentialEnv, modelPolicyRegistry())
	server := NewServer(0, nil, nil, "", "")
	server.SetModelEntitlements(service)
	return server
}

func signModelPolicyDelegation(request *http.Request, body []byte, userID, orgID, platformRole string) {
	if platformRole == "" {
		// The gateway always forwards a role; an ordinary signed-in user carries "user".
		platformRole = "user"
	}
	digest := serviceDelegationBodyDigest(body)
	claims := serviceDelegationClaims{
		Principal:  gatewayServicePrincipal,
		Audience:   serviceCredentialAudience,
		Timestamp:  time.Now().UTC().Format(time.RFC3339),
		Nonce:      fmt.Sprintf("model-policy-nonce-%d", modelPolicyNonce.Add(1)),
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     userID,
		OrgID:      orgID,
		UserRole:   platformRole,
		BodySHA256: digest,
	}
	request.Header.Set("X-Service-Id", claims.Principal)
	request.Header.Set("X-Service-Token", modelPolicyGatewayToken)
	request.Header.Set("X-User-Id", claims.UserID)
	request.Header.Set("X-Org-Id", claims.OrgID)
	request.Header.Set("X-User-Role", claims.UserRole)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", claims.Timestamp)
	request.Header.Set("X-Delegation-Nonce", claims.Nonce)
	request.Header.Set("X-Delegation-Body-SHA256", digest)
	request.Header.Set("X-Delegation-Signature", serviceDelegationSignature(modelPolicyGatewayToken, claims))
}

// modelPolicyAdminHarness mounts the model policy handlers behind the REAL
// service-auth middleware and the REAL membership guard, with a memory store
// behind the REAL service. The PUT route cannot run through NewServer's own
// router without a database (its guard reads org-core's membership table), so
// the route table itself is pinned by TestModelEntitlementRoutesAreMounted.
type modelPolicyAdminHarness struct {
	router *gin.Engine
	store  *memoryModelPolicyStore
}

func newModelPolicyAdminHarness(t *testing.T, roles map[string]string) *modelPolicyAdminHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(modelPolicyRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryModelPolicyStore{}
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}}
	server.SetModelEntitlements(modelentitlement.NewService(store, fakeCallerRoles{roles: roles}))

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	router.GET("/api/v1/organizations/:id/model-entitlement", server.getModelEntitlement)
	router.PUT("/api/v1/organizations/:id/model-entitlement", guard, server.putModelEntitlement)
	router.GET("/api/v1/organizations/:id/model-entitlement/explain", server.explainModelEntitlement)
	router.GET("/orgs/:id/model-entitlement", server.getModelEntitlement)
	router.PUT("/orgs/:id/model-entitlement", guard, server.putModelEntitlement)
	router.GET("/orgs/:id/model-entitlement/explain", server.explainModelEntitlement)
	return &modelPolicyAdminHarness{router: router, store: store}
}

func (h *modelPolicyAdminHarness) do(method, path, userID, platformRole string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, modelPolicyOrg, platformRole)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

const modelPolicyV1Path = "/api/v1/organizations/" + modelPolicyOrg + "/model-entitlement"

func validModelPolicyBody(expected int) []byte {
	return []byte(fmt.Sprintf(`{"expected_revision":%d,"org_default":{"allow_mode":"listed","allow":["GPT-5-Mini","provider:anthropic"],"deny":["claude-opus-4-8"],"max_effort":"high","defaults":{"chat":{"model":"gpt-5-mini","effort":"medium"}}},"roles":{"Viewer":{"allow_mode":"listed","allow":["gpt-5-mini"]}}}`, expected))
}

func decodeBody(t *testing.T, response *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode %q: %v", response.Body.String(), err)
	}
	return body
}

func TestModelEntitlementRoutesAreMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	want := map[string]bool{
		"GET /api/v1/organizations/:id/model-entitlement":         false,
		"PUT /api/v1/organizations/:id/model-entitlement":         false,
		"GET /api/v1/organizations/:id/model-entitlement/explain": false,
		"GET /orgs/:id/model-entitlement":                         false,
		"PUT /orgs/:id/model-entitlement":                         false,
		"GET /orgs/:id/model-entitlement/explain":                 false,
		"GET /internal/orgs/:orgId/model-entitlement":             false,
	}
	for _, route := range server.router.Routes() {
		key := route.Method + " " + route.Path
		if _, ok := want[key]; ok {
			want[key] = true
		}
		if route.Method != http.MethodGet && route.Method != http.MethodPut && strings.Contains(route.Path, "model-entitlement") {
			t.Fatalf("unexpected model-entitlement route %s", key)
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}

func TestMemberCannotWriteTheModelPolicyButOwnerAndAdminCan(t *testing.T) {
	harness := newModelPolicyAdminHarness(t, map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"})

	// A plain member is refused, whatever role header the gateway forwarded: the
	// header is not authority, the membership table is.
	for _, forwarded := range []string{"", "admin", "owner", "superadmin"} {
		response := harness.do(http.MethodPut, modelPolicyV1Path, "u-member", forwarded, validModelPolicyBody(0))
		if response.Code != http.StatusForbidden {
			t.Fatalf("member with forwarded role %q: status=%d body=%s; want 403", forwarded, response.Code, response.Body.String())
		}
	}
	if response := harness.do(http.MethodPut, modelPolicyV1Path, "u-viewer", "", validModelPolicyBody(0)); response.Code != http.StatusForbidden {
		t.Fatalf("viewer: status=%d; want 403", response.Code)
	}
	if response := harness.do(http.MethodPut, modelPolicyV1Path, "u-stranger", "admin", validModelPolicyBody(0)); response.Code != http.StatusForbidden {
		t.Fatalf("non-member: status=%d; want 403 from the membership guard", response.Code)
	}
	if harness.store.writes != 0 {
		t.Fatalf("a refused caller wrote the policy %d times", harness.store.writes)
	}

	owner := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", validModelPolicyBody(0))
	if owner.Code != http.StatusOK {
		t.Fatalf("owner: status=%d body=%s", owner.Code, owner.Body.String())
	}
	body := decodeBody(t, owner)
	if body["revision"] != float64(1) || body["updated_by"] != "u-owner" || body["organization_id"] != modelPolicyOrg {
		t.Fatalf("owner write answered %v", body)
	}
	if owner.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("a policy answer must be no-store, got %q", owner.Header().Get("Cache-Control"))
	}

	admin := harness.do(http.MethodPut, "/orgs/"+modelPolicyOrg+"/model-entitlement", "u-admin", "", validModelPolicyBody(1))
	if admin.Code != http.StatusOK || decodeBody(t, admin)["revision"] != float64(2) || harness.store.set.UpdatedBy != "u-admin" {
		t.Fatalf("admin through the proxy route: status=%d body=%s", admin.Code, admin.Body.String())
	}

	// The stored policy is the normalised one: lower-cased and sorted.
	stored := harness.store.set
	if stored.OrgDefault == nil || stored.OrgDefault.Allow[0] != "gpt-5-mini" || stored.OrgDefault.Allow[1] != "provider:anthropic" {
		t.Fatalf("stored policy was not normalised: %+v", stored.OrgDefault)
	}
	if _, ok := stored.Roles["viewer"]; !ok {
		t.Fatalf("role keys must be lower-cased, got %v", stored.Roles)
	}
}

func TestModelPolicyWriteWithAStaleRevisionIs409(t *testing.T) {
	harness := newModelPolicyAdminHarness(t, map[string]string{"u-owner": "owner"})
	if response := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", validModelPolicyBody(0)); response.Code != http.StatusOK {
		t.Fatalf("first write: %d %s", response.Code, response.Body.String())
	}
	stale := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", validModelPolicyBody(0))
	if stale.Code != http.StatusConflict {
		t.Fatalf("stale write: status=%d body=%s; want 409", stale.Code, stale.Body.String())
	}
	body := decodeBody(t, stale)
	if body["code"] != "revision_conflict" || body["current_revision"] != float64(1) {
		t.Fatalf("a conflict must name the revision to re-read: %v", body)
	}
	if harness.store.writes != 1 {
		t.Fatalf("the stale write must not be applied, writes=%d", harness.store.writes)
	}
}

func TestModelPolicyWriteRejectsBadBodies(t *testing.T) {
	harness := newModelPolicyAdminHarness(t, map[string]string{"u-owner": "owner"})
	cases := []struct {
		name       string
		body       string
		wantStatus int
		wantCode   string
	}{
		{"not json", `not json`, http.StatusBadRequest, "invalid_body"},
		{"empty body", ``, http.StatusBadRequest, "invalid_body"},
		{"unknown field", `{"expected_revision":0,"org_default":null,"roles":{},"extra":true}`, http.StatusBadRequest, "invalid_body"},
		{"two documents", `{"expected_revision":0} {"expected_revision":0}`, http.StatusBadRequest, "invalid_body"},
		{"missing expected_revision", `{"org_default":null,"roles":{}}`, http.StatusBadRequest, "invalid_model_policy"},
		{"glob entry", `{"expected_revision":0,"org_default":{"allow_mode":"listed","allow":["gpt-*"],"deny":[]},"roles":{}}`, http.StatusBadRequest, "invalid_model_policy"},
		{"too large", `{"expected_revision":0,"org_default":null,"roles":{},"pad":"` + strings.Repeat("x", modelentitlement.MaxBodyBytes+1) + `"}`, http.StatusRequestEntityTooLarge, "body_too_large"},
	}
	for _, tc := range cases {
		response := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", []byte(tc.body))
		if response.Code != tc.wantStatus {
			t.Errorf("%s: status=%d body=%s; want %d", tc.name, response.Code, response.Body.String(), tc.wantStatus)
			continue
		}
		if got := decodeBody(t, response)["code"]; got != tc.wantCode {
			t.Errorf("%s: code=%v; want %s", tc.name, got, tc.wantCode)
		}
	}
	if harness.store.writes != 0 {
		t.Fatalf("a refused body must write nothing, writes=%d", harness.store.writes)
	}

	response := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", []byte(`{"expected_revision":0,"org_default":{"allow_mode":"x","allow":[],"deny":[]},"roles":{"bad name":{"allow_mode":"all","allow":[],"deny":[]}}}`))
	details, ok := decodeBody(t, response)["details"].([]any)
	if !ok || len(details) != 2 {
		t.Fatalf("a validation failure lists every problem: %s", response.Body.String())
	}
}

func TestModelPolicyReadIsAdminOnlyAndExplainIsMemberScoped(t *testing.T) {
	harness := newModelPolicyAdminHarness(t, map[string]string{"u-owner": "owner", "u-member": "member", "u-viewer": "viewer"})
	if response := harness.do(http.MethodPut, modelPolicyV1Path, "u-owner", "", validModelPolicyBody(0)); response.Code != http.StatusOK {
		t.Fatalf("seed write: %d %s", response.Code, response.Body.String())
	}

	if response := harness.do(http.MethodGet, modelPolicyV1Path, "u-member", "", nil); response.Code != http.StatusForbidden {
		t.Fatalf("a member must not read the whole policy set: %d", response.Code)
	}
	if response := harness.do(http.MethodGet, modelPolicyV1Path, "u-stranger", "admin", nil); response.Code != http.StatusForbidden {
		t.Fatalf("a non-member must not read the policy set: %d", response.Code)
	}
	read := harness.do(http.MethodGet, modelPolicyV1Path, "u-owner", "", nil)
	if read.Code != http.StatusOK || decodeBody(t, read)["revision"] != float64(1) {
		t.Fatalf("owner read: %d %s", read.Code, read.Body.String())
	}

	explainPath := modelPolicyV1Path + "/explain"
	self := harness.do(http.MethodGet, explainPath, "u-member", "", nil)
	selfBody := decodeBody(t, self)
	if self.Code != http.StatusOK || selfBody["state"] != "org" || selfBody["user_id"] != "u-member" || selfBody["role"] != "member" || selfBody["can_manage"] != false {
		t.Fatalf("member explaining themself: %d %s", self.Code, self.Body.String())
	}
	viewer := harness.do(http.MethodGet, explainPath, "u-viewer", "", nil)
	if got := decodeBody(t, viewer); got["state"] != "role" || got["sources"].([]any)[0].(map[string]any)["layer"] != "role_override" {
		t.Fatalf("viewer must resolve to the role override: %s", viewer.Body.String())
	}

	// Explaining someone else discloses their role and policy: admin only.
	if response := harness.do(http.MethodGet, explainPath+"?user_id=u-viewer", "u-member", "admin", nil); response.Code != http.StatusForbidden {
		t.Fatalf("a member explaining another member: %d", response.Code)
	}
	other := harness.do(http.MethodGet, explainPath+"?user_id=u-viewer", "u-owner", "", nil)
	if other.Code != http.StatusOK || decodeBody(t, other)["user_id"] != "u-viewer" || decodeBody(t, other)["role"] != "viewer" {
		t.Fatalf("owner explaining a viewer: %d %s", other.Code, other.Body.String())
	}
	if response := harness.do(http.MethodGet, explainPath+"?user_id=", "u-owner", "", nil); response.Code != http.StatusBadRequest {
		t.Fatalf("a blank user_id must be refused, not read as the caller: %d", response.Code)
	}
	if response := harness.do(http.MethodGet, explainPath+"?user_id=u-nobody", "u-owner", "", nil); decodeBody(t, response)["state"] != "member_not_found" {
		t.Fatalf("explaining a non-member must say so: %s", response.Body.String())
	}
	if response := harness.do(http.MethodGet, explainPath, "u-stranger", "admin", nil); response.Code != http.StatusForbidden {
		t.Fatalf("a non-member must not explain: %d", response.Code)
	}
}

func TestModelPolicyRoutesFailClosedWhenTheirDependenciesAreMissingOrBroken(t *testing.T) {
	// No service wired: 503, never an empty policy.
	bare := &Server{callerRoles: fakeCallerRoles{roles: map[string]string{"u-owner": "owner"}}}
	router := gin.New()
	router.GET("/o/:id/p", bare.getModelEntitlement)
	router.GET("/i/:orgId/p", bare.getModelEntitlementInternal)
	for path, headers := range map[string]map[string]string{"/o/org-1/p": {"x-user-id": "u-owner"}, "/i/org-1/p": nil} {
		request := httptest.NewRequest(http.MethodGet, path, nil)
		for name, value := range headers {
			request.Header.Set(name, value)
		}
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("%s with no service: %d; want 503", path, response.Code)
		}
	}

	// No role source wired: an admin route refuses rather than guess a role.
	noRoles := &Server{}
	noRoles.SetModelEntitlements(&fakeResolver{})
	router = gin.New()
	router.GET("/o/:id/p", noRoles.getModelEntitlement)
	request := httptest.NewRequest(http.MethodGet, "/o/org-1/p", nil)
	request.Header.Set("x-user-id", "u-owner")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("an admin route with no role source: %d; want 503", response.Code)
	}

	// A failing role lookup is a 500, not a 403 that reads like "not a member".
	failing := &Server{callerRoles: fakeCallerRoles{err: errors.New("database down")}}
	failing.SetModelEntitlements(&fakeResolver{})
	router = gin.New()
	router.GET("/o/:id/p", failing.getModelEntitlement)
	request = httptest.NewRequest(http.MethodGet, "/o/org-1/p", nil)
	request.Header.Set("x-user-id", "u-owner")
	response = httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("a failing role lookup: %d; want 500", response.Code)
	}
}

func modelPolicyInternalRequest(path, principal, token string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	if principal != "" {
		request.Header.Set("X-Service-Id", principal)
	}
	if token != "" {
		request.Header.Set("X-Service-Token", token)
	}
	return request
}

func serveModelPolicy(server *Server, request *http.Request) *httptest.ResponseRecorder {
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	return response
}

func TestInternalModelEntitlementRouteServesInferenceCoreOnly(t *testing.T) {
	resolver := &fakeResolver{resolution: modelentitlement.Resolution{
		State: modelentitlement.StateOrg, Revision: 5, Role: "member",
		Policy:  &modelentitlement.Policy{AllowMode: modelentitlement.AllowModeListed, Allow: []string{"gpt-5-mini"}, Deny: []string{}},
		Sources: []modelentitlement.Source{{Layer: modelentitlement.LayerOrgDefault}},
	}}
	server := modelPolicyServer(t, resolver)
	path := "/internal/orgs/org-policy-1/model-entitlement?user_id=u-member"

	response := serveModelPolicy(server, modelPolicyInternalRequest(path, "inference-core", modelPolicyInferenceToken))
	if response.Code != http.StatusOK {
		t.Fatalf("inference-core: status=%d body=%s", response.Code, response.Body.String())
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("Cache-Control=%q; want no-store", response.Header().Get("Cache-Control"))
	}
	body := decodeBody(t, response)
	if body["state"] != "org" || body["revision"] != float64(5) || body["role"] != "member" {
		t.Fatalf("body=%v", body)
	}
	if calls, org, user := resolver.snapshot(); calls != 1 || org != "org-policy-1" || user != "u-member" {
		t.Fatalf("resolver got calls=%d org=%q user=%q", calls, org, user)
	}

	// A call with no user_id is a service call: the resolver is asked for no user.
	serveModelPolicy(server, modelPolicyInternalRequest("/internal/orgs/org-policy-1/model-entitlement", "inference-core", modelPolicyInferenceToken))
	if _, _, user := resolver.snapshot(); user != "" {
		t.Fatalf("a call with no user_id must reach the resolver with no user, got %q", user)
	}

	// model-gateway holds the scope (gateway-side explain); every other
	// principal, the gateway included, is refused before the resolver runs.
	callsBefore, _, _ := resolver.snapshot()
	if response := serveModelPolicy(server, modelPolicyInternalRequest(path, "model-gateway", modelPolicyModelGatewayToken)); response.Code != http.StatusOK {
		t.Fatalf("model-gateway: status=%d", response.Code)
	}
	for principal, token := range map[string]string{"billing-core": modelPolicyBillingToken, "verevon-gateway": modelPolicyGatewayToken} {
		if response := serveModelPolicy(server, modelPolicyInternalRequest(path, principal, token)); response.Code != http.StatusForbidden {
			t.Fatalf("%s reached the route: status=%d", principal, response.Code)
		}
	}
	if calls, _, _ := resolver.snapshot(); calls != callsBefore+1 {
		t.Fatalf("only model-gateway's call may reach the resolver, calls went from %d to %d", callsBefore, calls)
	}

	for name, request := range map[string]*http.Request{
		"no credentials":    modelPolicyInternalRequest(path, "", ""),
		"unknown principal": modelPolicyInternalRequest(path, "someone-else", modelPolicyInferenceToken),
		"wrong token":       modelPolicyInternalRequest(path, "inference-core", modelPolicyBillingToken),
	} {
		if response := serveModelPolicy(server, request); response.Code != http.StatusUnauthorized {
			t.Fatalf("%s: status=%d; want 401", name, response.Code)
		}
	}
}

func TestInternalModelEntitlementRouteNeverReadsAsUnrestrictedOnFailure(t *testing.T) {
	path := "/internal/orgs/org-policy-1/model-entitlement"

	resolver := &fakeResolver{err: errors.New("database down")}
	server := modelPolicyServer(t, resolver)
	response := serveModelPolicy(server, modelPolicyInternalRequest(path+"?user_id=u-1", "inference-core", modelPolicyInferenceToken))
	if response.Code != http.StatusInternalServerError || strings.Contains(response.Body.String(), "state") || strings.Contains(response.Body.String(), "database") {
		t.Fatalf("a failure must be a bare 500, got %d %s", response.Code, response.Body.String())
	}

	// A blank or oversized user_id is refused instead of being read as "no user",
	// which would widen a person's access to the organisation default.
	fresh := &fakeResolver{}
	server = modelPolicyServer(t, fresh)
	for _, query := range []string{"?user_id=", "?user_id=%20%20", "?user_id=" + strings.Repeat("u", maxModelEntitlementUserIDLength+1)} {
		if response := serveModelPolicy(server, modelPolicyInternalRequest(path+query, "inference-core", modelPolicyInferenceToken)); response.Code != http.StatusBadRequest {
			t.Fatalf("%q: status=%d; want 400", query, response.Code)
		}
	}
	if calls, _, _ := fresh.snapshot(); calls != 0 {
		t.Fatalf("a refused query reached the resolver %d times", calls)
	}

	// With no service wired the route says so.
	unwired := modelPolicyServer(t, nil)
	if response := serveModelPolicy(unwired, modelPolicyInternalRequest(path, "inference-core", modelPolicyInferenceToken)); response.Code != http.StatusServiceUnavailable {
		t.Fatalf("no service: %d; want 503", response.Code)
	}
}

// The grant is one route: the scope opens neither the broad organisation reads
// nor the user-facing policy routes.
func TestModelEntitlementReadScopeIsNarrow(t *testing.T) {
	server := modelPolicyServer(t, &fakeResolver{})
	for _, path := range []string{
		"/internal/orgs",
		"/internal/orgs/org-policy-1",
		"/internal/orgs/org-policy-1/retention-posture",
		"/internal/orgs/org-policy-1/model-entitlement/extra",
		"/api/v1/organizations/org-policy-1/model-entitlement",
		"/api/v1/organizations/org-policy-1/model-entitlement/explain",
		"/orgs/org-policy-1/model-entitlement",
	} {
		response := serveModelPolicy(server, modelPolicyInternalRequest(path, "inference-core", modelPolicyInferenceToken))
		if response.Code != http.StatusForbidden {
			t.Fatalf("inference-core read %s: status=%d; want 403", path, response.Code)
		}
	}
	for _, method := range []string{http.MethodPost, http.MethodPut, http.MethodDelete, http.MethodPatch} {
		request := httptest.NewRequest(method, "/internal/orgs/org-policy-1/model-entitlement", nil)
		request.Header.Set("X-Service-Id", "inference-core")
		request.Header.Set("X-Service-Token", modelPolicyInferenceToken)
		if response := serveModelPolicy(server, request); response.Code != http.StatusForbidden {
			t.Fatalf("%s on the machine route: status=%d; want 403", method, response.Code)
		}
	}
}

func TestModelEntitlementScopeIsMachineOnly(t *testing.T) {
	// The :any suffix keeps it off the gateway principal, which holds only
	// delegated :self scopes.
	gateway := `[{"principal":"verevon-gateway","audience":"org-core","token":"` + modelPolicyGatewayToken + `","scopes":["org:model-entitlement:read:any"]}]`
	if _, err := parseServiceCredentials(gateway); err == nil {
		t.Fatal("expected org:model-entitlement:read:any to be refused for the gateway principal")
	}
	machine := `[{"principal":"inference-core","audience":"org-core","token":"` + modelPolicyInferenceToken + `","scopes":["org:model-entitlement:read:any"]}]`
	if credentials, err := parseServiceCredentials(machine); err != nil || len(credentials) != 1 {
		t.Fatalf("a machine principal with the scope was rejected: %v", err)
	}
}
