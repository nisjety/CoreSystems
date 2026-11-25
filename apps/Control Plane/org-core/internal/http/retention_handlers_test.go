package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

const (
	retentionSessionToken = "ssssssssssssssssssssssssssssssssss"
	retentionV1Path       = "/api/v1/organizations/" + modelPolicyOrg + "/retention"
	legalHoldsV1Path      = "/api/v1/organizations/" + modelPolicyOrg + "/legal-holds"
	retentionCheckPath    = "/internal/orgs/" + modelPolicyOrg + "/retention/check"
)

// retentionRegistry is the org-core credential registry with the gateway holding
// the reviewed self scopes and session-core holding the one check scope.
func retentionRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self"]},
  {"principal":"session-core","audience":"org-core","token":%q,"scopes":["org:read:any","org:retention-policy:read:any"]}
]`, modelPolicyGatewayToken, retentionSessionToken)
}

// memoryRetentionStore is a retention.Store with the repository's revision
// compare-and-set and one-active-hold rule, so the real service and the real
// routes run without a database.
type memoryRetentionStore struct {
	mu      sync.Mutex
	policy  retention.Policy
	holds   []retention.Hold
	missing bool
	// actorRoles is the acting role of every hold placement and release, in order,
	// as the audit row names it.
	actorRoles []string
}

func (m *memoryRetentionStore) GetPolicy(_ context.Context, orgID string) (*retention.Policy, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	policy := m.policy
	policy.OrganizationID = orgID
	return &policy, nil
}

func (m *memoryRetentionStore) PutPolicy(_ context.Context, orgID string, actor retention.Actor, expected int64, days *int) (*retention.Policy, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if expected != m.policy.Revision {
		return nil, &retention.RevisionConflictError{Current: m.policy.Revision}
	}
	now := time.Now().UTC()
	m.policy = retention.Policy{OrganizationID: orgID, ThreadDays: days, Revision: expected + 1, UpdatedBy: actor.ID, UpdatedAt: &now}
	policy := m.policy
	return &policy, nil
}

func (m *memoryRetentionStore) ListHolds(context.Context, string) ([]retention.Hold, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	return append([]retention.Hold(nil), m.holds...), nil
}

func (m *memoryRetentionStore) PlaceHold(_ context.Context, _ string, actor retention.Actor, hold retention.Hold) (retention.PlaceResult, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, existing := range m.holds {
		if existing.Active() && existing.Scope == hold.Scope && existing.TargetRef == hold.TargetRef {
			return retention.PlaceResult{Hold: existing, Existed: true}, nil
		}
	}
	m.actorRoles = append(m.actorRoles, "place:"+actor.Role)
	hold.CreatedBy = actor.ID
	hold.CreatedAt = time.Now().UTC()
	m.holds = append(m.holds, hold)
	return retention.PlaceResult{Hold: hold}, nil
}

func (m *memoryRetentionStore) ReleaseHold(_ context.Context, _, holdID string, actor retention.Actor) (retention.Hold, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for i, hold := range m.holds {
		if hold.ID != holdID {
			continue
		}
		if !hold.Active() {
			return retention.Hold{}, retention.ErrHoldAlreadyReleased
		}
		m.actorRoles = append(m.actorRoles, "release:"+actor.Role)
		now := time.Now().UTC()
		releasedBy := actor.ID
		m.holds[i].ReleasedAt, m.holds[i].ReleasedBy = &now, &releasedBy
		return m.holds[i], nil
	}
	return retention.Hold{}, retention.ErrHoldNotFound
}

func (m *memoryRetentionStore) Check(_ context.Context, orgID string, ids []string) (retention.CheckResult, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.missing {
		return retention.CheckResult{}, retention.ErrOrganizationNotFound
	}
	result := retention.CheckResult{OrganizationID: orgID, WindowDays: m.policy.ThreadDays, Revision: m.policy.Revision}
	asked := map[string]bool{}
	for _, id := range ids {
		asked[id] = true
	}
	for _, hold := range m.holds {
		switch {
		case !hold.Active():
		case hold.Scope == retention.ScopeOrg:
			result.OrgHeld = true
		case asked[hold.TargetRef]:
			result.HeldThreadIDs = append(result.HeldThreadIDs, hold.TargetRef)
		}
	}
	sort.Strings(result.HeldThreadIDs)
	return result, nil
}

func (m *memoryRetentionStore) ClaimAudit(context.Context, int) ([]retention.AuditRow, error) {
	return nil, nil
}
func (m *memoryRetentionStore) MarkAuditPublished(context.Context, string) error { return nil }
func (m *memoryRetentionStore) MarkAuditFailed(context.Context, string, error) (bool, error) {
	return false, nil
}

// zdrThreads says nothing is retained for the threads it lists.
type zdrThreads map[string]bool

func (z zdrThreads) NothingRetained(_ context.Context, _, threadID string) (bool, error) {
	return z[threadID], nil
}

type retentionHarness struct {
	router *gin.Engine
	store  *memoryRetentionStore
}

// newRetentionHarness mounts the retention routes exactly as server.go does with
// the switch on, behind the REAL service-auth middleware and the REAL membership
// guard. The route table itself is pinned by TestRetentionRoutesFollowTheSwitch.
func newRetentionHarness(t *testing.T, roles map[string]string, threads retention.ThreadRetention) *retentionHarness {
	t.Helper()
	return newRetentionHarnessWithCompliance(t, roles, threads, false)
}

// newRetentionHarnessWithCompliance is the harness with ORG_COMPLIANCE_ROLE_ENABLED
// set as complianceOn says (O-21).
func newRetentionHarnessWithCompliance(t *testing.T, roles map[string]string, threads retention.ThreadRetention, complianceOn bool) *retentionHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(retentionRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryRetentionStore{}
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}, retentionOn: true, complianceRoleOn: complianceOn}
	server.SetRetention(retention.NewService(store, threads, nil))

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	server.registerRetentionRoutes(router.Group("/api/v1"), router.Group("/internal"), guard)
	return &retentionHarness{router: router, store: store}
}

func (h *retentionHarness) do(method, path, userID string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, modelPolicyOrg, "")
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func (h *retentionHarness) doAsSessionCore(body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodPost, retentionCheckPath, bytes.NewReader(body))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", "session-core")
	request.Header.Set("X-Service-Token", retentionSessionToken)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func jsonBody(value any) []byte {
	encoded, _ := json.Marshal(value)
	return encoded
}

func errorCode(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	envelope, ok := decodeBody(t, response)["error"].(map[string]any)
	if !ok {
		t.Fatalf("the refusal must be a typed envelope: %s", response.Body.String())
	}
	if _, ok := envelope["details"].(map[string]any); !ok {
		t.Fatalf("details must be an object: %s", response.Body.String())
	}
	code, _ := envelope["code"].(string)
	return code
}

func TestRetentionRoutesFollowTheSwitch(t *testing.T) {
	routes := []string{
		"GET /api/v1/organizations/:id/retention",
		"PUT /api/v1/organizations/:id/retention",
		"GET /api/v1/organizations/:id/legal-holds",
		"POST /api/v1/organizations/:id/legal-holds",
		"POST /api/v1/organizations/:id/legal-holds/:holdId/release",
		"POST /internal/orgs/:orgId/retention/check",
	}
	for _, value := range []string{"", "1", "TRUE", "true"} {
		t.Setenv(retention.EnvEnabled, value)
		mounted := map[string]bool{}
		for _, route := range NewServer(0, nil, nil, "", "").router.Routes() {
			mounted[route.Method+" "+route.Path] = true
		}
		for _, route := range routes {
			if mounted[route] != (value == "true") {
				t.Errorf("switch=%q: %s mounted=%v", value, route, mounted[route])
			}
		}
	}
}

func TestOnlyOwnerAndAdminSetTheRetentionWindow(t *testing.T) {
	harness := newRetentionHarness(t, map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-comp": "compliance"}, nil)

	for _, user := range []string{"u-member", "u-comp"} {
		if response := harness.do(http.MethodPut, retentionV1Path, user, jsonBody(map[string]any{"expected_revision": 0, "thread_days": 90})); response.Code != http.StatusForbidden || errorCode(t, response) != "forbidden" {
			t.Fatalf("%s must not set the window: status=%d body=%s", user, response.Code, response.Body.String())
		}
	}
	saved := harness.do(http.MethodPut, retentionV1Path, "u-admin", jsonBody(map[string]any{"expected_revision": 0, "thread_days": 90}))
	if saved.Code != http.StatusOK || saved.Header().Get("Cache-Control") != "no-store" || decodeBody(t, saved)["revision"].(float64) != 1 {
		t.Fatalf("admin write: status=%d body=%s", saved.Code, saved.Body.String())
	}
	stale := harness.do(http.MethodPut, retentionV1Path, "u-owner", jsonBody(map[string]any{"expected_revision": 0, "thread_days": nil}))
	if stale.Code != http.StatusConflict || errorCode(t, stale) != "revision_conflict" {
		t.Fatalf("a stale write must be 409: status=%d body=%s", stale.Code, stale.Body.String())
	}
	short := harness.do(http.MethodPut, retentionV1Path, "u-owner", jsonBody(map[string]any{"expected_revision": 1, "thread_days": 7}))
	if short.Code != http.StatusBadRequest || errorCode(t, short) != "invalid_retention_request" {
		t.Fatalf("a window under 30 days must be refused: status=%d body=%s", short.Code, short.Body.String())
	}
	unknown := harness.do(http.MethodPut, retentionV1Path, "u-owner", []byte(`{"expected_revision":1,"thread_days":90,"days":1}`))
	if unknown.Code != http.StatusBadRequest {
		t.Fatalf("an unknown field must be refused: status=%d", unknown.Code)
	}
	if read := harness.do(http.MethodGet, retentionV1Path, "u-member", nil); read.Code != http.StatusForbidden {
		t.Fatalf("a member must not read the window: status=%d", read.Code)
	}
}

func TestHoldsArePlacedAndReleasedByOwnerOrCompliance(t *testing.T) {
	harness := newRetentionHarness(t, map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-comp": "compliance"}, nil)
	threadHold := jsonBody(map[string]any{"scope": "thread", "thread_id": "thr-1", "hold_ref": "Matter 2026-17"})

	for _, user := range []string{"u-admin", "u-member"} {
		if response := harness.do(http.MethodPost, legalHoldsV1Path, user, threadHold); response.Code != http.StatusForbidden {
			t.Fatalf("%s must not place a hold: status=%d", user, response.Code)
		}
	}
	placed := harness.do(http.MethodPost, legalHoldsV1Path, "u-comp", threadHold)
	if placed.Code != http.StatusCreated {
		t.Fatalf("compliance placement: status=%d body=%s", placed.Code, placed.Body.String())
	}
	holdID := decodeBody(t, placed)["hold"].(map[string]any)["id"].(string)
	again := harness.do(http.MethodPost, legalHoldsV1Path, "u-owner", threadHold)
	if again.Code != http.StatusOK || decodeBody(t, again)["existed"] != true {
		t.Fatalf("a second hold on a held thread answers the first: status=%d body=%s", again.Code, again.Body.String())
	}
	if list := harness.do(http.MethodGet, legalHoldsV1Path, "u-admin", nil); list.Code != http.StatusOK || len(decodeBody(t, list)["holds"].([]any)) != 1 {
		t.Fatalf("an admin reads the holds: status=%d body=%s", list.Code, list.Body.String())
	}
	if list := harness.do(http.MethodGet, legalHoldsV1Path, "u-member", nil); list.Code != http.StatusForbidden {
		t.Fatalf("a member must not read the holds: status=%d", list.Code)
	}

	releasePath := legalHoldsV1Path + "/" + holdID + "/release"
	if response := harness.do(http.MethodPost, releasePath, "u-admin", nil); response.Code != http.StatusForbidden {
		t.Fatalf("an admin must not release a hold: status=%d", response.Code)
	}
	if response := harness.do(http.MethodPost, releasePath, "u-owner", nil); response.Code != http.StatusOK {
		t.Fatalf("owner release: status=%d body=%s", response.Code, response.Body.String())
	}
	if response := harness.do(http.MethodPost, releasePath, "u-owner", nil); response.Code != http.StatusConflict || errorCode(t, response) != "legal_hold_already_released" {
		t.Fatalf("a second release must be 409: status=%d body=%s", response.Code, response.Body.String())
	}
	if response := harness.do(http.MethodPost, legalHoldsV1Path+"/lh_"+strings.Repeat("0", 32)+"/release", "u-owner", nil); response.Code != http.StatusNotFound {
		t.Fatalf("an unknown hold must be 404: status=%d", response.Code)
	}
}

// O-21: with ORG_COMPLIANCE_ROLE_ENABLED on, an admin joins the owner and the
// compliance member as a role that places and releases holds; a member and a
// viewer never do. Every placement and release names the acting role in the audit.
func TestAdminPlacesAndReleasesHoldsOnlyWithTheComplianceSwitchOn(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer", "u-comp": "compliance"}
	threadHold := func(thread string) []byte {
		return jsonBody(map[string]any{"scope": "thread", "thread_id": thread, "hold_ref": "Matter 2026-17"})
	}

	off := newRetentionHarnessWithCompliance(t, roles, nil, false)
	if response := off.do(http.MethodPost, legalHoldsV1Path, "u-admin", threadHold("thr-1")); response.Code != http.StatusForbidden {
		t.Fatalf("switch off: an admin must not place a hold: status=%d body=%s", response.Code, response.Body.String())
	}

	on := newRetentionHarnessWithCompliance(t, roles, nil, true)
	for _, user := range []string{"u-member", "u-viewer"} {
		if response := on.do(http.MethodPost, legalHoldsV1Path, user, threadHold("thr-1")); response.Code != http.StatusForbidden {
			t.Fatalf("switch on: %s must not place a hold: status=%d", user, response.Code)
		}
		if response := on.do(http.MethodGet, legalHoldsV1Path, user, nil); response.Code != http.StatusForbidden {
			t.Fatalf("switch on: %s must not read the holds: status=%d", user, response.Code)
		}
	}
	placed := on.do(http.MethodPost, legalHoldsV1Path, "u-admin", threadHold("thr-1"))
	if placed.Code != http.StatusCreated {
		t.Fatalf("switch on: admin placement: status=%d body=%s", placed.Code, placed.Body.String())
	}
	holdID := decodeBody(t, placed)["hold"].(map[string]any)["id"].(string)
	releasePath := legalHoldsV1Path + "/" + holdID + "/release"
	for _, user := range []string{"u-member", "u-viewer"} {
		if response := on.do(http.MethodPost, releasePath, user, nil); response.Code != http.StatusForbidden {
			t.Fatalf("switch on: %s must not release a hold: status=%d", user, response.Code)
		}
	}
	if response := on.do(http.MethodPost, releasePath, "u-admin", nil); response.Code != http.StatusOK {
		t.Fatalf("switch on: admin release: status=%d body=%s", response.Code, response.Body.String())
	}
	if response := on.do(http.MethodPost, legalHoldsV1Path, "u-comp", threadHold("thr-2")); response.Code != http.StatusCreated {
		t.Fatalf("switch on: compliance placement: status=%d", response.Code)
	}
	if got, want := strings.Join(on.store.actorRoles, ","), "place:admin,release:admin,place:compliance"; got != want {
		t.Fatalf("the audit names the acting role: got %q want %q", got, want)
	}
}

func TestThreadHoldOnAZDRThreadIsRefusedWithTheTypedCode(t *testing.T) {
	harness := newRetentionHarness(t, map[string]string{"u-owner": "owner"}, zdrThreads{"thr-zdr": true})
	response := harness.do(http.MethodPost, legalHoldsV1Path, "u-owner", jsonBody(map[string]any{"scope": "thread", "thread_id": "thr-zdr", "hold_ref": "Matter"}))
	if response.Code != http.StatusUnprocessableEntity || errorCode(t, response) != retention.CodeNothingRetained {
		t.Fatalf("a ZDR thread hold must be legal_hold_nothing_retained: status=%d body=%s", response.Code, response.Body.String())
	}
	details := decodeBody(t, response)["error"].(map[string]any)["details"].(map[string]any)
	if details["feature"] != retention.ZDRFeature {
		t.Fatalf("the refusal names the zdr feature: %v", details)
	}
	// An org hold on a ZDR organisation is allowed.
	if org := harness.do(http.MethodPost, legalHoldsV1Path, "u-owner", jsonBody(map[string]any{"scope": "org", "hold_ref": "Matter"})); org.Code != http.StatusCreated {
		t.Fatalf("org hold: status=%d body=%s", org.Code, org.Body.String())
	}
}

// failingThreads is a receipt reader that cannot answer. The hold is placed anyway: an unneeded
// hold deletes nothing, while a refused one could let held content be swept.
type failingThreads struct{}

func (failingThreads) NothingRetained(context.Context, string, string) (bool, error) {
	return false, errors.New("session-core unavailable")
}

func TestThreadHoldsArePlacedWhenTheReceiptIsNotEmptyOrUnknown(t *testing.T) {
	for name, threads := range map[string]retention.ThreadRetention{
		"retained": zdrThreads{},
		"unknown":  failingThreads{},
	} {
		harness := newRetentionHarness(t, map[string]string{"u-owner": "owner"}, threads)
		response := harness.do(http.MethodPost, legalHoldsV1Path, "u-owner", jsonBody(map[string]any{"scope": "thread", "thread_id": "thr-1", "hold_ref": "Matter"}))
		if response.Code != http.StatusCreated {
			t.Fatalf("%s: a thread hold whose receipt is not known to be empty must be placed: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

func TestCheckRouteIsSessionCoresAlone(t *testing.T) {
	harness := newRetentionHarness(t, map[string]string{"u-owner": "owner", "u-comp": "compliance"}, nil)
	harness.do(http.MethodPut, retentionV1Path, "u-owner", jsonBody(map[string]any{"expected_revision": 0, "thread_days": 365}))
	harness.do(http.MethodPost, legalHoldsV1Path, "u-comp", jsonBody(map[string]any{"scope": "thread", "thread_id": "thr-1", "hold_ref": "Matter"}))

	response := harness.doAsSessionCore(jsonBody(map[string]any{"thread_ids": []string{"thr-2", "thr-1"}}))
	if response.Code != http.StatusOK || response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("session-core check: status=%d body=%s", response.Code, response.Body.String())
	}
	body := decodeBody(t, response)
	held, _ := body["held_thread_ids"].([]any)
	if body["window_days"].(float64) != 365 || body["revision"].(float64) != 1 || body["org_held"] != false || len(held) != 1 || held[0] != "thr-1" {
		t.Fatalf("check answer: %v", body)
	}

	// The gateway, even on behalf of the owner, cannot ask the sweeper's question.
	if denied := harness.do(http.MethodPost, retentionCheckPath, "u-owner", jsonBody(map[string]any{"thread_ids": []string{"thr-1"}})); denied.Code != http.StatusForbidden && denied.Code != http.StatusUnauthorized {
		t.Fatalf("the gateway must not reach the check route: status=%d", denied.Code)
	}

	tooMany := make([]string, retention.MaxCheckThreadIDs+1)
	for i := range tooMany {
		tooMany[i] = fmt.Sprintf("thr-%d", i)
	}
	if response := harness.doAsSessionCore(jsonBody(map[string]any{"thread_ids": tooMany})); response.Code != http.StatusBadRequest {
		t.Fatalf("more than %d ids must be refused: status=%d", retention.MaxCheckThreadIDs, response.Code)
	}

	harness.store.missing = true
	if response := harness.doAsSessionCore(jsonBody(map[string]any{"thread_ids": []string{}})); response.Code != http.StatusNotFound {
		t.Fatalf("a missing organisation is a definitive 404: status=%d", response.Code)
	}
}

func TestRetentionScopeIsPinnedToSessionCore(t *testing.T) {
	if _, err := parseServiceCredentials(fmt.Sprintf(`[
  {"principal":"model-gateway","audience":"org-core","token":%q,"scopes":["org:retention-policy:read:any"]}
]`, retentionSessionToken)); err == nil {
		t.Fatal("only session-core may hold org:retention-policy:read:any")
	}
}
