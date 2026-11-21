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
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/gin-gonic/gin"
)

const (
	browserBrokerTestToken = "rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr"
	browserSitesV1Path     = "/api/v1/organizations/" + modelPolicyOrg + "/browser-sites"
	browserSitesMePath     = browserSitesV1Path + "/me"
	browserSitesInternal   = "/internal/orgs/" + modelPolicyOrg + "/browser-sites/resolve"
)

// browserSiteRegistry is the org-core credential registry with the gateway holding
// the reviewed self scopes and browser-broker holding the one read scope.
func browserSiteRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self"]},
  {"principal":"browser-broker","audience":"org-core","token":%q,"scopes":["org:browser-site:read:any"]}
]`, modelPolicyGatewayToken, browserBrokerTestToken)
}

// memoryBrowserSiteStore is a browsersites.Store with the repository's revision
// compare-and-set and per-member limit, so the real service and the real routes
// run without a database.
type memoryBrowserSiteStore struct {
	mu        sync.Mutex
	policy    browsersites.PolicySet
	prefs     map[string]browsersites.Pref
	userOf    map[string]string
	history   int
	historyOf map[string]int
}

func (m *memoryBrowserSiteStore) key(userID, host string) string { return userID + "|" + host }

func (m *memoryBrowserSiteStore) Get(_ context.Context, orgID string) (*browsersites.PolicySet, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	set := m.policySet(orgID)
	return &set, nil
}

func (m *memoryBrowserSiteStore) policySet(orgID string) browsersites.PolicySet {
	set := m.policy
	set.OrganizationID = orgID
	if set.Mode == "" {
		set.Mode = browsersites.ModeOpen
	}
	if set.Rules == nil {
		set.Rules = []browsersites.Rule{}
	}
	return set
}

func (m *memoryBrowserSiteStore) Put(_ context.Context, orgID, actorID string, expected int64, req browsersites.PutRequest) (*browsersites.PolicySet, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if expected != m.policy.Revision {
		return nil, &browsersites.RevisionConflictError{Current: m.policy.Revision}
	}
	now := time.Now().UTC()
	m.policy = browsersites.PolicySet{Revision: expected + 1, Mode: req.Mode, Rules: req.Rules, UpdatedBy: actorID, UpdatedAt: &now}
	m.history++
	set := m.policySet(orgID)
	return &set, nil
}

func (m *memoryBrowserSiteStore) Snapshot(_ context.Context, orgID, userID string, hosts []string) (*browsersites.PolicySet, map[string]browsersites.Pref, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	set := m.policySet(orgID)
	if userID == "" {
		return &set, nil, nil
	}
	prefs := map[string]browsersites.Pref{}
	for _, host := range hosts {
		if pref, ok := m.prefs[m.key(userID, host)]; ok {
			prefs[host] = pref
		}
	}
	return &set, prefs, nil
}

func (m *memoryBrowserSiteStore) ListPrefs(_ context.Context, orgID, userID string) (browsersites.PrefSet, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := browsersites.PrefSet{OrganizationID: orgID, UserID: userID, Revision: m.policy.Revision, Prefs: []browsersites.Pref{}}
	for k, pref := range m.prefs {
		if strings.HasPrefix(k, userID+"|") {
			out.Prefs = append(out.Prefs, pref)
		}
	}
	return out, nil
}

func (m *memoryBrowserSiteStore) PutPref(_ context.Context, _, userID string, expected int64, pref browsersites.Pref) (browsersites.Pref, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if expected != m.policy.Revision {
		return browsersites.Pref{}, &browsersites.RevisionConflictError{Current: m.policy.Revision}
	}
	if _, exists := m.prefs[m.key(userID, pref.Host)]; !exists && m.countPrefs(userID) >= browsersites.MaxPrefsPerUser {
		return browsersites.Pref{}, browsersites.ErrPrefLimitReached
	}
	if m.prefs == nil {
		m.prefs = map[string]browsersites.Pref{}
	}
	pref.UpdatedAt = time.Now().UTC()
	m.prefs[m.key(userID, pref.Host)] = pref
	m.recordHistory(userID)
	return pref, nil
}

func (m *memoryBrowserSiteStore) DeletePref(_ context.Context, _, userID, host string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.prefs[m.key(userID, host)]; !ok {
		return browsersites.ErrPrefNotFound
	}
	delete(m.prefs, m.key(userID, host))
	m.recordHistory(userID)
	return nil
}

// recordHistory counts one history row for a member's choice write or delete.
func (m *memoryBrowserSiteStore) recordHistory(userID string) {
	m.history++
	if m.historyOf == nil {
		m.historyOf = map[string]int{}
	}
	m.historyOf[userID]++
}

// DeleteUserData mirrors the repository's erasure: the member's choices go and their
// history rows are redacted, which the fake counts as the rows it holds for the member.
func (m *memoryBrowserSiteStore) DeleteUserData(_ context.Context, _, userID string) (int64, int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	prefs, history := m.eraseUser(userID)
	return prefs, history, nil
}

// DeleteRemovedMemberData mirrors the repository's erasure for a removed member. The fake
// keeps no membership table, so the active-member guard is pinned by the database test;
// here the erase runs as it would for a member who is not active.
func (m *memoryBrowserSiteStore) DeleteRemovedMemberData(_ context.Context, _, userID string) (int64, int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	prefs, history := m.eraseUser(userID)
	return prefs, history, nil
}

func (m *memoryBrowserSiteStore) eraseUser(userID string) (int64, int64) {
	var prefs int64
	for k := range m.prefs {
		if strings.HasPrefix(k, userID+"|") {
			delete(m.prefs, k)
			prefs++
		}
	}
	history := int64(m.historyOf[userID])
	delete(m.historyOf, userID)
	return prefs, history
}

func (m *memoryBrowserSiteStore) countPrefs(userID string) int {
	count := 0
	for k := range m.prefs {
		if strings.HasPrefix(k, userID+"|") {
			count++
		}
	}
	return count
}

type browserSiteHarness struct {
	router *gin.Engine
	store  *memoryBrowserSiteStore
}

// newBrowserSiteHarness mounts the browser site routes exactly as server.go does,
// behind the REAL service-auth middleware and the REAL membership guard. The
// membership guard reads org-core's own table in production, so the route table
// itself is pinned by TestBrowserSiteRoutesAreMountedOnTheRealRouter.
func newBrowserSiteHarness(t *testing.T, roles map[string]string) *browserSiteHarness {
	t.Helper()
	return newBrowserSiteHarnessWithFloor(t, roles, false, nil)
}

// newBrowserSiteHarnessWithFloor is newBrowserSiteHarness with the Space floor switch
// and reader set as the caller chooses. A nil reader leaves the floor unwired.
func newBrowserSiteHarnessWithFloor(t *testing.T, roles map[string]string, floorOn bool, floor browsersites.SpaceFloorReader) *browserSiteHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(browserSiteRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryBrowserSiteStore{}
	service := browsersites.NewService(store, time.Now)
	if floor != nil {
		service.SetSpaceFloor(floor)
	}
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}, browserSpaceFloorOn: floorOn}
	server.SetBrowserSites(service)

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	router.GET("/api/v1/organizations/:id/browser-sites", server.getBrowserSites)
	router.PUT("/api/v1/organizations/:id/browser-sites", guard, server.putBrowserSites)
	router.GET("/api/v1/organizations/:id/browser-sites/explain", server.explainBrowserSite)
	router.GET("/api/v1/organizations/:id/browser-sites/me", server.listBrowserSitePrefs)
	router.PUT("/api/v1/organizations/:id/browser-sites/me/:host", guard, server.putBrowserSitePref)
	router.DELETE("/api/v1/organizations/:id/browser-sites/me/:host", guard, server.deleteBrowserSitePref)
	router.GET("/internal/orgs/:orgId/browser-sites/resolve", server.getBrowserSitesInternal)
	return &browserSiteHarness{router: router, store: store}
}

// do sends a request as a signed-in user through the gateway principal.
func (h *browserSiteHarness) do(method, path, userID, platformRole string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, modelPolicyOrg, platformRole)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

// doAsBroker sends a request as the browser broker, which carries no user.
func (h *browserSiteHarness) doAsBroker(path string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	request.Header.Set("X-Service-Id", browserBrokerPrincipal)
	request.Header.Set("X-Service-Token", browserBrokerTestToken)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func browserSitePolicyBody(expected int64, mode string, rules ...map[string]string) []byte {
	body := map[string]any{"expected_revision": expected, "mode": mode, "rules": rules}
	encoded, _ := json.Marshal(body)
	return encoded
}

func prefBody(expected int64, level string) []byte {
	encoded, _ := json.Marshal(map[string]any{"expected_revision": expected, "level": level})
	return encoded
}

func TestBrowserSiteRoutesAreMountedOnTheRealRouter(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	want := map[string]bool{
		"GET /api/v1/organizations/:id/browser-sites":             false,
		"PUT /api/v1/organizations/:id/browser-sites":             false,
		"GET /api/v1/organizations/:id/browser-sites/explain":     false,
		"GET /api/v1/organizations/:id/browser-sites/me":          false,
		"PUT /api/v1/organizations/:id/browser-sites/me/:host":    false,
		"DELETE /api/v1/organizations/:id/browser-sites/me/:host": false,
		"GET /internal/orgs/:orgId/browser-sites/resolve":         false,

		"DELETE /internal/orgs/:orgId/members/:userId/browser-sites": false,
	}
	for _, route := range server.router.Routes() {
		if _, ok := want[route.Method+" "+route.Path]; ok {
			want[route.Method+" "+route.Path] = true
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}

func TestOnlyOwnerAndAdminReadOrWriteTheSitePolicy(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member"})

	if response := harness.do(http.MethodGet, browserSitesV1Path, "u-member", "", nil); response.Code != http.StatusForbidden {
		t.Fatalf("a member must not read the admin policy: status=%d", response.Code)
	}
	if response := harness.do(http.MethodPut, browserSitesV1Path, "u-member", "admin", browserSitePolicyBody(0, "open")); response.Code != http.StatusForbidden {
		t.Fatalf("a member must not write the admin policy whatever role is forwarded: status=%d", response.Code)
	}

	response := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(0, "allowlist", map[string]string{"host": "Shop.Example.com", "rule": "act"}))
	if response.Code != http.StatusOK {
		t.Fatalf("owner write: status=%d body=%s", response.Code, response.Body.String())
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("the policy must be no-store, got %q", response.Header().Get("Cache-Control"))
	}
	saved := decodeBody(t, response)
	if saved["revision"].(float64) != 1 || saved["mode"] != browsersites.ModeAllowlist {
		t.Fatalf("owner write must return revision 1 in allowlist mode: %v", saved)
	}

	read := harness.do(http.MethodGet, browserSitesV1Path, "u-admin", "", nil)
	if read.Code != http.StatusOK || decodeBody(t, read)["revision"].(float64) != 1 {
		t.Fatalf("admin read: status=%d body=%s", read.Code, read.Body.String())
	}
}

func TestStaleAdminWriteIsRefusedWithTheCurrentRevision(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-owner": "owner"})
	if response := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(0, "open")); response.Code != http.StatusOK {
		t.Fatalf("first write: status=%d", response.Code)
	}

	stale := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(0, "allowlist"))
	if stale.Code != http.StatusConflict {
		t.Fatalf("a stale write must be 409: status=%d body=%s", stale.Code, stale.Body.String())
	}
	body := decodeBody(t, stale)
	if body["code"] != "revision_conflict" || body["current_revision"].(float64) != 1 {
		t.Fatalf("the conflict must name the current revision: %v", body)
	}

	invalid := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(1, "open", map[string]string{"host": "10.0.0.1", "rule": "act"}))
	if invalid.Code != http.StatusBadRequest || decodeBody(t, invalid)["code"] != "invalid_browser_site_policy" {
		t.Fatalf("an IP-literal host must be refused as an invalid policy: status=%d body=%s", invalid.Code, invalid.Body.String())
	}
	unknown := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", []byte(`{"expected_revision":1,"mode":"open","rules":[],"revision":9}`))
	if unknown.Code != http.StatusBadRequest {
		t.Fatalf("an unknown field must be refused: status=%d", unknown.Code)
	}
}

func TestMemberChoiceIsRecordedAgainstTheCurrentPolicyAndExplained(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-owner": "owner", "u-member": "member"})
	if response := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(0, "open")); response.Code != http.StatusOK {
		t.Fatalf("policy write: status=%d", response.Code)
	}

	const host = "/example.com"
	stale := harness.do(http.MethodPut, browserSitesMePath+host, "u-member", "", prefBody(0, "act"))
	if stale.Code != http.StatusConflict || decodeBody(t, stale)["current_revision"].(float64) != 1 {
		t.Fatalf("a choice made against a superseded policy must be 409: status=%d body=%s", stale.Code, stale.Body.String())
	}

	saved := harness.do(http.MethodPut, browserSitesMePath+host, "u-member", "", prefBody(1, "act"))
	if saved.Code != http.StatusOK || decodeBody(t, saved)["level"] != "act" {
		t.Fatalf("choice write: status=%d body=%s", saved.Code, saved.Body.String())
	}

	list := harness.do(http.MethodGet, browserSitesMePath, "u-member", "", nil)
	if list.Code != http.StatusOK || len(decodeBody(t, list)["prefs"].([]any)) != 1 {
		t.Fatalf("the member must see their one choice: status=%d body=%s", list.Code, list.Body.String())
	}

	explain := harness.do(http.MethodGet, browserSitesV1Path+"/explain?host=example.com", "u-member", "", nil)
	explained := decodeBody(t, explain)
	decision := explained["decision"].(map[string]any)
	if explain.Code != http.StatusOK || decision["effective"] != browsersites.EffectiveAct || decision["source"] != browsersites.SourceUserPref {
		t.Fatalf("explain must answer act from the member's choice: status=%d body=%s", explain.Code, explain.Body.String())
	}

	if response := harness.do(http.MethodDelete, browserSitesMePath+host, "u-member", "", nil); response.Code != http.StatusNoContent {
		t.Fatalf("delete: status=%d", response.Code)
	}
	if response := harness.do(http.MethodDelete, browserSitesMePath+host, "u-member", "", nil); response.Code != http.StatusNotFound {
		t.Fatalf("a second delete must be 404: status=%d", response.Code)
	}
}

func TestMemberBodyCannotNameAnotherUser(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-member": "member"})
	body := []byte(`{"expected_revision":0,"level":"act","user_id":"someone-else"}`)
	if response := harness.do(http.MethodPut, browserSitesMePath+"/example.com", "u-member", "", body); response.Code != http.StatusBadRequest {
		t.Fatalf("a body naming another user must be refused: status=%d", response.Code)
	}
}

func TestMemberChoiceListIsCappedAt200(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-member": "member"})
	for i := 0; i < browsersites.MaxPrefsPerUser; i++ {
		path := fmt.Sprintf("%s/site%03d.example.com", browserSitesMePath, i)
		if response := harness.do(http.MethodPut, path, "u-member", "", prefBody(0, "read")); response.Code != http.StatusOK {
			t.Fatalf("choice %d: status=%d body=%s", i, response.Code, response.Body.String())
		}
	}
	over := harness.do(http.MethodPut, browserSitesMePath+"/site200.example.com", "u-member", "", prefBody(0, "read"))
	if over.Code != http.StatusUnprocessableEntity || decodeBody(t, over)["code"] != "pref_limit_reached" {
		t.Fatalf("the 201st choice must be refused: status=%d body=%s", over.Code, over.Body.String())
	}
}

func TestBrokerResolvesTheSiteDecisionForAMemberOnly(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-owner": "owner", "u-member": "member"})
	if response := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", browserSitePolicyBody(0, "open", map[string]string{"host": "blocked.example.com", "rule": "block"})); response.Code != http.StatusOK {
		t.Fatalf("policy write: status=%d", response.Code)
	}
	if response := harness.do(http.MethodPut, browserSitesMePath+"/open.example.com", "u-member", "", prefBody(1, "act")); response.Code != http.StatusOK {
		t.Fatalf("member choice: status=%d body=%s", response.Code, response.Body.String())
	}

	withUser := harness.doAsBroker(browserSitesInternal + "?user_id=u-member&host=open.example.com&host=blocked.example.com")
	if withUser.Code != http.StatusOK {
		t.Fatalf("broker resolve: status=%d body=%s", withUser.Code, withUser.Body.String())
	}
	if withUser.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("the decision must be no-store, got %q", withUser.Header().Get("Cache-Control"))
	}
	decisions := decodeBody(t, withUser)["decisions"].([]any)
	// The resolution answers in the golden's bytewise host order, so blocked comes first.
	blocked := decisions[0].(map[string]any)
	open := decisions[1].(map[string]any)
	if open["effective"] != browsersites.EffectiveAct || open["source"] != browsersites.SourceUserPref {
		t.Fatalf("the member's choice must apply to the broker's resolve: %v", open)
	}
	if blocked["effective"] != browsersites.EffectiveDeny || blocked["source"] != browsersites.SourceOrgBlock {
		t.Fatalf("a blocked host must be denied: %v", blocked)
	}

	serviceCall := harness.doAsBroker(browserSitesInternal + "?host=open.example.com")
	if got := decodeBody(t, serviceCall)["decisions"].([]any)[0].(map[string]any); got["effective"] != browsersites.EffectiveAsk {
		t.Fatalf("a call with no user must never see a member's choice: %v", got)
	}

	blank := harness.doAsBroker(browserSitesInternal + "?user_id=&host=open.example.com")
	if blank.Code != http.StatusBadRequest {
		t.Fatalf("a blank user_id must be refused, not read as absent: status=%d", blank.Code)
	}
	if none := harness.doAsBroker(browserSitesInternal); none.Code != http.StatusBadRequest {
		t.Fatalf("a resolve with no host must be refused: status=%d", none.Code)
	}
	tooMany := browserSitesInternal + "?"
	for i := 0; i <= browsersites.MaxResolveHosts; i++ {
		tooMany += fmt.Sprintf("host=site%02d.example.com&", i)
	}
	if response := harness.doAsBroker(tooMany); response.Code != http.StatusBadRequest {
		t.Fatalf("more hosts than the golden allows must be refused: status=%d", response.Code)
	}
}

func TestGatewayCannotReadTheBrokerResolve(t *testing.T) {
	harness := newBrowserSiteHarness(t, map[string]string{"u-member": "member"})
	request := httptest.NewRequest(http.MethodGet, browserSitesInternal+"?host=example.com", nil)
	request.Header.Set("X-Service-Id", gatewayServicePrincipal)
	request.Header.Set("X-Service-Token", modelPolicyGatewayToken)
	response := httptest.NewRecorder()
	harness.router.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("the gateway must not hold the broker's resolve scope: status=%d", response.Code)
	}
}

func TestUnconfiguredServiceAnswers503(t *testing.T) {
	server := &Server{callerRoles: fakeCallerRoles{roles: map[string]string{"u-owner": "owner"}}}
	router := gin.New()
	router.GET("/api/v1/organizations/:id/browser-sites", server.getBrowserSites)
	request := httptest.NewRequest(http.MethodGet, browserSitesV1Path, nil)
	signModelPolicyDelegation(request, nil, "u-owner", modelPolicyOrg, "")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("a server without the service must answer 503: status=%d", response.Code)
	}
}

func TestBrowserSiteScopesAreTheReviewedSet(t *testing.T) {
	cases := []struct {
		method, path, want string
	}{
		{http.MethodGet, browserSitesV1Path, "org:read:self"},
		{http.MethodPut, browserSitesV1Path, "org:settings:write:self"},
		{http.MethodGet, browserSitesV1Path + "/explain", "org:read:self"},
		{http.MethodGet, browserSitesMePath, "org:read:self"},
		{http.MethodPut, browserSitesMePath + "/example.com", "org:settings:write:self"},
		{http.MethodDelete, browserSitesMePath + "/example.com", "org:settings:write:self"},
		{http.MethodGet, browserSitesInternal, "org:browser-site:read:any"},
		{http.MethodDelete, "/internal/orgs/" + modelPolicyOrg + "/members/u-1/browser-sites", "org:browser-site:erase:any"},
	}
	for _, tc := range cases {
		request := httptest.NewRequest(tc.method, tc.path, nil)
		scopes := serviceScopesForRequest(request)
		if len(scopes) != 1 || scopes[0] != tc.want {
			t.Errorf("%s %s: scopes %v, want [%s]", tc.method, tc.path, scopes, tc.want)
		}
	}
}

func TestOnlyTheBrowserBrokerMayHoldTheBrokerResolveScope(t *testing.T) {
	gatewayWithScope := fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self","org:browser-site:read:any"]}
]`, modelPolicyGatewayToken)
	if _, err := parseServiceCredentials(gatewayWithScope); err == nil {
		t.Fatal("the gateway must not be able to hold org:browser-site:read:any")
	}
	auditWithScope := fmt.Sprintf(`[
  {"principal":"audit-core","audience":"org-core","token":%q,"scopes":["org:browser-site:read:any"]}
]`, browserBrokerTestToken)
	if _, err := parseServiceCredentials(auditWithScope); err == nil {
		t.Fatal("only the three allowed holders may hold org:browser-site:read:any: browser-broker, model-gateway, execution-core")
	}
	if _, err := parseServiceCredentials(browserSiteRegistry()); err != nil {
		t.Fatalf("the browser-broker registry must parse: %v", err)
	}
}

// recordingSpaceFloor answers one value for the Space floor and records every read, so
// a test can see whether the floor was read at all and for which organisation and Space.
type recordingSpaceFloor struct {
	mu       sync.Mutex
	entitled bool
	err      error
	reads    int
	org      string
	space    string
}

func (f *recordingSpaceFloor) BrowserFloor(_ context.Context, orgID, spaceID string) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.reads++
	f.org, f.space = orgID, spaceID
	return f.entitled, f.err
}

func (f *recordingSpaceFloor) readCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.reads
}

// floorPolicy writes the policy and the one member choice the Space floor tests read:
// the organisation blocks one host, and the member has chosen act for another.
func floorPolicy(t *testing.T, harness *browserSiteHarness) {
	t.Helper()
	policy := browserSitePolicyBody(0, "open", map[string]string{"host": "blocked.example.com", "rule": "block"})
	if response := harness.do(http.MethodPut, browserSitesV1Path, "u-owner", "", policy); response.Code != http.StatusOK {
		t.Fatalf("policy write: status=%d body=%s", response.Code, response.Body.String())
	}
	if response := harness.do(http.MethodPut, browserSitesMePath+"/open.example.com", "u-member", "", prefBody(1, "act")); response.Code != http.StatusOK {
		t.Fatalf("member choice: status=%d body=%s", response.Code, response.Body.String())
	}
}

// resolvedDecision returns the decision for one host in a broker resolve response.
func resolvedDecision(t *testing.T, response *httptest.ResponseRecorder, host string) map[string]any {
	t.Helper()
	if response.Code != http.StatusOK {
		t.Fatalf("resolve: status=%d body=%s", response.Code, response.Body.String())
	}
	for _, item := range decodeBody(t, response)["decisions"].([]any) {
		if decision := item.(map[string]any); decision["host"] == host {
			return decision
		}
	}
	t.Fatalf("no decision for %s in %s", host, response.Body.String())
	return nil
}

func TestSpaceFloorSwitchIsTheStrictLiteral(t *testing.T) {
	switchedTo := func(value string) func(string) string {
		return func(name string) string {
			if name == browserSpaceFloorEnvEnabled {
				return value
			}
			return ""
		}
	}
	if !browserSpaceFloorEnabled(switchedTo("true")) {
		t.Fatal("the literal true must turn the Space floor on")
	}
	for _, value := range []string{"", "1", "TRUE", " true", "true ", "on", "yes"} {
		if browserSpaceFloorEnabled(switchedTo(value)) {
			t.Errorf("%q must leave the Space floor off", value)
		}
	}
}

func TestSpaceIdIsIgnoredWhileTheSpaceFloorSwitchIsOff(t *testing.T) {
	floor := &recordingSpaceFloor{entitled: false}
	harness := newBrowserSiteHarnessWithFloor(t, map[string]string{"u-owner": "owner", "u-member": "member"}, false, floor)
	floorPolicy(t, harness)

	const base = browserSitesInternal + "?user_id=u-member&host=open.example.com"
	plain := harness.doAsBroker(base)
	scoped := harness.doAsBroker(base + "&space_id=sp-1")
	blank := harness.doAsBroker(base + "&space_id=")
	for _, response := range []*httptest.ResponseRecorder{plain, scoped, blank} {
		if response.Code != http.StatusOK {
			t.Fatalf("with the switch off every resolve answers as before: status=%d body=%s", response.Code, response.Body.String())
		}
	}
	if scoped.Body.String() != plain.Body.String() || blank.Body.String() != plain.Body.String() {
		t.Fatalf("with the switch off space_id must not change the answer:\nplain=%s\nscoped=%s\nblank=%s", plain.Body.String(), scoped.Body.String(), blank.Body.String())
	}
	if got := resolvedDecision(t, scoped, "open.example.com"); got["effective"] != browsersites.EffectiveAct {
		t.Fatalf("the member's choice must still apply while the switch is off: %v", got)
	}
	if floor.readCount() != 0 {
		t.Fatalf("the Space floor must not be read while the switch is off: %d reads", floor.readCount())
	}
}

func TestSpaceFloorRefusesAnUnentitledSpaceWhileTheSwitchIsOn(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-member": "member"}
	const scopedPath = browserSitesInternal + "?user_id=u-member&host=blocked.example.com&host=open.example.com&space_id=sp-1"

	t.Run("an unentitled Space is refused every host", func(t *testing.T) {
		floor := &recordingSpaceFloor{entitled: false}
		harness := newBrowserSiteHarnessWithFloor(t, roles, true, floor)
		floorPolicy(t, harness)
		response := harness.doAsBroker(scopedPath)
		if got := resolvedDecision(t, response, "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("an unentitled Space must refuse the open host: %v", got)
		}
		if got := resolvedDecision(t, response, "blocked.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceOrgBlock {
			t.Fatalf("a host the organisation blocks keeps its reason: %v", got)
		}
		if floor.readCount() != 1 || floor.org != modelPolicyOrg || floor.space != "sp-1" {
			t.Fatalf("the floor must be read once for this organisation and Space: reads=%d org=%s space=%s", floor.readCount(), floor.org, floor.space)
		}
	})

	t.Run("an entitled Space answers exactly as an unscoped call", func(t *testing.T) {
		harness := newBrowserSiteHarnessWithFloor(t, roles, true, &recordingSpaceFloor{entitled: true})
		floorPolicy(t, harness)
		scoped := harness.doAsBroker(scopedPath)
		unscoped := harness.doAsBroker(browserSitesInternal + "?user_id=u-member&host=blocked.example.com&host=open.example.com")
		if got := resolvedDecision(t, scoped, "open.example.com"); got["effective"] != browsersites.EffectiveAct || got["source"] != browsersites.SourceUserPref {
			t.Fatalf("an entitled Space must keep the member's choice: %v", got)
		}
		if scoped.Body.String() != unscoped.Body.String() {
			t.Fatalf("an entitled Space must not change the answer:\nscoped=%s\nunscoped=%s", scoped.Body.String(), unscoped.Body.String())
		}
	})

	t.Run("an unreadable floor refuses, fail closed", func(t *testing.T) {
		floor := &recordingSpaceFloor{entitled: true, err: errors.New("user-core unavailable")}
		harness := newBrowserSiteHarnessWithFloor(t, roles, true, floor)
		floorPolicy(t, harness)
		if got := resolvedDecision(t, harness.doAsBroker(scopedPath), "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("an unreadable floor must refuse: %v", got)
		}
	})

	t.Run("a service with no floor reader refuses, fail closed", func(t *testing.T) {
		harness := newBrowserSiteHarnessWithFloor(t, roles, true, nil)
		floorPolicy(t, harness)
		if got := resolvedDecision(t, harness.doAsBroker(scopedPath), "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("a Space-scoped resolve with no floor reader must refuse: %v", got)
		}
	})
}

func TestAbsentSpaceIdIsUnscopedWhileTheSpaceFloorSwitchIsOn(t *testing.T) {
	floor := &recordingSpaceFloor{entitled: false}
	harness := newBrowserSiteHarnessWithFloor(t, map[string]string{"u-owner": "owner", "u-member": "member"}, true, floor)
	floorPolicy(t, harness)
	response := harness.doAsBroker(browserSitesInternal + "?user_id=u-member&host=open.example.com")
	if got := resolvedDecision(t, response, "open.example.com"); got["effective"] != browsersites.EffectiveAct {
		t.Fatalf("a call without a Space is unscoped and keeps the member's choice: %v", got)
	}
	if floor.readCount() != 0 {
		t.Fatalf("an unscoped call must not read the Space floor: %d reads", floor.readCount())
	}
}

func TestSpaceFloorRefusesABlankOrOverlongSpaceIdWhileTheSwitchIsOn(t *testing.T) {
	harness := newBrowserSiteHarnessWithFloor(t, map[string]string{"u-owner": "owner"}, true, &recordingSpaceFloor{entitled: true})
	const base = browserSitesInternal + "?host=open.example.com&space_id="
	if response := harness.doAsBroker(base); response.Code != http.StatusBadRequest {
		t.Fatalf("a blank space_id must be refused, not read as absent: status=%d", response.Code)
	}
	if response := harness.doAsBroker(base + "%20"); response.Code != http.StatusBadRequest {
		t.Fatalf("a whitespace space_id must be refused: status=%d", response.Code)
	}
	if response := harness.doAsBroker(base + strings.Repeat("s", maxBrowserSpaceIDLength+1)); response.Code != http.StatusBadRequest {
		t.Fatalf("an overlong space_id must be refused: status=%d", response.Code)
	}
	if response := harness.doAsBroker(base + strings.Repeat("s", maxBrowserSpaceIDLength)); response.Code != http.StatusOK {
		t.Fatalf("a space_id at the cap is accepted: status=%d body=%s", response.Code, response.Body.String())
	}
}

// A space_id made of anything but letters, digits, underscores, hyphens and colons is refused
// before the floor is read, and no floor read is made for it.
func TestSpaceFloorRefusesAMalformedSpaceIdWhileTheSwitchIsOn(t *testing.T) {
	floor := &recordingSpaceFloor{entitled: true}
	harness := newBrowserSiteHarnessWithFloor(t, map[string]string{"u-owner": "owner"}, true, floor)
	const base = browserSitesInternal + "?host=open.example.com&space_id="
	for name, value := range map[string]string{"dot": "sp.1", "slash": "sp%2F1", "space": "sp%201", "quote": "sp%22"} {
		if response := harness.doAsBroker(base + value); response.Code != http.StatusBadRequest {
			t.Fatalf("%s: a malformed space_id must be refused: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
	if floor.readCount() != 0 {
		t.Fatalf("a refused space_id must not read the floor: %d reads", floor.readCount())
	}
}

// browserSiteFloorTestToken is a low-entropy fixture for org-core's service token on the user-core hop.
const browserSiteFloorTestToken = "fixture-user-core-floor-aaaaaaaaaaaaaaaaaaaa"

// userCoreFloorHit is one floor read a stand-in user-core received.
type userCoreFloorHit struct {
	path      string
	serviceID string
	token     string
}

// newStandInUserCore answers every floor read with one entitlement and records what each read carried.
func newStandInUserCore(t *testing.T, entitled bool) (*httptest.Server, func() []userCoreFloorHit) {
	t.Helper()
	var mu sync.Mutex
	var hits []userCoreFloorHit
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		hits = append(hits, userCoreFloorHit{path: r.URL.Path, serviceID: r.Header.Get("X-Service-Id"), token: r.Header.Get("X-Service-Token")})
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = fmt.Fprintf(w, `{"data":{"space_ref":"sp-1","browser_entitled":%t}}`, entitled)
	}))
	t.Cleanup(server.Close)
	return server, func() []userCoreFloorHit {
		mu.Lock()
		defer mu.Unlock()
		return append([]userCoreFloorHit(nil), hits...)
	}
}

// newBrowserSiteHarnessWired is the harness with the Space floor switch on and the reader wired the way
// server.go and main.go wire it: SetBrowserSpaceFloor builds it from the server's user-core URL and token.
func newBrowserSiteHarnessWired(t *testing.T, roles map[string]string, userCoreURL, serviceToken string) *browserSiteHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(browserSiteRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryBrowserSiteStore{}
	service := browsersites.NewService(store, time.Now)
	server := &Server{
		callerRoles:         fakeCallerRoles{roles: roles},
		browserSpaceFloorOn: true,
		userService:         userCoreURL,
		userServiceToken:    serviceToken,
	}
	server.SetBrowserSites(service)
	server.SetBrowserSpaceFloor()

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	router.GET("/api/v1/organizations/:id/browser-sites", server.getBrowserSites)
	router.PUT("/api/v1/organizations/:id/browser-sites", guard, server.putBrowserSites)
	router.PUT("/api/v1/organizations/:id/browser-sites/me/:host", guard, server.putBrowserSitePref)
	router.GET("/internal/orgs/:orgId/browser-sites/resolve", server.getBrowserSitesInternal)
	return &browserSiteHarness{router: router, store: store}
}

// The Space floor is read from user-core with the server's own user-core URL and service token, the way
// the server wires it. Without either, the floor is never read and a Space-scoped resolve refuses every host.
func TestSpaceFloorIsWiredFromTheServerUserCoreURLAndToken(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-member": "member"}
	const scopedPath = browserSitesInternal + "?user_id=u-member&host=blocked.example.com&host=open.example.com&space_id=sp-1"

	t.Run("an entitled Space keeps the member's choice and is read once with org-core's identity", func(t *testing.T) {
		userCore, hits := newStandInUserCore(t, true)
		harness := newBrowserSiteHarnessWired(t, roles, userCore.URL, browserSiteFloorTestToken)
		floorPolicy(t, harness)
		response := harness.doAsBroker(scopedPath)
		if got := resolvedDecision(t, response, "open.example.com"); got["effective"] != browsersites.EffectiveAct || got["source"] != browsersites.SourceUserPref {
			t.Fatalf("an entitled Space must keep the member's choice: %v", got)
		}
		want := userCoreFloorHit{path: "/api/v1/internal/spaces/sp-1/browser-floor", serviceID: "org-core", token: browserSiteFloorTestToken}
		if seen := hits(); len(seen) != 1 || seen[0] != want {
			t.Fatalf("the floor read carries %+v, want one read of %+v", seen, want)
		}
	})

	t.Run("an unentitled Space refuses every host", func(t *testing.T) {
		userCore, _ := newStandInUserCore(t, false)
		harness := newBrowserSiteHarnessWired(t, roles, userCore.URL, browserSiteFloorTestToken)
		floorPolicy(t, harness)
		if got := resolvedDecision(t, harness.doAsBroker(scopedPath), "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("an unentitled Space must refuse the host: %v", got)
		}
	})

	t.Run("a missing service token leaves the floor unread and refuses every host", func(t *testing.T) {
		userCore, hits := newStandInUserCore(t, true)
		harness := newBrowserSiteHarnessWired(t, roles, userCore.URL, "")
		floorPolicy(t, harness)
		if got := resolvedDecision(t, harness.doAsBroker(scopedPath), "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("without a token the floor is unknown and must refuse: %v", got)
		}
		if n := len(hits()); n != 0 {
			t.Fatalf("without a token user-core must not be asked: %d reads", n)
		}
	})

	t.Run("a missing user-core URL leaves the floor unread and refuses every host", func(t *testing.T) {
		harness := newBrowserSiteHarnessWired(t, roles, "", browserSiteFloorTestToken)
		floorPolicy(t, harness)
		if got := resolvedDecision(t, harness.doAsBroker(scopedPath), "open.example.com"); got["effective"] != browsersites.EffectiveDeny || got["source"] != browsersites.SourceSpaceFloor {
			t.Fatalf("without a URL the floor is unknown and must refuse: %v", got)
		}
	})
}
