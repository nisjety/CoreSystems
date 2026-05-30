package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

type testRepository struct {
	stored map[string]*notification.StoredRequest
}

func newTestRepository() *testRepository {
	return &testRepository{stored: make(map[string]*notification.StoredRequest)}
}

func (r *testRepository) FindByIdempotencyKey(_ context.Context, idempotencyKey string) (*notification.StoredRequest, error) {
	for _, storedRequest := range r.stored {
		if storedRequest.IdempotencyKey == idempotencyKey {
			cloned := *storedRequest
			return &cloned, nil
		}
	}
	return nil, notification.ErrNotFound
}

func (r *testRepository) Create(_ context.Context, params notification.CreateRequestParams) (*notification.StoredRequest, error) {
	storedRequest := &notification.StoredRequest{
		ID:             params.ID,
		IdempotencyKey: params.IdempotencyKey,
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

func newTestNotificationService(runtimeErr error) *notification.Service {
	providerRequestID := "novu_req_test"
	if runtimeErr != nil {
		providerRequestID = ""
	}

	// U5-2: NewService is now variadic; old 5-arg shape lives as
	// NewServiceLegacy until the test suite migrates to options.
	return notification.NewServiceLegacy(
		newTestRepository(),
		&testRuntimeClient{providerRequestID: providerRequestID, err: runtimeErr},
		noopPublisher{},
		func() string { return "req_test" },
		func() time.Time { return time.Date(2026, time.March, 31, 13, 0, 0, 0, time.UTC) },
	)
}

func TestHealthEndpoint(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	response, err := stdhttp.Get(server.URL + "/health")
	if err != nil {
		t.Fatalf("GET /health error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusOK {
		t.Fatalf("GET /health status = %d, want %d", response.StatusCode, stdhttp.StatusOK)
	}
}

func TestCreateNotificationRequestWithValidKey(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	requestBody := map[string]any{
		"recipient_id": "user_123",
		"type":         "notification.created",
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
	request.Header.Set("x-internal-api-key", "internal-secret")

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

func TestCreateNotificationRequestRuntimeFailureReturnsBadGateway(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(errors.New("novu runtime unavailable"))})
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-internal-api-key", "internal-secret")

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
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusUnauthorized {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusUnauthorized)
	}
}

func TestCreateNotificationRequestWrongKey(t *testing.T) {
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	requestBody := []byte(`{"recipient_id":"user_123","type":"notification.created","payload":{"title":"Hello"}}`)
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(requestBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-internal-api-key", "wrong-key")

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
	server := httptest.NewServer(newRouter(handler, "internal-secret"))
	defer server.Close()

	oversizedBody := `{"recipient_id":"user_123","type":"notification.created","payload":{"message":"` + strings.Repeat("a", 1<<20) + `"}}`
	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", strings.NewReader(oversizedBody))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-internal-api-key", "internal-secret")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()

	if response.StatusCode != stdhttp.StatusRequestEntityTooLarge {
		t.Fatalf("POST /api/v1/notification-requests status = %d, want %d", response.StatusCode, stdhttp.StatusRequestEntityTooLarge)
	}
}
