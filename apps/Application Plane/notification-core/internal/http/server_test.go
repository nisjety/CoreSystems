package http

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/feed"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
)

const testDelegationSecret = "gateway-test-secret-at-least-32-bytes"

var testNonceCounter atomic.Uint64

func newTestVerifier(t *testing.T) *delegation.Verifier {
	t.Helper()
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "notification-core",
		Keys:     map[string]string{"verevon-gateway": testDelegationSecret},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	return verifier
}

func signTestRequest(t *testing.T, request *stdhttp.Request, body []byte, secret, userID, organizationID, role string) {
	t.Helper()
	timestamp := time.Now().UTC().Format(time.RFC3339)
	nonce := fmt.Sprintf("test-nonce-%016d", testNonceCounter.Add(1))
	digestBytes := sha256.Sum256(body)
	digest := base64.RawURLEncoding.EncodeToString(digestBytes[:])
	canonical := delegation.Canonical(delegation.CanonicalFields{
		ServiceID:      "verevon-gateway",
		Audience:       "notification-core",
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

	request.Header.Set(delegation.HeaderServiceID, "verevon-gateway")
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

type testRepository struct {
	stored map[string]*notification.StoredRequest
}

func newTestRepository() *testRepository {
	return &testRepository{stored: make(map[string]*notification.StoredRequest)}
}

func (r *testRepository) FindByIdempotencyKey(_ context.Context, organizationID, idempotencyKey string) (*notification.StoredRequest, error) {
	for _, storedRequest := range r.stored {
		if storedRequest.OrganizationID == organizationID && storedRequest.IdempotencyKey == idempotencyKey {
			cloned := *storedRequest
			return &cloned, nil
		}
	}
	return nil, notification.ErrNotFound
}

func (r *testRepository) Create(_ context.Context, params notification.CreateRequestParams) (*notification.StoredRequest, error) {
	storedRequest := &notification.StoredRequest{
		ID:             params.ID,
		OrganizationID: params.OrganizationID,
		IdempotencyKey: params.IdempotencyKey,
		RequestSHA256:  params.RequestSHA256,
		RetentionMode:  params.RetentionMode,
		RecipientKind:  params.RecipientKind,
		RecipientID:    params.RecipientID,
		Type:           params.Type,
		Payload:        params.Payload,
		Source:         params.Source,
		Status:         params.Status,
		Provider:       params.Provider,
		CreatedAt:      params.OccurredAt,
		UpdatedAt:      params.OccurredAt,
	}
	r.stored[storedRequest.ID] = storedRequest
	cloned := *storedRequest
	return &cloned, nil
}

func (r *testRepository) MarkSubmitted(_ context.Context, requestID string, providerRequestID string, occurredAt time.Time) (*notification.StoredRequest, error) {
	storedRequest, ok := r.stored[requestID]
	if !ok {
		return nil, notification.ErrNotFound
	}
	storedRequest.Status = notification.StatusSubmitted
	storedRequest.ProviderRequestID = providerRequestID
	storedRequest.SubmittedAt = &occurredAt
	storedRequest.UpdatedAt = occurredAt
	cloned := *storedRequest
	return &cloned, nil
}

func (r *testRepository) MarkFailed(_ context.Context, requestID string, failureMessage string, occurredAt time.Time) (*notification.StoredRequest, error) {
	storedRequest, ok := r.stored[requestID]
	if !ok {
		return nil, notification.ErrNotFound
	}
	storedRequest.Status = notification.StatusFailed
	storedRequest.ErrorMessage = failureMessage
	storedRequest.FailedAt = &occurredAt
	storedRequest.UpdatedAt = occurredAt
	cloned := *storedRequest
	return &cloned, nil
}

type testRuntimeClient struct {
	providerRequestID string
	err               error
}

func (c *testRuntimeClient) Dispatch(_ context.Context, _ notification.DeliveryRequest) (*notification.DispatchResult, error) {
	if c.err != nil {
		return nil, c.err
	}
	return &notification.DispatchResult{Provider: notification.ProviderNovu, ProviderRequestID: c.providerRequestID}, nil
}

type noopPublisher struct{}

func (noopPublisher) Publish(_ context.Context, _ string, _ any) error {
	return nil
}

type deniedRecipientAuthorizer struct{}

func (deniedRecipientAuthorizer) ResolveUser(context.Context, string, string) (string, error) {
	return "", subscribers.ErrNotFound
}

type testRecipientResolver struct{}

func (testRecipientResolver) ResolveRecipient(_ context.Context, organizationID string, recipient notification.Recipient) (*notification.ResolvedRecipient, error) {
	return &notification.ResolvedRecipient{
		Kind:                 recipient.Kind,
		ID:                   recipient.ID,
		ProviderSubscriberID: "provider:" + organizationID + ":" + recipient.ID,
	}, nil
}

func newTestNotificationService(runtimeErr error) *notification.Service {
	providerRequestID := "novu_req_test"
	if runtimeErr != nil {
		providerRequestID = ""
	}

	return notification.NewService(
		newTestRepository(),
		&testRuntimeClient{providerRequestID: providerRequestID, err: runtimeErr},
		noopPublisher{},
		notification.WithIDGenerator(func() string { return "req_test" }),
		notification.WithNow(func() time.Time { return time.Date(2026, time.March, 31, 13, 0, 0, 0, time.UTC) }),
		notification.WithRecipientResolver(testRecipientResolver{}),
	)
}

func TestHealthEndpoint(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core", DeliveryMode: "disabled"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	response, err := stdhttp.Get(server.URL + "/health")
	if err != nil {
		t.Fatalf("GET /health error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusOK {
		t.Fatalf("GET /health status = %d, want %d", response.StatusCode, stdhttp.StatusOK)
	}

	var payload struct {
		DeliveryMode   string `json:"delivery_mode"`
		DeliveryStatus string `json:"delivery_status"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		t.Fatalf("Decode /health response error = %v", err)
	}
	if payload.DeliveryMode != "disabled" || payload.DeliveryStatus != "disabled" {
		t.Fatalf("delivery state = (%q, %q), want disabled", payload.DeliveryMode, payload.DeliveryStatus)
	}
}

func TestReadyEndpointReflectsDeliveryMode(t *testing.T) {
	tests := []struct {
		name       string
		mode       string
		wantStatus int
		wantState  string
	}{
		{name: "disabled", mode: "disabled", wantStatus: stdhttp.StatusServiceUnavailable, wantState: "disabled"},
		{name: "novu", mode: "novu", wantStatus: stdhttp.StatusOK, wantState: "ready"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			handler := NewHandler(&config.Config{ServiceName: "notification-core", DeliveryMode: test.mode}, HandlerDeps{Notifications: newTestNotificationService(nil)})
			server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
			defer server.Close()

			response, err := stdhttp.Get(server.URL + "/ready")
			if err != nil {
				t.Fatalf("GET /ready error = %v", err)
			}
			defer response.Body.Close()
			if response.StatusCode != test.wantStatus {
				t.Fatalf("GET /ready status = %d, want %d", response.StatusCode, test.wantStatus)
			}
			var payload struct {
				DeliveryStatus string `json:"delivery_status"`
			}
			if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
				t.Fatalf("Decode /ready response error = %v", err)
			}
			if payload.DeliveryStatus != test.wantState {
				t.Fatalf("delivery_status = %q, want %q", payload.DeliveryStatus, test.wantState)
			}
		})
	}
}

func TestCreateNotificationRequestWithValidKey(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := map[string]any{
		"organization_id": "org_123",
		"recipient": map[string]any{
			"kind": "user",
			"id":   "user_123",
		},
		"type": "notification.created",
		"payload": map[string]any{
			"title": "Hello",
		},
	}
	encodedBody, err := json.Marshal(requestBody)
	if err != nil {
		t.Fatalf("json.Marshal error = %v", err)
	}

	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(encodedBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, encodedBody, testDelegationSecret, "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusAccepted {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusAccepted)
	}

	var payload struct {
		RequestID string `json:"request_id"`
		Status    string `json:"status"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		t.Fatalf("Decode response error = %v", err)
	}
	if payload.RequestID == "" {
		t.Fatal("request_id = empty, want non-empty")
	}
	if payload.Status != notification.StatusSubmitted {
		t.Fatalf("status = %q, want %q", payload.Status, notification.StatusSubmitted)
	}
}

func TestCreateNotificationRequestRejectsLegacyUnscopedRecipient(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, testDelegationSecret, "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusBadRequest {
		t.Fatalf("legacy unscoped request status = %d, want %d", response.StatusCode, stdhttp.StatusBadRequest)
	}
}

func TestCreateNotificationRequestAcceptsTenantScopedTypedUser(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{
		"organization_id":"org_123",
		"recipient":{"kind":"user","id":"user_123"},
		"type":"notification.created",
		"payload":{"title":"Hello"}
	}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, testDelegationSecret, "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusAccepted {
		t.Fatalf("tenant-scoped typed request status = %d, want %d", response.StatusCode, stdhttp.StatusAccepted)
	}
}

func TestCreateNotificationRequestRuntimeFailureReturnsBadGateway(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(errors.New("novu runtime unavailable"))})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"organization_id":"org_123","recipient":{"kind":"user","id":"user_123"},"type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, testDelegationSecret, "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusBadGateway {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusBadGateway)
	}

	var payload struct {
		RequestID string `json:"request_id"`
		Status    string `json:"status"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		t.Fatalf("Decode response error = %v", err)
	}
	if payload.RequestID == "" {
		t.Fatal("request_id = empty, want non-empty")
	}
	if payload.Status != notification.StatusFailed {
		t.Fatalf("status = %q, want %q", payload.Status, notification.StatusFailed)
	}
}

func TestCreateNotificationRequestMissingKey(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-internal-api-key", "legacy-shared-key-must-not-authorize")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusUnauthorized {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusUnauthorized)
	}
}

func TestFeedRejectsSignedGatewayScopeWithoutActiveMembership(t *testing.T) {
	handler := NewHandler(
		&config.Config{ServiceName: "notification-core"},
		HandlerDeps{
			Feed:       feed.NewService(nil),
			Recipients: deniedRecipientAuthorizer{},
		},
	)
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodGet, server.URL+"/notifications", nil)
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	signTestRequest(t, request, nil, testDelegationSecret, "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("GET /notifications error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusForbidden {
		t.Fatalf("GET /notifications status = %d, want %d", response.StatusCode, stdhttp.StatusForbidden)
	}
}

func TestServicePrincipalsHaveExplicitNotificationTypeAllowlists(t *testing.T) {
	for _, notificationType := range []string{
		"ticket.assigned",
		"ticket.triaged",
		"sla.warning",
		"sla.breach",
	} {
		if !isNotificationTypeAuthorized("support-worker", notificationType) {
			t.Fatalf("support-worker type %q rejected, want authorized", notificationType)
		}
	}
	if isNotificationTypeAuthorized("support-worker", "billing.payment_succeeded") {
		t.Fatal("support-worker can request an unrelated notification type")
	}
	if isNotificationTypeAuthorized("insight-core", "daily_brief") {
		t.Fatal("insight-core remains authorized without an authoritative user subscription mapping")
	}
	if isNotificationTypeAuthorized("verevon-gateway", "ticket.assigned") {
		t.Fatal("gateway has no direct notification-dispatch workflow")
	}
}

func TestCreateNotificationRequestRejectsDelegatedOrganizationMismatch(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"organization_id":"org_2","recipient":{"kind":"user","id":"user_123"},"type":"notification.created","payload":{}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, testDelegationSecret, "user_123", "org_1", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST notification request error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.StatusCode, stdhttp.StatusForbidden)
	}
}

func TestCreateNotificationRequestRejectsGatewayTargetingAnotherUser(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"organization_id":"org_1","recipient":{"kind":"user","id":"user_2"},"type":"notification.created","payload":{}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, testDelegationSecret, "user_1", "org_1", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST notification request error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.StatusCode, stdhttp.StatusForbidden)
	}
}

func TestCreateNotificationRequestWrongKey(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, requestBody, "wrong-test-secret-at-least-32-bytes", "user_123", "org_123", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusUnauthorized {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusUnauthorized)
	}
}

func TestCreateNotificationRequestTooLarge(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	oversizedBody := `{"recipient_id":"user_123","type":"notification.created","payload":{"message":"` + strings.Repeat("a", 1<<20) + `"}}`
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", strings.NewReader(oversizedBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusRequestEntityTooLarge {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusRequestEntityTooLarge)
	}
}

func TestFeedRequiresOrganizationAndUserScopeBeforeHandler(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodGet, server.URL+"/notifications", nil)
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("x-user-id", "user-1")
	signTestRequest(t, request, nil, testDelegationSecret, "user-1", "", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("GET /notifications error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusBadRequest {
		t.Fatalf("GET /notifications status = %d, want %d", response.StatusCode, stdhttp.StatusBadRequest)
	}
}

func TestChannelConfigurationDoesNotAcceptOrganizationFromQuery(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodGet, server.URL+"/channels/config?org_id=forged-org", nil)
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	signTestRequest(t, request, nil, testDelegationSecret, "user-1", "", "admin")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("GET /channels/config error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusBadRequest {
		t.Fatalf("GET /channels/config status = %d, want %d", response.StatusCode, stdhttp.StatusBadRequest)
	}
}

func TestChannelConfigurationRequiresDelegatedAdminRole(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodGet, server.URL+"/channels/config", nil)
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	signTestRequest(t, request, nil, testDelegationSecret, "user-1", "org-1", "member")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("GET /channels/config error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusForbidden {
		t.Fatalf("GET /channels/config status = %d, want %d", response.StatusCode, stdhttp.StatusForbidden)
	}
}

func TestLegacyRecipientUpsertRouteIsRemoved(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/internal/recipients/upsert", strings.NewReader(`{"user_id":"user-1","org_id":"org-1"}`))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /internal/recipients/upsert error = %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != stdhttp.StatusNotFound {
		t.Fatalf("POST /internal/recipients/upsert status = %d, want %d", response.StatusCode, stdhttp.StatusNotFound)
	}
}
