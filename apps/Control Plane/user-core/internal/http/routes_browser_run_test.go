package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS20-S1: POST /api/v1/internal/spaces/browser-run-decision (pure tier). The
// 200 and entitlement-off legs need a database and live in the Live-suffixed
// tests below (TEST_DATABASE_URL via the shared d12Pool/w2Room harness).

const browserRunPath = "/api/v1/internal/spaces/browser-run-decision"

func validBrowserRunBody() map[string]any {
	return map[string]any{
		"space_ref": "room-1", "thread_id": "thread-1",
		"goal_digest":     "sha256:" + strings.Repeat("a", 64),
		"allowed_domains": []string{"docs.example.org", "fixture.test"},
		"view_scope":      "owner", "max_steps": 20, "max_runtime_s": 300,
		"idempotency_key": "browser-1",
	}
}

func TestBrowserRunRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+browserRunPath] != 1 {
		t.Fatalf("browser-run-decision registered %d times, want 1", counts["POST "+browserRunPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, browserRunPath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue (an unmapped POST would fall through to users:write:self)", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, browserRunPath, nil)); got == "spaces:issue" {
		t.Fatalf("a GET of the issuer path must not map to spaces:issue, got %q", got)
	}
}

func TestBrowserRunRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":              {},
		"user session":         {"auth_method": "user_session", "org_id": "org-1", "user_id": "user-1"},
		"other service":        {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"gateway, unverified":  {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
		"gateway without flag": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			rec := postJSON(engine, browserRunPath, validBrowserRunBody())
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestBrowserRunRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceIssuerHarness(t, nil, gatewayContext())
	rec := postJSON(engine, browserRunPath, validBrowserRunBody())
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

// An empty thread_id (the new-thread case) must pass the shape check rather
// than being refused as a missing field: the handler moves on to evidence
// resolution, which fails closed 503 against the unwired repository here. The
// full 200 leg (claims.thread_mode == "new") is BrowserRunRouteAdmits...Live below.
func TestBrowserRunRouteAcceptsAnEmptyThreadIDAtTheShapeCheck(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	body := validBrowserRunBody()
	delete(body, "thread_id")
	rec := postJSON(engine, browserRunPath, body)
	if rec.Code == http.StatusBadRequest {
		t.Fatalf("an empty thread_id must pass the shape check (new-thread mode), got 400: %s", rec.Body.String())
	}
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 once the shape check passes (%s)", rec.Code, rec.Body.String())
	}
}

func TestBrowserRunRouteRejectsMalformedRequestsBeforeReadingAnyEvidence(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{
		"space_ref", "goal_digest", "allowed_domains", "view_scope", "max_steps", "max_runtime_s", "idempotency_key",
	} {
		t.Run("missing "+field, func(t *testing.T) {
			body := validBrowserRunBody()
			delete(body, field)
			rec := postJSON(engine, browserRunPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
	for name, mutate := range map[string]func(map[string]any){
		"unknown view_scope":     func(b map[string]any) { b["view_scope"] = "admin" },
		"max_steps too low":      func(b map[string]any) { b["max_steps"] = 4 },
		"max_steps too high":     func(b map[string]any) { b["max_steps"] = 41 },
		"max_runtime_s too low":  func(b map[string]any) { b["max_runtime_s"] = 29 },
		"max_runtime_s too high": func(b map[string]any) { b["max_runtime_s"] = 601 },
		"too many domains": func(b map[string]any) {
			b["allowed_domains"] = []string{
				"h1.example.com", "h2.example.com", "h3.example.com", "h4.example.com", "h5.example.com",
				"h6.example.com", "h7.example.com", "h8.example.com", "h9.example.com",
			}
		},
		"malformed domain": func(b map[string]any) { b["allowed_domains"] = []string{"localhost"} },
		"bad goal digest":  func(b map[string]any) { b["goal_digest"] = "sha256:abc" },
	} {
		t.Run(name, func(t *testing.T) {
			body := validBrowserRunBody()
			mutate(body)
			rec := postJSON(engine, browserRunPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "token") {
				t.Fatalf("a 400 must not carry a token: %s", rec.Body.String())
			}
		})
	}
	req := httptest.NewRequest(http.MethodPost, browserRunPath, strings.NewReader("not json"))
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("invalid JSON: status = %d, want 400", rec.Code)
	}
}

// The body decodes strictly (DisallowUnknownFields): an actor, organization,
// role, audience or decision_ref the caller tries to send is a 400, never
// silently dropped.
func TestBrowserRunRouteRejectsUnknownBodyFields(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{"org_id", "user_id", "role", "audience", "decision_ref"} {
		t.Run(field, func(t *testing.T) {
			body := validBrowserRunBody()
			body[field] = "x"
			rec := postJSON(engine, browserRunPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			var response map[string]any
			_ = json.Unmarshal(rec.Body.Bytes(), &response)
			if response["error"] != spaceBrowserRunInvalidMessage {
				t.Fatalf("error = %v, want %q", response["error"], spaceBrowserRunInvalidMessage)
			}
		})
	}
}

func TestBrowserRunDenialMessageIsTheFixedString(t *testing.T) {
	const message = spaceBrowserRunDeniedMessage
	if message != "Space browser run is not authorized" {
		t.Fatalf("spaceBrowserRunDeniedMessage = %q", message)
	}
	evidence := spaces.PersonalThreadDecisionEvidence{
		Membership: spaces.CurrentMembership{
			SpaceRef: "room-1", OrgID: "org-1", SubjectID: "user-1", Kind: spaces.KindRoom, Role: "viewer",
			Revisions: spaces.AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 2, Entitlement: 3},
		},
		RecipientAudienceRef: "audience:room-1:2", RecipientAudienceHash: "sha256:a",
		BrowserEntitled: true, ResourceAuthorizationRef: "control:room-1:browser-run:7",
		Privacy: spaces.PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
	request := spaces.BrowserRunDecisionRequest{
		DecisionRef: "d", ThreadID: "t", GoalDigest: "sha256:" + strings.Repeat("a", 64),
		AllowedDomains: []string{"fixture.test"}, ViewScope: spaces.BrowserViewOwner,
		MaxSteps: 20, MaxRuntimeS: 300, IdempotencyKey: "k", Nonce: "n",
	}

	// Typed: a viewer is below the editor floor.
	_, _, err := spaces.IssueBrowserRunDecision(evidence, request, time.Now().UTC())
	if err == nil {
		t.Fatal("a viewer must be refused")
	}
	rec := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(rec)
	writeSpaceIssuerDenial(c, err, message)
	var body map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &body)
	if rec.Code != http.StatusForbidden || body["error"] != message || body["code"] != string(spaces.DenialRoleBelowFloor) {
		t.Fatalf("typed refusal = %d %v", rec.Code, body)
	}

	// Untyped: a request-shape error (not a typed Denial) is the one-key body.
	malformed := request
	malformed.ViewScope = "admin"
	_, _, err = spaces.IssueBrowserRunDecision(evidence, malformed, time.Now().UTC())
	if err == nil {
		t.Fatal("an invalid view_scope must be refused")
	}
	rec = httptest.NewRecorder()
	c, _ = gin.CreateTestContext(rec)
	writeSpaceIssuerDenial(c, err, message)
	if rec.Code != http.StatusForbidden || strings.TrimSpace(rec.Body.String()) != `{"error":"`+message+`"}` {
		t.Fatalf("untyped refusal = %d %s", rec.Code, rec.Body.String())
	}
}

// WS20-S1 live legs: the real handler against a real repository. Gated on
// TEST_DATABASE_URL via d12Pool (see space_w2_live_seed_test.go / thread_read_decision_live_test.go).

func browserRunBodyFor(room w2Room, threadID string) string {
	b, _ := json.Marshal(map[string]any{
		"space_ref": room.Room, "thread_id": threadID,
		"goal_digest":     "sha256:" + strings.Repeat("a", 64),
		"allowed_domains": []string{"docs.example.org"},
		"view_scope":      "owner", "max_steps": 20, "max_runtime_s": 300,
		"idempotency_key": "ws20s1route-browser-1",
	})
	return string(b)
}

func TestBrowserRunRouteAdmitsAnAuthorizedEditorWithAnEmptyThreadLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws20s1route")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.BrowserEntitled = true })
	s := &Server{}
	s.SetSpaceRepository(repo)

	w := w2Post(s, (*Server).issueBrowserRunDecision, browserRunPath, room.Org, room.Editor, browserRunBodyFor(room, ""))
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var response struct {
		Data struct {
			Claims spaces.BrowserRunClaims `json:"claims"`
			Token  string                  `json:"token"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if response.Data.Token == "" {
		t.Fatal("a 200 must carry a token")
	}
	if response.Data.Claims.ThreadID != "" || response.Data.Claims.ThreadMode != "new" {
		t.Fatalf("claims = %+v, want an empty thread id and thread_mode \"new\"", response.Data.Claims)
	}

	// A named thread signs the existing-thread mode.
	w = w2Post(s, (*Server).issueBrowserRunDecision, browserRunPath, room.Org, room.Editor, browserRunBodyFor(room, "thread-1"))
	if w.Code != http.StatusOK {
		t.Fatalf("existing thread: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	response.Data.Claims = spaces.BrowserRunClaims{}
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if response.Data.Claims.ThreadID != "thread-1" || response.Data.Claims.ThreadMode != "existing" {
		t.Fatalf("claims = %+v, want thread-1 and thread_mode \"existing\"", response.Data.Claims)
	}

	// A viewer is below the editor floor.
	w = w2Post(s, (*Server).issueBrowserRunDecision, browserRunPath, room.Org, room.Viewer, browserRunBodyFor(room, ""))
	if w.Code != http.StatusForbidden {
		t.Fatalf("viewer: want 403, got %d (%s)", w.Code, w.Body.String())
	}
	if !strings.Contains(w.Body.String(), spaceBrowserRunDeniedMessage) {
		t.Fatalf("viewer refusal body = %s", w.Body.String())
	}
	if strings.Contains(w.Body.String(), `"token"`) {
		t.Fatalf("a refusal must carry no token: %s", w.Body.String())
	}
}

func TestBrowserRunRouteRefusesWithoutBrowserEntitlementLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws20s1routeb")
	seedW2Room(t, pool, repo, room, nil) // every entitlement off
	s := &Server{}
	s.SetSpaceRepository(repo)
	w := w2Post(s, (*Server).issueBrowserRunDecision, browserRunPath, room.Org, room.Editor, browserRunBodyFor(room, "thread-1"))
	if w.Code != http.StatusForbidden || !strings.Contains(w.Body.String(), string(spaces.DenialEntitlementOff)) {
		t.Fatalf("entitlement off: %d %s", w.Code, w.Body.String())
	}
}
