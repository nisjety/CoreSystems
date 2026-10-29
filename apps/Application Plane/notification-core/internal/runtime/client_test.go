package runtime

import (
	"errors"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// ---------------------------------------------------------------------------
// Stub mode (no NOVU_SECRET_KEY)
// ---------------------------------------------------------------------------

func TestNewNovuAdapterRequiresSecretInNovuMode(t *testing.T) {
	_, err := NewNovuAdapter(Config{Mode: DeliveryModeNovu})
	if err == nil {
		t.Fatal("NewNovuAdapter() error = nil, want missing secret error")
	}
}

func TestDisabledModeDispatchFailsWithoutSyntheticSuccess(t *testing.T) {
	adapter, err := NewNovuAdapter(Config{Mode: DeliveryModeDisabled})
	if err != nil {
		t.Fatalf("NewNovuAdapter() error = %v", err)
	}

	result, err := adapter.Dispatch(t.Context(), notification.DeliveryRequest{
		RequestID:   "req_123",
		RecipientID: "user_123",
		Type:        "comment.mentioned",
		Payload:     map[string]any{"title": "Hello"},
	})
	if result != nil {
		t.Fatalf("Dispatch() result = %#v, want nil", result)
	}
	if !errors.Is(err, ErrDeliveryDisabled) {
		t.Fatalf("Dispatch() error = %v, want ErrDeliveryDisabled", err)
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
//   - req.Payload     → trigger payload + _verevon_* correlation fields
// ---------------------------------------------------------------------------
