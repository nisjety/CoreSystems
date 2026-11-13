package http

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
)

// The member capability route: GET /orgs/:id/roles/:roleName/capabilities, the signed-user twin of
// the internal read the gateway's similarity-candidate check used to call. These tests drive the
// REAL route table and the REAL service-auth middleware. The role lookup and the capability read
// are fakes, so no database is needed.

const roleCapsOrg = "org-role-caps-1"

// roleCapsRegistry is a compose-like registry: the gateway holds only :self scopes, as it does in
// compose, and no :any scope (validateServiceCredential refuses one for the gateway).
func roleCapsRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self"]}
]`, modelPolicyGatewayToken)
}

// fakeRoleCapabilities answers EffectiveCapabilities from a map and counts the reads.
type fakeRoleCapabilities struct {
	mu       sync.Mutex
	caps     map[string][]string
	calls    int
	lastOrg  string
	lastRole string
	err      error
}

func (f *fakeRoleCapabilities) EffectiveCapabilities(_ context.Context, orgID, roleName string) ([]string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	f.lastOrg, f.lastRole = orgID, roleName
	if f.err != nil {
		return nil, f.err
	}
	return append([]string(nil), f.caps[roleName]...), nil
}

func (f *fakeRoleCapabilities) reads() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls
}

// roleCapsServer builds org-core's real server with the switch read from switchValue, the role
// lookup and the capability read replaced by fakes.
func roleCapsServer(t *testing.T, switchValue string, roles map[string]string, caps map[string][]string) (*Server, *fakeRoleCapabilities) {
	t.Helper()
	t.Setenv(serviceCredentialEnv, roleCapsRegistry())
	t.Setenv(roleCapabilitiesEnvEnabled, switchValue)
	server := NewServer(0, nil, nil, "", "")
	reader := &fakeRoleCapabilities{caps: caps}
	server.callerRoles = fakeCallerRoles{roles: roles}
	server.roleCapabilities = reader
	return server, reader
}

// roleCapsRequest is the gateway's delegated call for userID in org orgID, the way the BFF signs it.
func roleCapsRequest(path, userID, orgID string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	signModelPolicyDelegation(request, nil, userID, orgID, "")
	return request
}

func roleCapsErrorCode(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	envelope, ok := decodeBody(t, response)["error"].(map[string]any)
	if !ok {
		t.Fatalf("not a typed error envelope: %s", response.Body.String())
	}
	code, _ := envelope["code"].(string)
	return code
}

func roleCapsPath(role string) string {
	return "/orgs/" + roleCapsOrg + "/roles/" + role + "/capabilities"
}

// ORG_ROLE_CAPABILITIES_ENABLED is the strict literal "true": nothing else turns the route on.
func TestRoleCapabilitiesSwitchIsTheStrictLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "false", "TRUE", "True", "1", "yes", "on", " true", "true ", "\"true\""} {
		if roleCapabilitiesEnabled(func(string) string { return value }) {
			t.Errorf("ORG_ROLE_CAPABILITIES_ENABLED=%q turned the route on", value)
		}
	}
	if !roleCapabilitiesEnabled(func(name string) string {
		if name != "ORG_ROLE_CAPABILITIES_ENABLED" {
			t.Fatalf("read %q, want ORG_ROLE_CAPABILITIES_ENABLED", name)
		}
		return "true"
	}) {
		t.Fatal("ORG_ROLE_CAPABILITIES_ENABLED=true left the route off")
	}
}

// Off (unset or anything but "true"), the route answers 404 not_found to a member that would
// otherwise be served, and no role is read or resolved.
func TestRoleCapabilitiesRouteAnswersNotFoundWhileOff(t *testing.T) {
	roles := map[string]string{"u-member": "member"}
	caps := map[string][]string{"member": {"support:recurrence:read"}}
	for _, switchValue := range []string{"", "false", "1", "TRUE"} {
		server, reader := roleCapsServer(t, switchValue, roles, caps)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", roleCapsOrg))
		if response.Code != http.StatusNotFound || roleCapsErrorCode(t, response) != "not_found" {
			t.Fatalf("switch %q: %d %s; want 404 not_found", switchValue, response.Code, response.Body.String())
		}
		if reads := reader.reads(); reads != 0 {
			t.Fatalf("switch %q: the capability read ran %d times while off", switchValue, reads)
		}
	}
}

// On: a member reads the capabilities of the role it holds, on the signed-in delegation. The read
// is keyed by the membership table's role, the path's org, and never by a role the caller does
// not hold; a stranger is refused.
func TestRoleCapabilitiesAreTheCallersOwnRole(t *testing.T) {
	roles := map[string]string{"u-member": "member", "u-owner": "owner"}
	caps := map[string][]string{
		"member": {"org:read", "support:recurrence:read"},
		"owner":  {"org:read", "roles:manage"},
	}
	server, reader := roleCapsServer(t, "true", roles, caps)

	// The member reads its own role.
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", roleCapsOrg))
	if response.Code != http.StatusOK {
		t.Fatalf("member reads its own role: %d %s; want 200", response.Code, response.Body.String())
	}
	got, _ := decodeBody(t, response)["capabilities"].([]any)
	if len(got) != 2 || got[0] != "org:read" || got[1] != "support:recurrence:read" {
		t.Fatalf("member capabilities = %v; want [org:read support:recurrence:read]", got)
	}
	if reader.lastOrg != roleCapsOrg || reader.lastRole != "member" {
		t.Fatalf("the read was for org=%q role=%q; want org=%q role=member", reader.lastOrg, reader.lastRole, roleCapsOrg)
	}

	// A member asking for another role is refused before any capability is read.
	readsBefore := reader.reads()
	response = httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("owner"), "u-member", roleCapsOrg))
	if response.Code != http.StatusForbidden || roleCapsErrorCode(t, response) != "forbidden" {
		t.Fatalf("member asks for owner: %d %s; want 403 forbidden", response.Code, response.Body.String())
	}
	if reader.reads() != readsBefore {
		t.Fatal("a refused role was still read")
	}

	// A stranger with no active membership is refused.
	response = httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-stranger", roleCapsOrg))
	if response.Code != http.StatusForbidden || roleCapsErrorCode(t, response) != "forbidden" {
		t.Fatalf("stranger: %d %s; want 403 forbidden", response.Code, response.Body.String())
	}
}

// A role with no capabilities answers an empty list, never null, so the gateway reads "none".
func TestRoleCapabilitiesEmptyRoleAnswersAnEmptyList(t *testing.T) {
	server, _ := roleCapsServer(t, "true", map[string]string{"u-viewer": "viewer"}, nil)
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("viewer"), "u-viewer", roleCapsOrg))
	if response.Code != http.StatusOK {
		t.Fatalf("viewer: %d %s; want 200", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"capabilities":[]`) {
		t.Fatalf("viewer body = %s; want an empty capabilities list", response.Body.String())
	}
}

// The delegation binds the route's org: a delegation signed for another org is refused by the
// middleware before the handler runs, and no role is read.
func TestRoleCapabilitiesDelegationIsBoundToTheRouteOrg(t *testing.T) {
	server, reader := roleCapsServer(t, "true", map[string]string{"u-member": "member"}, map[string][]string{"member": {"org:read"}})
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", "some-other-org"))
	if response.Code != http.StatusForbidden {
		t.Fatalf("a delegation for another org: %d %s; want 403", response.Code, response.Body.String())
	}
	if reader.reads() != 0 {
		t.Fatal("the capability read ran for a delegation of another org")
	}
}

// A request with no verified delegation is refused by the :self middleware, so the gateway's
// machine identity alone reads no role.
func TestRoleCapabilitiesNeedAVerifiedDelegation(t *testing.T) {
	server, reader := roleCapsServer(t, "true", map[string]string{"u-member": "member"}, map[string][]string{"member": {"org:read"}})
	request := httptest.NewRequest(http.MethodGet, roleCapsPath("member"), nil)
	request.Header.Set("X-Service-Id", "verevon-gateway")
	request.Header.Set("X-Service-Token", modelPolicyGatewayToken)
	request.Header.Set("X-User-Id", "u-member")
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("an unsigned identity: %d %s; want 403", response.Code, response.Body.String())
	}
	if reader.reads() != 0 {
		t.Fatal("the capability read ran without a verified delegation")
	}
}

// A failed membership lookup is a 500 and a failed capability read is a 500; neither answers a set.
func TestRoleCapabilitiesReadFailuresAreServerErrors(t *testing.T) {
	t.Setenv(serviceCredentialEnv, roleCapsRegistry())
	t.Setenv(roleCapabilitiesEnvEnabled, "true")

	lookupFails := NewServer(0, nil, nil, "", "")
	lookupFails.callerRoles = fakeCallerRoles{err: errors.New("membership table unavailable")}
	lookupFails.roleCapabilities = &fakeRoleCapabilities{caps: map[string][]string{"member": {"org:read"}}}
	response := httptest.NewRecorder()
	lookupFails.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", roleCapsOrg))
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("membership lookup failure: %d %s; want 500", response.Code, response.Body.String())
	}

	readFails := NewServer(0, nil, nil, "", "")
	readFails.callerRoles = fakeCallerRoles{roles: map[string]string{"u-member": "member"}}
	readFails.roleCapabilities = &fakeRoleCapabilities{err: errors.New("rbac read failed")}
	response = httptest.NewRecorder()
	readFails.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", roleCapsOrg))
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("capability read failure: %d %s; want 500", response.Code, response.Body.String())
	}
}

// With no rbac repository wired, the route answers 503 rather than reading nothing as "no
// capabilities".
func TestRoleCapabilitiesWithoutARepositoryAreUnavailable(t *testing.T) {
	t.Setenv(serviceCredentialEnv, roleCapsRegistry())
	t.Setenv(roleCapabilitiesEnvEnabled, "true")
	server := NewServer(0, nil, nil, "", "")
	server.callerRoles = fakeCallerRoles{roles: map[string]string{"u-member": "member"}}
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, roleCapsRequest(roleCapsPath("member"), "u-member", roleCapsOrg))
	if response.Code != http.StatusServiceUnavailable || roleCapsErrorCode(t, response) != "unavailable" {
		t.Fatalf("no repository: %d %s; want 503 unavailable", response.Code, response.Body.String())
	}
}

// The real route table mounts the member route and leaves the internal read where it was.
func TestRoleCapabilitiesRouteIsMountedBesideTheInternalRead(t *testing.T) {
	t.Setenv(serviceCredentialEnv, roleCapsRegistry())
	t.Setenv(roleCapabilitiesEnvEnabled, "false")
	server := NewServer(0, nil, nil, "", "")
	want := map[string]bool{
		"GET /orgs/:id/roles/:roleName/capabilities":             false,
		"GET /internal/orgs/:orgId/roles/:roleName/capabilities": false,
	}
	for _, route := range server.router.Routes() {
		key := route.Method + " " + route.Path
		if _, ok := want[key]; ok {
			want[key] = true
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}
