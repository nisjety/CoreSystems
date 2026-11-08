package runtime

import (
	"context"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// recordingDispatcher is a provider adapter double that records the types it was asked to send.
type recordingDispatcher struct {
	types []string
}

func (d *recordingDispatcher) Dispatch(_ context.Context, req notification.DeliveryRequest) (*notification.DispatchResult, error) {
	d.types = append(d.types, req.Type)
	return &notification.DispatchResult{Provider: notification.ProviderNovu, ProviderRequestID: "novu-txn-1"}, nil
}

// attentionDeliveryRequest is the request the notification service hands the runtime for one waiting state.
func attentionDeliveryRequest(notificationType string) notification.DeliveryRequest {
	return notification.DeliveryRequest{
		RequestID:      "req_1",
		OrganizationID: "org-1",
		RecipientKind:  notification.RecipientKindUser,
		RecipientID:    "user-1",
		Type:           notificationType,
		Payload: map[string]any{
			"title":       "Approval needed",
			"body":        "A run in one of your chats is waiting for your approval.",
			"message_key": notificationType,
			"cta_href":    "/chat?thread=thread-9",
			"cta_label":   "Open chat",
			"thread_id":   "thread-9",
			"run_id":      "run-abc",
			"kind":        "needs_approval",
		},
		Source:        "capability-core",
		RetentionMode: notification.RetentionModeStandard,
	}
}

func TestNativeAdapterDispatchesNativeTypeUnderTheNativeProvider(t *testing.T) {
	result, err := NewNativeAdapter().Dispatch(t.Context(), attentionDeliveryRequest("modelplane.needs_approval"))
	if err != nil {
		t.Fatalf("Dispatch() error = %v", err)
	}
	if result.Provider != notification.ProviderNative || result.ProviderRequestID != "native:req_1" {
		t.Fatalf("result = %+v, want provider native and request id native:req_1", result)
	}
}

func TestNativeAdapterRefusesTypesOutsideTheNativeSet(t *testing.T) {
	for _, notificationType := range []string{"modelplane.run_completed", "ticket.assigned"} {
		result, err := NewNativeAdapter().Dispatch(t.Context(), attentionDeliveryRequest(notificationType))
		if result != nil || err == nil {
			t.Fatalf("Dispatch(%s) = (%+v, %v), want a refusal", notificationType, result, err)
		}
	}
}

func TestNativeAdapterRefusesZeroRetention(t *testing.T) {
	request := attentionDeliveryRequest("modelplane.needs_approval")
	request.RetentionMode = notification.RetentionModeZDR
	if result, err := NewNativeAdapter().Dispatch(t.Context(), request); result != nil || err == nil {
		t.Fatalf("Dispatch() = (%+v, %v), want a zero-retention refusal", result, err)
	}
}

// TestAttentionNotificationsAreContentFree pins the content-free boundary: the fixed copy and opaque ids
// pass, and any field a run could fill with its own text or arguments is refused before it reaches the feed.
func TestAttentionNotificationsAreContentFree(t *testing.T) {
	adapter := NewNativeAdapter()
	if _, err := adapter.Dispatch(t.Context(), attentionDeliveryRequest("modelplane.plan_ready")); err != nil {
		t.Fatalf("canonical payload refused: %v", err)
	}
	for _, field := range []string{"tool_name", "args", "reason", "text", "prompt"} {
		request := attentionDeliveryRequest("modelplane.needs_input")
		request.Payload[field] = "run content"
		if result, err := adapter.Dispatch(t.Context(), request); result != nil || err == nil {
			t.Fatalf("payload field %q accepted, want refused", field)
		}
	}
}

func TestCompositeRoutesNativeTypesOnlyToTheNativeAdapter(t *testing.T) {
	provider := &recordingDispatcher{}
	composite := NewComposite(provider, NewNativeAdapter())

	result, err := composite.Dispatch(t.Context(), attentionDeliveryRequest("modelplane.needs_approval"))
	if err != nil {
		t.Fatalf("native Dispatch() error = %v", err)
	}
	if result.Provider != notification.ProviderNative {
		t.Fatalf("native type provider = %q, want %q", result.Provider, notification.ProviderNative)
	}
	if len(provider.types) != 0 {
		t.Fatalf("provider received %v, want no native type", provider.types)
	}

	if _, err := composite.Dispatch(t.Context(), attentionDeliveryRequest("ticket.assigned")); err != nil {
		t.Fatalf("provider Dispatch() error = %v", err)
	}
	if len(provider.types) != 1 || provider.types[0] != "ticket.assigned" {
		t.Fatalf("provider received %v, want ticket.assigned", provider.types)
	}
}
