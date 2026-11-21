package http

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/gin-gonic/gin"
)

const (
	// userCoreEraseTestToken is a low-entropy fixture, like browserBrokerTestToken.
	userCoreEraseTestToken = "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu"
	eraseMemberPath        = "/internal/orgs/" + modelPolicyOrg + "/members/u-member/browser-sites"
)

// eraseRegistry is the org-core credential registry with user-core holding the erase scope
// and browser-broker holding the read scope, so the erase route can be refused to the broker.
func eraseRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"user-core","audience":"org-core","token":%q,"scopes":["org:browser-site:erase:any"]},
  {"principal":"browser-broker","audience":"org-core","token":%q,"scopes":["org:browser-site:read:any"]}
]`, userCoreEraseTestToken, browserBrokerTestToken)
}

// eraseHarness mounts the erase route the way server.go does, behind the real service-auth
// middleware. It has no membership guard: the route takes no acting end user.
type eraseHarness struct {
	router *gin.Engine
	store  *memoryBrowserSiteStore
}

func newEraseHarness(t *testing.T, withService bool) *eraseHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(eraseRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryBrowserSiteStore{}
	server := &Server{}
	if withService {
		server.SetBrowserSites(browsersites.NewService(store, time.Now))
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	router.DELETE("/internal/orgs/:orgId/members/:userId/browser-sites", server.eraseBrowserSiteChoicesInternal)
	return &eraseHarness{router: router, store: store}
}

// erase sends one erase request as the named service principal.
func (h *eraseHarness) erase(principal, token, path string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodDelete, path, nil)
	if principal != "" {
		request.Header.Set("X-Service-Id", principal)
	}
	if token != "" {
		request.Header.Set("X-Service-Token", token)
	}
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

// TestEraseRouteNeedsTheEraseScopeAndAnswersCountsOnly pins the route's access and answer:
// the broker's read scope does not reach it, user-core's erase scope does, the answer is
// no-store and carries counts and no host or user id, and the member's choice is gone.
func TestEraseRouteNeedsTheEraseScopeAndAnswersCountsOnly(t *testing.T) {
	harness := newEraseHarness(t, true)
	ctx := context.Background()
	if _, err := harness.store.PutPref(ctx, modelPolicyOrg, "u-member", 0, browsersites.Pref{
		Host: "shop.example.com", Level: browsersites.LevelAct, ExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("seed a choice: %v", err)
	}

	if response := harness.erase(browserBrokerPrincipal, browserBrokerTestToken, eraseMemberPath); response.Code != http.StatusForbidden {
		t.Fatalf("the broker's read scope must not erase a member's choices: status=%d", response.Code)
	}
	if response := harness.erase("", "", eraseMemberPath); response.Code != http.StatusUnauthorized {
		t.Fatalf("a request without service credentials must be refused: status=%d", response.Code)
	}

	response := harness.erase("user-core", userCoreEraseTestToken, eraseMemberPath)
	if response.Code != http.StatusOK {
		t.Fatalf("user-core erase: status=%d body=%s", response.Code, response.Body.String())
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("the erase answer must be no-store, got %q", response.Header().Get("Cache-Control"))
	}
	body := decodeBody(t, response)
	if body["ok"] != true || body["prefs_deleted"].(float64) != 1 || body["history_redacted"].(float64) != 1 {
		t.Fatalf("erase answer must carry the counts: %v", body)
	}
	if raw := response.Body.String(); strings.Contains(raw, "shop.example.com") || strings.Contains(raw, "u-member") {
		t.Fatalf("the erase answer must carry no host and no user id: %s", raw)
	}
	left, err := harness.store.ListPrefs(ctx, modelPolicyOrg, "u-member")
	if err != nil || len(left.Prefs) != 0 {
		t.Fatalf("the member's choices must be gone after the erase: %+v err=%v", left, err)
	}
}

// TestEraseRouteRefusesTheTombstoneAsAUserID pins that the tombstone cannot be used as a
// member id: it is refused as an invalid request, before any erasure.
func TestEraseRouteRefusesTheTombstoneAsAUserID(t *testing.T) {
	harness := newEraseHarness(t, true)
	response := harness.erase("user-core", userCoreEraseTestToken, "/internal/orgs/"+modelPolicyOrg+"/members/erased/browser-sites")
	if response.Code != http.StatusBadRequest {
		t.Fatalf("the tombstone as a user id must be refused: status=%d body=%s", response.Code, response.Body.String())
	}
}

// TestEraseRouteAnswers503WithoutAService pins that a server with no site service answers
// 503 to an authorised erase, rather than reporting an erasure it did not make.
func TestEraseRouteAnswers503WithoutAService(t *testing.T) {
	harness := newEraseHarness(t, false)
	response := harness.erase("user-core", userCoreEraseTestToken, eraseMemberPath)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("a server without the site service must answer 503: status=%d", response.Code)
	}
}

// TestOnlyUserCoreMayHoldTheEraseScope pins the principal pin: no other principal may hold
// org:browser-site:erase:any in the registry, and user-core may.
func TestOnlyUserCoreMayHoldTheEraseScope(t *testing.T) {
	for _, principal := range []string{"verevon-gateway", "browser-broker", "model-gateway", "execution-core", "audit-core"} {
		registry := fmt.Sprintf(`[{"principal":%q,"audience":"org-core","token":%q,"scopes":["org:browser-site:erase:any"]}]`, principal, userCoreEraseTestToken)
		if _, err := parseServiceCredentials(registry); err == nil {
			t.Fatalf("%s must not be able to hold org:browser-site:erase:any", principal)
		}
	}
	if _, err := parseServiceCredentials(eraseRegistry()); err != nil {
		t.Fatalf("the user-core registry must parse: %v", err)
	}
}
