package notification

import (
	"context"
	"testing"
	"time"
)

func TestAcceptValidRequest(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_test"}}
	publisher := &fakePublisher{}
	service := NewServiceLegacy(repository, runtimeClient, publisher, func() string { return "req_test" }, func() time.Time {
		return time.Date(2026, time.March, 31, 10, 0, 0, 0, time.UTC)
	})

	acceptedRequest, err := service.Accept(context.Background(), Request{
		RecipientID: "user_123",
		Type:        "notification.created",
		Payload: map[string]any{
			"title": "Hello",
		},
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if acceptedRequest.RequestID != "req_test" {
		t.Fatalf("RequestID = %q, want %q", acceptedRequest.RequestID, "req_test")
	}
	if acceptedRequest.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q", acceptedRequest.Status, StatusSubmitted)
	}
}

func TestAcceptMissingRecipientID(t *testing.T) {
	service := NewServiceLegacy(nil, nil, nil, nil, nil)

	acceptedRequest, err := service.Accept(context.Background(), Request{Type: "notification.created"})
	if err == nil {
		t.Fatal("Accept() error = nil, want error")
	}
	if acceptedRequest != nil {
		t.Fatalf("Accept() acceptedRequest = %#v, want nil", acceptedRequest)
	}
	if !IsValidationError(err) {
		t.Fatalf("IsValidationError(%v) = false, want true", err)
	}
	if err.Error() != "recipient_id is required" {
		t.Fatalf("error = %q, want %q", err.Error(), "recipient_id is required")
	}
}

func TestAcceptWhitespaceRecipientID(t *testing.T) {
	service := NewServiceLegacy(nil, nil, nil, nil, nil)

	_, err := service.Accept(context.Background(), Request{RecipientID: "   ", Type: "notification.created"})
	if err == nil {
		t.Fatal("Accept() error = nil, want error")
	}
	if !IsValidationError(err) {
		t.Fatalf("IsValidationError(%v) = false, want true", err)
	}
}

func TestAcceptMissingType(t *testing.T) {
	service := NewServiceLegacy(nil, nil, nil, nil, nil)

	acceptedRequest, err := service.Accept(context.Background(), Request{RecipientID: "user_123"})
	if err == nil {
		t.Fatal("Accept() error = nil, want error")
	}
	if acceptedRequest != nil {
		t.Fatalf("Accept() acceptedRequest = %#v, want nil", acceptedRequest)
	}
	if !IsValidationError(err) {
		t.Fatalf("IsValidationError(%v) = false, want true", err)
	}
	if err.Error() != "type is required" {
		t.Fatalf("error = %q, want %q", err.Error(), "type is required")
	}
}

func TestAcceptWhitespaceType(t *testing.T) {
	service := NewServiceLegacy(nil, nil, nil, nil, nil)

	_, err := service.Accept(context.Background(), Request{RecipientID: "user_123", Type: "   "})
	if err == nil {
		t.Fatal("Accept() error = nil, want error")
	}
	if !IsValidationError(err) {
		t.Fatalf("IsValidationError(%v) = false, want true", err)
	}
}
