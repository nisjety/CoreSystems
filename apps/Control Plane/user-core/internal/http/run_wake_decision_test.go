package http

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS08-S1: POST /api/v1/internal/spaces/run-wake-decision (pure tier). The 403
// and 200 legs need a database and live in run_wake_decision_live_test.go.

const runWakePath = "/api/v1/internal/spaces/run-wake-decision"

// spaceIssuerHarness mounts the real Space route group (core + feature routes)
// behind a middleware that stands in for the service-auth layer by setting the
// context keys it would set.
func spaceIssuerHarness(t *testing.T, repo *spaces.Repository, ctx map[string]any) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	group.Use(func(c *gin.Context) {
		for key, value := range ctx {
			c.Set(key, value)
		}
		c.Next()
	})
	server.registerSpaceRoutes(group)
	return engine
}

func gatewayContext() map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": verevonGatewayPrincipal,
		"delegation_verified": true, "org_id": "org-1", "user_id": "user-1",
	}
}

func postJSON(engine *gin.Engine, path string, body any) *httptest.ResponseRecorder {
	raw, _ := json.Marshal(body)
	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	return rec
}

func validRunWakeBody() map[string]any {
	return map[string]any{
		"space_ref": "room-1", "thread_id": "thread-1", "trigger_message_id": "message-9",
		"content_digest": "sha256:" + strings.Repeat("a", 64), "agent_subject_id": "agent-7",
		"turn_origin": "direct", "idempotency_key": "wake-1",
	}
}

func TestRunWakeRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+runWakePath] != 1 {
		t.Fatalf("run-wake-decision registered %d times, want 1", counts["POST "+runWakePath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, runWakePath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue (an unmapped POST would fall through to users:write:self)", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	// Neighbouring paths keep their own mapping.
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, runWakePath, nil)); got == "spaces:issue" {
		t.Fatalf("a GET of the issuer path must not map to spaces:issue, got %q", got)
	}
}

func TestRunWakeRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":              {},
		"user session":         {"auth_method": "user_session", "org_id": "org-1", "user_id": "user-1"},
		"other service":        {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"gateway, unverified":  {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
		"gateway without flag": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			rec := postJSON(engine, runWakePath, validRunWakeBody())
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestRunWakeRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceIssuerHarness(t, nil, gatewayContext())
	rec := postJSON(engine, runWakePath, validRunWakeBody())
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

func TestRunWakeRouteRejectsMalformedRequestsBeforeReadingAnyEvidence(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{
		"space_ref", "thread_id", "trigger_message_id", "content_digest", "agent_subject_id", "turn_origin", "idempotency_key",
	} {
		t.Run("missing "+field, func(t *testing.T) {
			body := validRunWakeBody()
			delete(body, field)
			rec := postJSON(engine, runWakePath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
	for name, mutate := range map[string]func(map[string]any){
		"ambient origin":    func(b map[string]any) { b["turn_origin"] = "ambient" },
		"automation origin": func(b map[string]any) { b["turn_origin"] = "automation" },
		"unknown origin":    func(b map[string]any) { b["turn_origin"] = "telepathy" },
		"short digest":      func(b map[string]any) { b["content_digest"] = "sha256:abc" },
		"unprefixed digest": func(b map[string]any) { b["content_digest"] = strings.Repeat("a", 71) },
		"blank agent":       func(b map[string]any) { b["agent_subject_id"] = "  " },
	} {
		t.Run(name, func(t *testing.T) {
			body := validRunWakeBody()
			mutate(body)
			rec := postJSON(engine, runWakePath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "token") {
				t.Fatalf("a 400 must not carry a token: %s", rec.Body.String())
			}
		})
	}
	req := httptest.NewRequest(http.MethodPost, runWakePath, strings.NewReader("not json"))
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("invalid JSON: status = %d, want 400", rec.Code)
	}
}

func TestRunWakeDenialMessageIsTheSingleFixedString(t *testing.T) {
	// The BFF maps this exact string to the typed wake_not_authorized
	// (wake_denial_is_typed_at_the_bff is the gateway-side pair, WS08-S4).
	// A refused issue is a 403 {error: <fixed>} plus, for a typed refusal, the
	// closed-vocabulary code.
	const message = "Space run wake is not authorized"
	evidence := spaces.PersonalThreadDecisionEvidence{
		Membership: spaces.CurrentMembership{
			SpaceRef: "room-1", OrgID: "org-1", SubjectID: "user-1", Kind: spaces.KindRoom, Role: "viewer",
			Revisions: spaces.AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 2, Entitlement: 3},
		},
		RecipientAudienceRef: "audience:room-1:2", RecipientAudienceHash: "sha256:a",
		ThreadCreateEntitled: true, ResourceAuthorizationRef: "control:room-1:run-wake:7",
		Privacy: spaces.PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
	request := spaces.RunWakeDecisionRequest{
		DecisionRef: "d", ThreadID: "t", TriggerMessageID: "m", ContentDigest: "sha256:" + strings.Repeat("a", 64),
		AgentSubjectID: "agent-7", TurnOrigin: spaces.TurnOriginDirect, IdempotencyKey: "k", Nonce: "n",
	}
	roster := []spaces.RosterMember{{SubjectType: "service", SubjectID: "agent-7", Role: "editor"}}

	// Typed: a viewer is below the editor floor.
	_, err := spaces.IssueRunWakeDecision(evidence, roster, request, time.Now().UTC())
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

	// Untyped: an agent the room never bound is exactly the one-key body.
	evidence.Membership.Role = "editor"
	_, err = spaces.IssueRunWakeDecision(evidence, nil, request, time.Now().UTC())
	if err == nil {
		t.Fatal("an unbound agent must be refused")
	}
	rec = httptest.NewRecorder()
	c, _ = gin.CreateTestContext(rec)
	writeSpaceIssuerDenial(c, err, message)
	if rec.Code != http.StatusForbidden || strings.TrimSpace(rec.Body.String()) != `{"error":"Space run wake is not authorized"}` {
		t.Fatalf("untyped refusal = %d %s", rec.Code, rec.Body.String())
	}
}
