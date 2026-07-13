package api

import (
	"errors"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestValidActionIdempotencyKeyContract(t *testing.T) {
	tests := []struct {
		name  string
		value string
		valid bool
	}{
		{name: "minimum length", value: "1234567890123456", valid: true},
		{name: "allowed separators", value: "conversation:org_1.reply-1", valid: true},
		{name: "too short", value: "too-short", valid: false},
		{name: "too long", value: strings.Repeat("a", 201), valid: false},
		{name: "space", value: "conversation:org 1:reply-1", valid: false},
		{name: "slash", value: "conversation/org-1/reply-1", valid: false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := validActionIdempotencyKey(tt.value); got != tt.valid {
				t.Fatalf("validActionIdempotencyKey(%q) = %v, want %v", tt.value, got, tt.valid)
			}
		})
	}
}

func TestActionProviderMessageIDFindsOnlyBoundedKnownFields(t *testing.T) {
	tests := []struct {
		name  string
		value any
		want  string
	}{
		{name: "direct snake case", value: map[string]any{"provider_message_id": " direct-1 "}, want: "direct-1"},
		{name: "nested provider response", value: map[string]any{"data": map[string]any{"messages": []any{map[string]any{"id": "nested-1"}}}}, want: "nested-1"},
		{name: "array response", value: []any{map[string]any{"ignored": true}, map[string]any{"messageId": "array-1"}}, want: "array-1"},
		{name: "non string id", value: map[string]any{"id": 42}, want: ""},
		{name: "unknown field", value: map[string]any{"secret": "must-not-be-persisted"}, want: ""},
		{name: "depth limit", value: map[string]any{"data": map[string]any{"data": map[string]any{"data": map[string]any{"data": map[string]any{"data": map[string]any{"id": "too-deep"}}}}}}, want: ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := actionProviderMessageID(tt.value); got != tt.want {
				t.Fatalf("actionProviderMessageID(%#v) = %q, want %q", tt.value, got, tt.want)
			}
		})
	}
}

func TestActionRequestSHA256RejectsUnserializablePayload(t *testing.T) {
	_, err := actionRequestSHA256(store.Connection{
		ID:             "conn-1",
		OrganizationID: "org-1",
		ProviderKey:    "microsoft",
	}, actionBody{
		Operation: "mail.send",
		Body:      map[string]any{"invalid": make(chan struct{})},
	})
	if err == nil {
		t.Fatal("actionRequestSHA256 error = nil, want serialization failure")
	}
}

func TestActionErrorEnvelopeContract(t *testing.T) {
	tests := []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{name: "missing connection", err: store.ErrNotFound, status: fiber.StatusNotFound, code: "connection_not_found"},
		{name: "actions unavailable", err: errActionUnavailable, status: fiber.StatusServiceUnavailable, code: "actions_unavailable"},
		{name: "idempotency required", err: errActionIdempotencyRequired, status: fiber.StatusBadRequest, code: "idempotency_key_required"},
		{name: "idempotency conflict", err: errActionIdempotencyConflict, status: fiber.StatusConflict, code: "idempotency_conflict"},
		{name: "unknown outcome", err: errActionOutcomeUnknown, status: fiber.StatusConflict, code: "action_outcome_unknown"},
		{name: "pre-provider retryable", err: errActionPreProviderRetryable, status: fiber.StatusServiceUnavailable, code: "action_pre_provider_retryable"},
		{name: "authorization", err: auth.NewError(fiber.StatusForbidden, "approval_required", "approval required"), status: fiber.StatusForbidden, code: "approval_required"},
		{name: "provider failure", err: errors.New("provider unavailable"), status: fiber.StatusBadGateway, code: "action_failed"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			app := fiber.New(fiber.Config{DisableStartupMessage: true})
			app.Get("/", func(c *fiber.Ctx) error { return actionError(c, tt.err) })
			response, err := app.Test(httptest.NewRequest("GET", "/", nil))
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if response.StatusCode != tt.status {
				defer response.Body.Close()
				t.Fatalf("status = %d, want %d", response.StatusCode, tt.status)
			}
			if code := readAPIErrorCode(t, response); code != tt.code {
				t.Fatalf("error code = %q, want %q", code, tt.code)
			}
		})
	}
}
