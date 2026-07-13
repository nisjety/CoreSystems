// Package runtime holds the Novu delivery adapter.
//
// NovuAdapter wires the official novu-go/v3 SDK.
//
// Disabled mode is explicit and fail-closed: Dispatch returns
// ErrDeliveryDisabled and never fabricates a provider transaction.
//
// Production mode (NOVU_SECRET_KEY set):
//   - Dispatch calls s.Novu.Trigger using req.Type as the Novu workflow
//     identifier and req.RequestID as the idempotency key.
//   - req.RecipientID must match an existing Novu subscriber ID (or be upserted
//     before the trigger — see Novu docs on subscriber identification).
//   - req.Payload is forwarded verbatim plus two correlation fields:
//     _velion_request_id  — our internal request ID
//     _velion_source      — source service (when non-empty)
//
// EU region: set NOVU_BASE_URL=https://eu.api.novu.co
package runtime

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	v3 "github.com/novuhq/novu-go/v3"
	"github.com/novuhq/novu-go/v3/models/components"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
)

// Config carries the Novu SDK credentials.
// Both fields are optional: leave SecretKey empty for local stub mode.
type Config struct {
	Mode      string // NOTIFICATION_DELIVERY_MODE: "novu" or "disabled"
	SecretKey string // NOVU_SECRET_KEY — required for real delivery
	BaseURL   string // NOVU_BASE_URL   — optional (EU: https://eu.api.novu.co)
}

const (
	DeliveryModeNovu     = "novu"
	DeliveryModeDisabled = "disabled"
)

var ErrDeliveryDisabled = errors.New("external notification delivery is disabled")

// NovuAdapter is the production delivery adapter backed by the Novu v3 API.
// It satisfies notification.RuntimeClient.
type NovuAdapter struct {
	client *v3.Novu // nil in stub mode
	mode   string
}

// NewNovuAdapter constructs the adapter.
// When cfg.SecretKey is empty the adapter starts in stub mode — a one-time
// warning is logged and Dispatch returns synthetic IDs without calling Novu.
func NewNovuAdapter(cfg Config) (*NovuAdapter, error) {
	mode := strings.ToLower(strings.TrimSpace(cfg.Mode))
	if mode == "" {
		mode = DeliveryModeDisabled
	}
	if mode == DeliveryModeDisabled {
		return &NovuAdapter{mode: mode}, nil
	}
	if mode != DeliveryModeNovu {
		return nil, fmt.Errorf("unsupported notification delivery mode %q", mode)
	}
	if strings.TrimSpace(cfg.SecretKey) == "" {
		return nil, errors.New("NOVU_SECRET_KEY is required in novu delivery mode")
	}

	httpClient := &http.Client{Timeout: 15 * time.Second}
	baseURL := strings.TrimSpace(cfg.BaseURL)

	var client *v3.Novu
	if baseURL != "" {
		client = v3.New(
			v3.WithSecurity(cfg.SecretKey),
			v3.WithClient(httpClient),
			v3.WithServerURL(baseURL),
		)
	} else {
		client = v3.New(
			v3.WithSecurity(cfg.SecretKey),
			v3.WithClient(httpClient),
		)
	}

	return &NovuAdapter{client: client, mode: mode}, nil
}

// Dispatch triggers a Novu workflow.
//
// Field mapping:
//
//	req.Type        → Novu WorkflowID  (must match a workflow in your dashboard)
//	req.ProviderRecipientID → Novu SubscriberID resolved from an active org membership
//	req.RequestID   → Novu idempotency key  (Novu deduplicates on this)
//	req.Payload     → Novu trigger payload  (+ _velion_* correlation fields)
func (a *NovuAdapter) Dispatch(ctx context.Context, req notification.DeliveryRequest) (*notification.DispatchResult, error) {
	if a == nil || a.mode == DeliveryModeDisabled {
		return nil, ErrDeliveryDisabled
	}
	if a.client == nil {
		return nil, errors.New("novu client is not configured")
	}

	// Build payload — copy first so we never mutate the caller's map.
	payload := make(map[string]any, len(req.Payload)+3)
	for k, v := range req.Payload {
		payload[k] = v
	}
	payload["_velion_request_id"] = req.RequestID
	payload["_velion_organization_id"] = req.OrganizationID
	if req.Source != "" {
		payload["_velion_source"] = req.Source
	}

	// req.RequestID doubles as the Novu idempotency key so that retries
	// with the same request ID are deduplicated on the Novu side.
	idempotencyKey := req.RequestID

	res, err := a.client.Trigger(ctx, components.TriggerEventRequestDto{
		WorkflowID: req.Type,
		To:         components.CreateToStr(req.ProviderRecipientID),
		Payload:    payload,
	}, &idempotencyKey)
	if err != nil {
		return nil, fmt.Errorf("novu trigger %q: %w", req.Type, err)
	}
	if res.TriggerEventResponseDto == nil {
		return nil, errors.New("novu trigger: empty response body")
	}

	transactionID := ""
	if res.TriggerEventResponseDto.TransactionID != nil {
		transactionID = *res.TriggerEventResponseDto.TransactionID
	}
	if transactionID == "" {
		return nil, errors.New("novu trigger: response missing transaction id")
	}

	return &notification.DispatchResult{
		Provider:          notification.ProviderNovu,
		ProviderRequestID: transactionID,
	}, nil
}

// ===========================================================================
// U5-2 — subscriber + preference + inbox extensions.
//
// These extend NovuAdapter beyond the original Dispatch-only surface to
// cover the full /profile/notifications surface velion needs. Each method
// is a thin wrapper over a typed novu-go/v3 SDK call. In stub mode (client
// is nil, no NOVU_SECRET_KEY) every method is a no-op — local development
// still works without a Novu account.
// ===========================================================================

// IdentifySubscriber upserts a subscriber on Novu. Idempotent — Novu's
// Patch endpoint creates the subscriber on first call and updates on
// subsequent calls (we'd use Create directly but it returns 400 on
// "subscriber already exists", which complicates the happy path).
//
// Signature matches subscribers.IdentifyClient — the runtime adapter
// satisfies the interface declared in the subscribers package. We import
// subscribers (rather than defining a parallel type here) so the Novu
// payload mapping lives next to the SDK call.
func (a *NovuAdapter) IdentifySubscriber(ctx context.Context, p subscribers.IdentifyParams) error {
	if a == nil || a.client == nil {
		return ErrDeliveryDisabled
	}
	if strings.TrimSpace(p.SubscriberID) == "" {
		return errors.New("IdentifySubscriber: subscriber_id required")
	}

	// First try Create; on conflict fall through to Patch.
	failIfExists := false
	dto := components.CreateSubscriberRequestDto{
		SubscriberID: p.SubscriberID,
		Email:        nonEmptyPtr(p.Email),
		Phone:        nonEmptyPtr(p.Phone),
		FirstName:    nonEmptyPtr(p.FirstName),
		LastName:     nonEmptyPtr(p.LastName),
		Avatar:       nonEmptyPtr(p.Avatar),
		Locale:       nonEmptyPtr(p.Locale),
		Timezone:     nonEmptyPtr(p.Timezone),
		Data:         p.Data,
	}

	if _, err := a.client.Subscribers.Create(ctx, dto, &failIfExists, nil); err != nil {
		return fmt.Errorf("novu subscriber create %q: %w", p.SubscriberID, err)
	}
	return nil
}

// UpdateSubscriberPreference flips a single (workflow, channel) toggle.
// `workflowID` is the same value used in Dispatch.req.Type. `channel`
// must match Novu's vocabulary: `in_app`, `email`, `sms`, `push`, `chat`.
func (a *NovuAdapter) UpdateSubscriberPreference(ctx context.Context, subscriberID, workflowID, channel string, enabled bool) error {
	if a == nil || a.client == nil {
		return ErrDeliveryDisabled
	}
	if strings.TrimSpace(subscriberID) == "" || strings.TrimSpace(workflowID) == "" || strings.TrimSpace(channel) == "" {
		return errors.New("UpdateSubscriberPreference: subscriber_id, workflow_id, channel required")
	}

	channels := &components.PatchPreferenceChannelsDto{}
	switch strings.ToLower(channel) {
	case "in_app":
		channels.InApp = &enabled
	case "email":
		channels.Email = &enabled
	case "sms":
		channels.Sms = &enabled
	case "push":
		channels.Push = &enabled
	case "chat":
		channels.Chat = &enabled
	default:
		return fmt.Errorf("UpdateSubscriberPreference: unknown channel %q", channel)
	}

	dto := components.PatchSubscriberPreferencesDto{
		WorkflowID: &workflowID,
		Channels:   channels,
	}

	if _, err := a.client.Subscribers.Preferences.Update(ctx, subscriberID, dto, nil); err != nil {
		return fmt.Errorf("novu preference update %s/%s/%s: %w", subscriberID, workflowID, channel, err)
	}
	return nil
}

func nonEmptyPtr(s string) *string {
	s = strings.TrimSpace(s)
	if s == "" {
		return nil
	}
	return &s
}
