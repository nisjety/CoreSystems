package http

import (
	"bytes"
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// W5-079-b: the organisation flag routes. Section 3.4 of the verification plan owes a
// parser test and an off-unchanged test for ORG_FLAGS_ENABLED; the rest pins the
// authority of the routes (membership role from the table, never the forwarded role)
// and the shape of what a non-admin reader sees.

const (
	orgFlagsOrg         = "org-flags-1"
	orgFlagsReaderToken = "hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh"
	orgFlagsMemberPath  = "/orgs/" + orgFlagsOrg + "/flags"
	orgFlagsInternal    = "/internal/orgs/" + orgFlagsOrg + "/flags"
)

// orgFlagsRegistry is a compose-like registry: the gateway holds only :self scopes, and
// one backend principal is granted org:flags:read:any the way a deployment would.
func orgFlagsRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self"]},
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:flags:read:any"]}
]`, modelPolicyGatewayToken, orgFlagsReaderToken)
}

// memoryOrgFlagService answers the routes from a map. It knows the real closed list
// (orgcore.OrgOverridableFlags) and refuses an unknown name the way the service does.
type memoryOrgFlagService struct {
	mu        sync.Mutex
	stored    map[string]orgcore.OrgFlagView
	reads     int
	writes    int
	lastValue string
	lastActor string
	lastRole  string
}

func newMemoryOrgFlagService() *memoryOrgFlagService {
	return &memoryOrgFlagService{stored: map[string]orgcore.OrgFlagView{}}
}

func (m *memoryOrgFlagService) ListOrgFlags(_ context.Context, _ string) ([]orgcore.OrgFlagView, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.reads++
	var out []orgcore.OrgFlagView
	for _, def := range orgcore.OrgOverridableFlags {
		view := orgcore.OrgFlagView{Name: def.Name, Kind: def.Kind, Default: def.Default, Value: def.Default}
		if stored, ok := m.stored[def.Name]; ok {
			view = stored
		}
		out = append(out, view)
	}
	return out, nil
}

func (m *memoryOrgFlagService) SetOrgFlag(_ context.Context, _, name, value, changedBy, actorRole string) (*orgcore.OrgFlagPutResult, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	known := false
	var def orgcore.OrgFlagDef
	for _, candidate := range orgcore.OrgOverridableFlags {
		if candidate.Name == name {
			known, def = true, candidate
		}
	}
	if !known {
		return nil, &orgcore.OrgFlagError{Kind: orgcore.ErrUnknownOrgFlag, Name: name, Message: "not an org-overridable flag"}
	}
	m.writes++
	m.lastValue, m.lastActor, m.lastRole = value, changedBy, actorRole
	revision := m.stored[name].Revision + 1
	m.stored[name] = orgcore.OrgFlagView{
		Name: name, Kind: def.Kind, Default: def.Default, Value: value, Stored: true,
		Revision: revision, UpdatedBy: changedBy, UpdatedAt: "2026-11-12T10:00:00Z",
	}
	return &orgcore.OrgFlagPutResult{Name: name, Value: value, Revision: revision}, nil
}

func (m *memoryOrgFlagService) counts() (reads, writes int) {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.reads, m.writes
}

// orgFlagsServer builds org-core's REAL server (real route table, real switch read, real
// service-auth middleware) with the flag service and the role lookup replaced by fakes.
// The PUT route's membership guard reads org-core's own table, so PUT is driven through
// newOrgFlagsHarness instead.
func orgFlagsServer(t *testing.T, switchValue string, roles map[string]string) (*Server, *memoryOrgFlagService) {
	t.Helper()
	t.Setenv(serviceCredentialEnv, orgFlagsRegistry())
	t.Setenv(orgFlagsEnvEnabled, switchValue)
	server := NewServer(0, nil, nil, "", "")
	service := newMemoryOrgFlagService()
	server.SetOrgFlags(service)
	server.callerRoles = fakeCallerRoles{roles: roles}
	return server, service
}

// newOrgFlagsHarness mounts the flag routes exactly as setupRoutes does (gate, then the
// REAL membership guard on the write) behind the REAL service-auth middleware.
func newOrgFlagsHarness(t *testing.T, on bool, roles map[string]string) (*gin.Engine, *memoryOrgFlagService) {
	t.Helper()
	credentials, err := parseServiceCredentials(orgFlagsRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	service := newMemoryOrgFlagService()
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}, orgFlagsOn: on}
	server.SetOrgFlags(service)
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	gate := server.requireOrgFlagsEnabled()
	guard := requireActiveMembership(modelPolicyMembers(roles))
	router.GET("/orgs/:id/flags", gate, server.getOrgFlags)
	router.PUT("/orgs/:id/flags/:name", gate, guard, server.putOrgFlag)
	return router, service
}

func orgFlagsMemberRequest(method, path, userID, forwardedRole string, body []byte) *http.Request {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, orgFlagsOrg, forwardedRole)
	return request
}

func serveOrgFlags(router http.Handler, request *http.Request) *httptest.ResponseRecorder {
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	return response
}

func orgFlagsErrorCode(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	envelope, ok := decodeBody(t, response)["error"].(map[string]any)
	if !ok {
		t.Fatalf("not a typed error envelope: %s", response.Body.String())
	}
	code, _ := envelope["code"].(string)
	return code
}

func orgFlagsEntries(t *testing.T, response *httptest.ResponseRecorder) map[string]map[string]any {
	t.Helper()
	data, ok := decodeBody(t, response)["data"].(map[string]any)
	if !ok {
		t.Fatalf("not a {data} envelope: %s", response.Body.String())
	}
	list, ok := data["flags"].([]any)
	if !ok {
		t.Fatalf("data.flags is not a list: %s", response.Body.String())
	}
	out := map[string]map[string]any{}
	for _, raw := range list {
		entry, _ := raw.(map[string]any)
		name, _ := entry["name"].(string)
		out[name] = entry
	}
	return out
}

// ORG_FLAGS_ENABLED is the strict literal "true": nothing else turns the routes on.
func TestOrgFlagsSwitchIsTheStrictLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "false", "TRUE", "True", "1", "yes", "on", " true", "true ", "\"true\""} {
		if orgFlagsEnabled(func(string) string { return value }) {
			t.Errorf("ORG_FLAGS_ENABLED=%q turned the routes on", value)
		}
	}
	if !orgFlagsEnabled(func(name string) string {
		if name != "ORG_FLAGS_ENABLED" {
			t.Fatalf("read %q, want ORG_FLAGS_ENABLED", name)
		}
		return "true"
	}) {
		t.Fatal("ORG_FLAGS_ENABLED=true left the routes off")
	}
}

// Off (unset or anything but "true"), every flag route answers 404 not_found to a caller
// that would otherwise be served, and no value is read.
func TestOrgFlagsRoutesAnswerNotFoundWhileOff(t *testing.T) {
	roles := map[string]string{"u-owner": "owner"}
	for _, switchValue := range []string{"", "1", "TRUE"} {
		server, service := orgFlagsServer(t, switchValue, roles)
		probes := []*http.Request{
			orgFlagsMemberRequest(http.MethodGet, orgFlagsMemberPath, "u-owner", "", nil),
			modelPolicyInternalRequest(orgFlagsInternal, "auth-core", orgFlagsReaderToken),
		}
		for _, request := range probes {
			response := serveOrgFlags(server.router, request)
			if response.Code != http.StatusNotFound || orgFlagsErrorCode(t, response) != "not_found" {
				t.Fatalf("switch %q: %s %s = %d %s; want 404 not_found", switchValue, request.Method, request.URL.Path, response.Code, response.Body.String())
			}
		}
		router, harnessService := newOrgFlagsHarness(t, false, roles)
		put := serveOrgFlags(router, orgFlagsMemberRequest(http.MethodPut, orgFlagsMemberPath+"/VITE_CHAT_READER_POLISH", "u-owner", "", []byte(`{"value":true}`)))
		if put.Code != http.StatusNotFound {
			t.Fatalf("switch %q: PUT = %d %s; want 404", switchValue, put.Code, put.Body.String())
		}
		for _, svc := range []*memoryOrgFlagService{service, harnessService} {
			if reads, writes := svc.counts(); reads != 0 || writes != 0 {
				t.Fatalf("switch %q: the service was used while off (reads=%d writes=%d)", switchValue, reads, writes)
			}
		}
	}
}

// The real route table mounts exactly the three flag routes.
func TestOrgFlagsRoutesAreMounted(t *testing.T) {
	server, _ := orgFlagsServer(t, "true", nil)
	want := map[string]bool{
		"GET /orgs/:id/flags":             false,
		"PUT /orgs/:id/flags/:name":       false,
		"GET /internal/orgs/:orgId/flags": false,
	}
	for _, route := range server.router.Routes() {
		key := route.Method + " " + route.Path
		if strings.HasSuffix(route.Path, "/flags") || strings.Contains(route.Path, "/flags/") {
			if _, ok := want[key]; !ok {
				t.Fatalf("unexpected flag route %s", key)
			}
			want[key] = true
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}

// On: any active member reads the values; only an owner or admin sees who wrote them; a
// stranger is refused; the role comes from the membership table, never the forwarded role.
func TestOrgFlagsMemberReadAuthority(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member"}
	server, service := orgFlagsServer(t, "true", roles)
	service.stored["VITE_CHAT_READER_POLISH"] = orgcore.OrgFlagView{
		Name: "VITE_CHAT_READER_POLISH", Kind: orgcore.OrgFlagKindSwitch, Default: "false", Value: "true",
		Stored: true, Revision: 3, UpdatedBy: "u-owner", UpdatedAt: "2026-11-12T10:00:00Z",
	}

	for _, user := range []string{"u-owner", "u-admin"} {
		response := serveOrgFlags(server.router, orgFlagsMemberRequest(http.MethodGet, orgFlagsMemberPath, user, "", nil))
		if response.Code != http.StatusOK {
			t.Fatalf("%s GET = %d %s", user, response.Code, response.Body.String())
		}
		if response.Header().Get("Cache-Control") != "no-store" {
			t.Fatalf("%s GET: Cache-Control %q, want no-store", user, response.Header().Get("Cache-Control"))
		}
		entry := orgFlagsEntries(t, response)["VITE_CHAT_READER_POLISH"]
		if entry["value"] != "true" || entry["updated_by"] != "u-owner" || entry["updated_at"] == nil {
			t.Fatalf("%s must see the stored value and its writer: %v", user, entry)
		}
	}

	for _, forwarded := range []string{"", "admin", "owner", "superadmin"} {
		response := serveOrgFlags(server.router, orgFlagsMemberRequest(http.MethodGet, orgFlagsMemberPath, "u-member", forwarded, nil))
		if response.Code != http.StatusOK {
			t.Fatalf("member (forwarded %q) GET = %d %s", forwarded, response.Code, response.Body.String())
		}
		entries := orgFlagsEntries(t, response)
		if len(entries) != len(orgcore.OrgOverridableFlags) {
			t.Fatalf("member read %d flags, want every org-overridable flag (%d)", len(entries), len(orgcore.OrgOverridableFlags))
		}
		entry := entries["VITE_CHAT_READER_POLISH"]
		if entry["value"] != "true" || entry["updated_by"] != nil || entry["updated_at"] != nil {
			t.Fatalf("member (forwarded %q) must see the value but not its writer: %v", forwarded, entry)
		}
	}

	stranger := serveOrgFlags(server.router, orgFlagsMemberRequest(http.MethodGet, orgFlagsMemberPath, "u-stranger", "owner", nil))
	if stranger.Code != http.StatusForbidden || orgFlagsErrorCode(t, stranger) != "forbidden" {
		t.Fatalf("non-member GET = %d %s; want 403 forbidden", stranger.Code, stranger.Body.String())
	}
}

// On: only an owner or admin writes, whatever role the gateway forwards; the audit role
// passed to the service is the table role; the body is strict.
func TestOrgFlagsWriteAuthorityAndBody(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"}
	router, service := newOrgFlagsHarness(t, true, roles)
	put := func(user, forwarded, name, body string) *httptest.ResponseRecorder {
		return serveOrgFlags(router, orgFlagsMemberRequest(http.MethodPut, orgFlagsMemberPath+"/"+name, user, forwarded, []byte(body)))
	}

	for _, user := range []string{"u-member", "u-viewer"} {
		for _, forwarded := range []string{"", "admin", "owner"} {
			response := put(user, forwarded, "VITE_CHAT_READER_POLISH", `{"value":true}`)
			if response.Code != http.StatusForbidden || orgFlagsErrorCode(t, response) != "admin_required" {
				t.Fatalf("%s (forwarded %q) PUT = %d %s; want 403 admin_required", user, forwarded, response.Code, response.Body.String())
			}
		}
	}
	if stranger := put("u-stranger", "owner", "VITE_CHAT_READER_POLISH", `{"value":true}`); stranger.Code != http.StatusForbidden {
		t.Fatalf("non-member PUT = %d; want 403", stranger.Code)
	}
	if _, writes := service.counts(); writes != 0 {
		t.Fatalf("a refused caller wrote a flag (%d writes)", writes)
	}

	ok := put("u-admin", "member", "VITE_CHAT_READER_POLISH", `{"value":true}`)
	if ok.Code != http.StatusOK {
		t.Fatalf("admin PUT = %d %s", ok.Code, ok.Body.String())
	}
	if service.lastValue != "true" || service.lastActor != "u-admin" || service.lastRole != "admin" {
		t.Fatalf("service got value=%q actor=%q role=%q; want true/u-admin/admin (table role)", service.lastValue, service.lastActor, service.lastRole)
	}
	if off := put("u-owner", "", "VITE_CHAT_READER_POLISH", `{"value":false}`); off.Code != http.StatusOK || service.lastValue != "false" {
		t.Fatalf("owner PUT false = %d %s (stored %q)", off.Code, off.Body.String(), service.lastValue)
	}

	_, writes := service.counts()
	refusals := []struct {
		body   string
		status int
		code   string
	}{
		{`{"value":1}`, http.StatusUnprocessableEntity, "invalid_value"},
		{`{"value":null}`, http.StatusUnprocessableEntity, "invalid_value"},
		{`{"value":["true"]}`, http.StatusUnprocessableEntity, "invalid_value"},
		{`{"value":true,"extra":1}`, http.StatusBadRequest, "invalid_body"},
		{`{"value":true}{"value":false}`, http.StatusBadRequest, "invalid_body"},
		{`not json`, http.StatusBadRequest, "invalid_body"},
		{`{"value":"` + strings.Repeat("x", 2048) + `"}`, http.StatusRequestEntityTooLarge, "too_large"},
	}
	for _, refusal := range refusals {
		response := put("u-owner", "", "VITE_CHAT_READER_POLISH", refusal.body)
		if response.Code != refusal.status || orgFlagsErrorCode(t, response) != refusal.code {
			t.Fatalf("body %.40q: %d %s; want %d %s", refusal.body, response.Code, response.Body.String(), refusal.status, refusal.code)
		}
	}
	unknown := put("u-owner", "", "VITE_NOT_A_FLAG", `{"value":true}`)
	if unknown.Code != http.StatusUnprocessableEntity || orgFlagsErrorCode(t, unknown) != "unknown_flag" {
		t.Fatalf("unknown flag PUT = %d %s; want 422 unknown_flag", unknown.Code, unknown.Body.String())
	}
	if _, after := service.counts(); after != writes {
		t.Fatalf("a refused body was written (%d -> %d)", writes, after)
	}
}

// The internal read is served to a holder of org:flags:read:any only, and never names
// the writer (there is no acting user to show it to).
func TestOrgFlagsInternalReadNeedsItsOwnScope(t *testing.T) {
	server, service := orgFlagsServer(t, "true", nil)
	service.stored["VITE_CHAT_CHANGED_FILES"] = orgcore.OrgFlagView{
		Name: "VITE_CHAT_CHANGED_FILES", Kind: orgcore.OrgFlagKindSwitch, Default: "false", Value: "true",
		Stored: true, Revision: 1, UpdatedBy: "u-owner", UpdatedAt: "2026-11-12T10:00:00Z",
	}
	ok := serveOrgFlags(server.router, modelPolicyInternalRequest(orgFlagsInternal, "auth-core", orgFlagsReaderToken))
	if ok.Code != http.StatusOK {
		t.Fatalf("scope holder internal GET = %d %s", ok.Code, ok.Body.String())
	}
	entry := orgFlagsEntries(t, ok)["VITE_CHAT_CHANGED_FILES"]
	if entry["value"] != "true" || entry["updated_by"] != nil || entry["updated_at"] != nil {
		t.Fatalf("internal read must carry the value and no writer: %v", entry)
	}
	readsBefore, _ := service.counts()
	for _, probe := range []struct{ principal, token string }{
		{"verevon-gateway", modelPolicyGatewayToken},
		{"auth-core", "wrong-token-wrong-token-wrong-token-"},
		{"", ""},
	} {
		response := serveOrgFlags(server.router, modelPolicyInternalRequest(orgFlagsInternal, probe.principal, probe.token))
		if response.Code != http.StatusUnauthorized && response.Code != http.StatusForbidden {
			t.Fatalf("%q internal GET = %d %s; want 401/403", probe.principal, response.Code, response.Body.String())
		}
	}
	if reads, _ := service.counts(); reads != readsBefore {
		t.Fatalf("a refused internal caller read the flags")
	}
}

// org-core refuses any :any scope for the gateway principal, so the gateway can never be
// granted org:flags:read:any: a registry that tries is rejected at parse (boot).
func TestOrgFlagsGatewayCannotHoldTheInternalReadScope(t *testing.T) {
	raw := fmt.Sprintf(`[{"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:flags:read:any"]}]`, modelPolicyGatewayToken)
	if _, err := parseServiceCredentials(raw); err == nil {
		t.Fatal("a registry granting org:flags:read:any to verevon-gateway was accepted")
	}
}
