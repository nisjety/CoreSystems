package runtime

import (
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// ---------------------------------------------------------------------------
// Stub mode (no NOVU_SECRET_KEY)
// ---------------------------------------------------------------------------

func TestNewNovuAdapterStubModeWhenNoSecretKey(t *testing.T) {
	adapter := NewNovuAdapter(Config{})
	if adapter.client != nil {
		t.Fatal("expected client to be nil in stub mode")
	}
}

func TestStubModeDispatchReturnsNovuProvider(t *testing.T) {
	adapter := NewNovuAdapter(Config{})

	result, err := adapter.Dispatch(t.Context(), notification.DeliveryRequest{
		RequestID:   "req_123",
		RecipientID: "user_123",
		Type:        "comment.mentioned",
		Payload:     map[string]any{"title": "Hello"},
	})
	if err != nil {
		t.Fatalf("Dispatch() error = %v", err)
	}
	if result == nil {
		t.Fatal("Dispatch() result = nil, want non-nil")
	}
	if result.Provider != notification.ProviderNovu {
		t.Fatalf("Provider = %q, want %q", result.Provider, notification.ProviderNovu)
	}
	if strings.TrimSpace(result.ProviderRequestID) == "" {
		t.Fatal("ProviderRequestID = empty, want non-empty")
	}
}

func TestStubModeDispatchGeneratesUniqueIDs(t *testing.T) {
	adapter := NewNovuAdapter(Config{})

	seen := make(map[string]struct{}, 20)
	for i := range 20 {
		result, err := adapter.Dispatch(t.Context(), notification.DeliveryRequest{
			RequestID:   "req_123",
			RecipientID: "user_123",
			Type:        "comment.mentioned",
		})
		if err != nil {
			t.Fatalf("Dispatch()[%d] error = %v", i, err)
		}
		if _, exists := seen[result.ProviderRequestID]; exists {
			t.Fatalf("Dispatch() generated duplicate ProviderRequestID %q", result.ProviderRequestID)
		}
		seen[result.ProviderRequestID] = struct{}{}
	}
}

func TestStubModeDispatchWithCustomGenerator(t *testing.T) {
	adapter := &NovuAdapter{generateID: func() string { return "fixed_id" }}

	result, err := adapter.Dispatch(t.Context(), notification.DeliveryRequest{
		RequestID:   "req_123",
		RecipientID: "user_123",
		Type:        "comment.mentioned",
	})
	if err != nil {
		t.Fatalf("Dispatch() error = %v", err)
	}
	if result.ProviderRequestID != "fixed_id" {
		t.Fatalf("ProviderRequestID = %q, want %q", result.ProviderRequestID, "fixed_id")
	}
}

// ---------------------------------------------------------------------------
// Production mode is tested via integration tests.
// Set NOVU_SECRET_KEY and run:
//
//	NOVU_SECRET_KEY=your_key go test ./internal/runtime/... -run Integration -v
//
// The adapter wires directly to the Novu v3 API:
//   - req.Type        → WorkflowID  (must match a workflow in your Novu dashboard)
//   - req.RecipientID → SubscriberID
//   - req.RequestID   → Novu idempotency key  (Novu deduplicates on this)
//   - req.Payload     → trigger payload + _velion_* correlation fields
// ---------------------------------------------------------------------------
