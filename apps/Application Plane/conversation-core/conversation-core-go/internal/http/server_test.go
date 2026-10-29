package http

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"fmt"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

const (
	testGatewaySecret = "gateway-test-secret-at-least-32-bytes"
	testIngestSecret  = "ingest-test-secret-at-least-32-bytes-1"
)

var conversationTestNonce atomic.Uint64

func TestListFilterFromRequestParsesConversationCursor(t *testing.T) {
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)
	ctx.Request = httptest.NewRequest(stdhttp.MethodGet, "/api/v1/conversations?limit=25&cursor_updated=2026-07-10T09%3A00%3A00Z&cursor_id=conversation-older", nil)

	filter := listFilterFromRequest(ctx, "org-1")

	if filter.CursorUpdated == nil || !filter.CursorUpdated.Equal(time.Date(2026, 7, 10, 9, 0, 0, 0, time.UTC)) {
		t.Fatalf("cursor updated = %v, want parsed UTC time", filter.CursorUpdated)
	}
	if filter.CursorID != "conversation-older" {
		t.Fatalf("cursor id = %q, want conversation-older", filter.CursorID)
	}
}

func TestDeliveryUnknownUsesReconciliationRequiredErrorEnvelope(t *testing.T) {
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)

	writeServiceError(ctx, conversation.ErrDeliveryUnknown)

	if recorder.Code != stdhttp.StatusConflict {
		t.Fatalf("status = %d, want 409", recorder.Code)
	}
	body := recorder.Body.String()
	if !strings.Contains(body, `"code":"delivery_unknown"`) || !strings.Contains(body, "may have been submitted") {
		t.Fatalf("body = %s, want honest reconciliation-required envelope", body)
	}
	if strings.Contains(body, "was not sent") {
		t.Fatalf("unknown outcome made a false not-sent claim: %s", body)
	}
}

func TestWriteServiceErrorMapsEveryPublicOutcome(t *testing.T) {
	tests := []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{name: "forbidden", err: conversation.ErrForbidden, status: stdhttp.StatusForbidden, code: "forbidden"},
		{name: "not found", err: conversation.ErrNotFound, status: stdhttp.StatusNotFound, code: "not_found"},
		{name: "conflict", err: conversation.ErrConflict, status: stdhttp.StatusConflict, code: "conflict"},
		{name: "validation", err: fmt.Errorf("%w: bad input", conversation.ErrInvalidInput), status: stdhttp.StatusUnprocessableEntity, code: "validation_error"},
		{name: "unknown", err: conversation.ErrDeliveryUnknown, status: stdhttp.StatusConflict, code: "delivery_unknown"},
		{name: "send failed", err: conversation.ErrSendFailed, status: stdhttp.StatusBadGateway, code: "send_failed"},
		{name: "unavailable", err: conversation.ErrDeliveryUnavailable, status: stdhttp.StatusServiceUnavailable, code: "delivery_unavailable"},
		{name: "internal", err: fmt.Errorf("database unavailable"), status: stdhttp.StatusInternalServerError, code: "internal_error"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			recorder := httptest.NewRecorder()
			ctx, _ := gin.CreateTestContext(recorder)
			writeServiceError(ctx, test.err)
			if recorder.Code != test.status || !strings.Contains(recorder.Body.String(), `"code":"`+test.code+`"`) {
				t.Fatalf("status/body = %d/%s", recorder.Code, recorder.Body.String())
			}
		})
	}
}

func testVerifier(t *testing.T) *delegation.Verifier {
	t.Helper()
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "conversation-core",
		Keys: map[string]string{
			"verevon-gateway":     testGatewaySecret,
			"conversation-ingest": testIngestSecret,
		},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	return verifier
}

func signConversationRequest(t *testing.T, request *stdhttp.Request, body []byte, serviceID, secret, userID, organizationID, role string) {
	t.Helper()
	timestamp := time.Now().UTC().Format(time.RFC3339)
	nonce := fmt.Sprintf("test-nonce-%016d", conversationTestNonce.Add(1))
	digestBytes := sha256.Sum256(body)
	digest := base64.RawURLEncoding.EncodeToString(digestBytes[:])
	canonical := delegation.Canonical(delegation.CanonicalFields{
		ServiceID:      serviceID,
		Audience:       "conversation-core",
		Timestamp:      timestamp,
		Nonce:          nonce,
		Method:         request.Method,
		URI:            request.URL.RequestURI(),
		UserID:         userID,
		OrganizationID: organizationID,
		Role:           role,
		BodySHA256:     digest,
	})
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))

	request.Header.Set(delegation.HeaderServiceID, serviceID)
	request.Header.Set(delegation.HeaderTimestamp, timestamp)
	request.Header.Set(delegation.HeaderNonce, nonce)
	request.Header.Set(delegation.HeaderBodySHA256, digest)
	request.Header.Set(delegation.HeaderSignature, base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
	if userID != "" {
		request.Header.Set(delegation.HeaderUserID, userID)
	}
	if organizationID != "" {
		request.Header.Set(delegation.HeaderOrganizationID, organizationID)
	}
	if role != "" {
		request.Header.Set(delegation.HeaderRole, role)
	}
}

func performRequest(router *gin.Engine, request *stdhttp.Request) *httptest.ResponseRecorder {
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	return recorder
}

func TestConversationAPIRoutesRejectUnsignedAndLegacySharedKeyRequests(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))

	for _, configure := range []func(*stdhttp.Request){
		func(_ *stdhttp.Request) {},
		func(request *stdhttp.Request) { request.Header.Set("x-internal-api-key", "legacy-shared-key") },
	} {
		request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/inboxes", nil)
		configure(request)
		if response := performRequest(router, request); response.Code != stdhttp.StatusUnauthorized {
			t.Fatalf("status = %d, want 401; body=%s", response.Code, response.Body.String())
		}
	}
}

func TestConversationWriteRoutesRejectReadOnlyMembership(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))
	body := []byte(`{"body_text":"hello"}`)
	request := httptest.NewRequest(stdhttp.MethodPost, "/api/v1/conversations/conversation-1/messages", bytes.NewReader(body))
	signConversationRequest(t, request, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "viewer")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want 403; body=%s", response.Code, response.Body.String())
	}
}

func TestFeedbackRouteRejectsReadOnlyMembership(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))
	body := []byte(`{"body_text":"hello","idempotency_key":"demo-feedback-viewer-0001"}`)
	request := httptest.NewRequest(stdhttp.MethodPost, "/api/v1/feedback", bytes.NewReader(body))
	signConversationRequest(t, request, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "viewer")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want 403; body=%s", response.Code, response.Body.String())
	}
}

func TestConversationAdministrativeRoutesRequireOwnerOrAdmin(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))
	body := []byte(`{"name":"macro"}`)
	request := httptest.NewRequest(stdhttp.MethodPost, "/api/v1/ticket-macros", bytes.NewReader(body))
	signConversationRequest(t, request, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "member")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want 403; body=%s", response.Code, response.Body.String())
	}
}

func TestConversationIngestRouteRejectsGatewayAndCrossTenantBody(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))
	body := []byte(`{"org_id":"org-2","idempotency_key":"event-1"}`)

	gatewayRequest := httptest.NewRequest(stdhttp.MethodPost, "/internal/conversation-events", bytes.NewReader(body))
	signConversationRequest(t, gatewayRequest, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "admin")
	if response := performRequest(router, gatewayRequest); response.Code != stdhttp.StatusForbidden {
		t.Fatalf("gateway ingest status = %d, want 403; body=%s", response.Code, response.Body.String())
	}

	ingestRequest := httptest.NewRequest(stdhttp.MethodPost, "/internal/conversation-events", bytes.NewReader(body))
	signConversationRequest(t, ingestRequest, body, "conversation-ingest", testIngestSecret, "", "org-1", "")
	if response := performRequest(router, ingestRequest); response.Code != stdhttp.StatusForbidden {
		t.Fatalf("cross-tenant ingest status = %d, want 403; body=%s", response.Code, response.Body.String())
	}
}

func TestConversationRouterDoesNotExposeAlternateAIActionMutationPaths(t *testing.T) {
	router := newRouter(&Handler{}, testVerifier(t))
	for _, forbidden := range []struct {
		method string
		path   string
	}{
		{stdhttp.MethodGet, "/internal/ai-actions"},
		{stdhttp.MethodPost, "/internal/ai-actions"},
		{stdhttp.MethodPost, "/internal/ai-actions/:id/review"},
		{stdhttp.MethodPost, "/internal/ai-actions/:id/approve"},
		{stdhttp.MethodPost, "/internal/ai-actions/:id/reject"},
		{stdhttp.MethodGet, "/internal/conversations/:id/projection"},
	} {
		for _, route := range router.Routes() {
			if route.Method == forbidden.method && route.Path == forbidden.path {
				t.Fatalf("unexpected alternate route %s %s", route.Method, route.Path)
			}
		}
	}
}

func TestRequireOrgAndActorUseVerifiedPrincipalNotRawHeadersOrQuery(t *testing.T) {
	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.GET("/scope", func(c *gin.Context) {
		principal := delegation.Principal{ServiceID: "verevon-gateway", UserID: "user-1", OrganizationID: "org-1", Role: "member"}
		c.Request = c.Request.WithContext(delegation.WithPrincipal(c.Request.Context(), principal))
		c.JSON(stdhttp.StatusOK, gin.H{"org": requireOrgID(c), "user": actorUserID(c)})
	})

	request := httptest.NewRequest(stdhttp.MethodGet, "/scope?org_id=org-query", nil)
	request.Header.Set("x-org-id", "org-header")
	request.Header.Set("x-user-id", "user-header")
	response := performRequest(router, request)
	if response.Code != stdhttp.StatusOK || response.Body.String() != `{"org":"org-1","user":"user-1"}` {
		t.Fatalf("status/body = %d %s", response.Code, response.Body.String())
	}
}

func TestTrustedMessageActorCannotBeOverriddenByRawIdentityHeaders(t *testing.T) {
	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.GET("/actor", func(c *gin.Context) {
		principal := delegation.Principal{ServiceID: "verevon-gateway", UserID: "user-1", OrganizationID: "org-1", Role: "member"}
		c.Request = c.Request.WithContext(delegation.WithPrincipal(c.Request.Context(), principal))
		name, email := trustedMessageActor(c)
		c.JSON(stdhttp.StatusOK, gin.H{"name": name, "email": email})
	})

	request := httptest.NewRequest(stdhttp.MethodGet, "/actor", nil)
	request.Header.Set("x-user-id", "impersonated-user")
	request.Header.Set("x-user-name", "Impersonated Admin")
	request.Header.Set("x-user-email", "admin@example.invalid")
	response := performRequest(router, request)
	if response.Code != stdhttp.StatusOK || response.Body.String() != `{"email":"","name":"user-1"}` {
		t.Fatalf("status/body = %d %s", response.Code, response.Body.String())
	}
}

func TestAuthorizationMiddlewareAllowsOnlyExpectedRoleAndService(t *testing.T) {
	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.POST(
		"/agent-write",
		requireDelegation(testVerifier(t)),
		requireServicePrincipal("verevon-gateway"),
		requireScopedPrincipal(),
		requireAnyRole("owner", "admin", "member"),
		func(c *gin.Context) { c.Status(stdhttp.StatusNoContent) },
	)

	body := []byte(`{}`)
	request := httptest.NewRequest(stdhttp.MethodPost, "/agent-write", bytes.NewReader(body))
	signConversationRequest(t, request, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "member")
	if response := performRequest(router, request); response.Code != stdhttp.StatusNoContent {
		t.Fatalf("member status = %d, want 204; body=%s", response.Code, response.Body.String())
	}
}
