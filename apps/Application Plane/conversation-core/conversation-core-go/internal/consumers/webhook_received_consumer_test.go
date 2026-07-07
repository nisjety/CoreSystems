package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

// fakeWebhookFetcher serves a canned payload/connection lookup and records
// what it was asked for, so tests can assert the consumer only fetches when
// it should and forwards the resolved connection id correctly.
type fakeWebhookFetcher struct {
	mu             sync.Mutex
	payloads       map[string]map[string]any
	fetchErr       error
	connectionID   string
	connectionErr  error
	fetchCalls     int
	connectionKeys [][]string
}

func (f *fakeWebhookFetcher) FetchWebhookEvent(_ context.Context, _, webhookEventID string) (*integration.WebhookEvent, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.fetchCalls++
	if f.fetchErr != nil {
		return nil, f.fetchErr
	}
	payload, ok := f.payloads[webhookEventID]
	if !ok {
		return nil, fmt.Errorf("no fixture payload for %s", webhookEventID)
	}
	return &integration.WebhookEvent{ID: webhookEventID, Payload: payload}, nil
}

func (f *fakeWebhookFetcher) FetchActiveConnectionID(_ context.Context, _ string, providerKeys ...string) (string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.connectionKeys = append(f.connectionKeys, providerKeys)
	if f.connectionErr != nil {
		return "", f.connectionErr
	}
	return f.connectionID, nil
}

// fakeIngester records IngestEvent calls, tolerating an injected error so the
// consumer's ack-vs-retry policy can be asserted.
type fakeIngester struct {
	mu      sync.Mutex
	events  []conversation.InboundEvent
	err     error
	invalid bool
}

func (f *fakeIngester) IngestEvent(_ context.Context, event conversation.InboundEvent) (*conversation.StoredEventResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	f.events = append(f.events, event)
	return &conversation.StoredEventResult{Created: true}, nil
}

func (f *fakeIngester) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.events)
}

func whatsAppWebhookEvent(orgID, webhookEventID string) ingestionEvent {
	return ingestionEvent{
		Type:           "integration.webhook_received",
		OrganizationID: orgID,
		ProviderKey:    "whatsapp",
		Data:           map[string]any{"eventType": "provider.webhook", "webhookEventId": webhookEventID},
	}
}

const whatsAppMessagePayload = `{
	"object": "whatsapp_business_account",
	"entry": [{
		"id": "WABA_ID",
		"changes": [{
			"field": "messages",
			"value": {
				"messaging_product": "whatsapp",
				"metadata": {"display_phone_number": "16505551111", "phone_number_id": "PHONE_NUMBER_ID"},
				"contacts": [{"profile": {"name": "Kari Nordmann"}, "wa_id": "4790000001"}],
				"messages": [{"from": "4790000001", "id": "wamid.ABC", "timestamp": "1700000000", "type": "text", "text": {"body": "Hei, trenger hjelp"}}]
			}
		}]
	}]
}`

const whatsAppStatusPayload = `{
	"object": "whatsapp_business_account",
	"entry": [{
		"id": "WABA_ID",
		"changes": [{
			"field": "messages",
			"value": {
				"metadata": {"phone_number_id": "PHONE_NUMBER_ID"},
				"statuses": [{"id": "wamid.ABC", "status": "delivered", "recipient_id": "4790000001"}]
			}
		}]
	}]
}`

const messengerMessagePayload = `{
	"object": "page",
	"entry": [{
		"id": "PAGE_ID",
		"time": 1700000000,
		"messaging": [{
			"sender": {"id": "PSID_1"},
			"recipient": {"id": "PAGE_ID"},
			"timestamp": 1700000000,
			"message": {"mid": "mid.1", "text": "Hei der!"}
		}]
	}]
}`

const messengerReadReceiptPayload = `{
	"object": "page",
	"entry": [{
		"id": "PAGE_ID",
		"messaging": [{
			"sender": {"id": "PSID_1"},
			"recipient": {"id": "PAGE_ID"},
			"read": {"watermark": 1700000000}
		}]
	}]
}`

func decodePayload(t *testing.T, raw string) map[string]any {
	t.Helper()
	var payload map[string]any
	if err := json.Unmarshal([]byte(raw), &payload); err != nil {
		t.Fatalf("decode fixture: %v", err)
	}
	return payload
}

func TestWebhookReceived_WhatsAppMessage_StoresInboundEvent(t *testing.T) {
	fetcher := &fakeWebhookFetcher{
		payloads:     map[string]map[string]any{"wh-1": decodePayload(t, whatsAppMessagePayload)},
		connectionID: "conn-whatsapp-1",
	}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 1 {
		t.Fatalf("IngestEvent called %d times, want 1", ingester.count())
	}
	event := ingester.events[0]
	if event.OrgID != "org-1" || event.Provider != "whatsapp" {
		t.Errorf("org/provider mismatch: %+v", event)
	}
	if event.From.Name != "Kari Nordmann" || event.From.Phone != "4790000001" {
		t.Errorf("sender mismatch: %+v", event.From)
	}
	if event.BodyText != "Hei, trenger hjelp" {
		t.Errorf("body mismatch: %q", event.BodyText)
	}
	if event.ProviderThreadID != "PHONE_NUMBER_ID:4790000001" {
		t.Errorf("provider_thread_id = %q, want composite phoneNumberId:waId", event.ProviderThreadID)
	}
	if event.ConnectionID != "conn-whatsapp-1" {
		t.Errorf("connection_id = %q, want resolved connection", event.ConnectionID)
	}
	if len(fetcher.connectionKeys) != 1 || fetcher.connectionKeys[0][0] != "whatsapp" {
		t.Errorf("expected connection resolution to try whatsapp first, got %+v", fetcher.connectionKeys)
	}
}

func TestWebhookReceived_MessengerMessage_StoresInboundEvent(t *testing.T) {
	fetcher := &fakeWebhookFetcher{
		payloads:     map[string]map[string]any{"wh-2": decodePayload(t, messengerMessagePayload)},
		connectionID: "conn-facebook-1",
	}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := ingestionEvent{
		OrganizationID: "org-1",
		ProviderKey:    "facebook",
		Data:           map[string]any{"webhookEventId": "wh-2"},
	}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 1 {
		t.Fatalf("IngestEvent called %d times, want 1", ingester.count())
	}
	event := ingester.events[0]
	if event.Provider != "messenger" {
		t.Errorf("provider = %q, want messenger", event.Provider)
	}
	if event.ProviderThreadID != "PAGE_ID:PSID_1" {
		t.Errorf("provider_thread_id = %q, want composite pageId:psid", event.ProviderThreadID)
	}
	if event.BodyText != "Hei der!" {
		t.Errorf("body mismatch: %q", event.BodyText)
	}
	if len(fetcher.connectionKeys) != 1 || fetcher.connectionKeys[0][0] != "facebook" {
		t.Errorf("expected connection resolution to try facebook first, got %+v", fetcher.connectionKeys)
	}
}

func TestWebhookReceived_StatusOnlyPayload_AcksWithoutStoring(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-3": decodePayload(t, whatsAppStatusPayload)}}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-3")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 0 {
		t.Errorf("a delivery-status-only webhook stored %d events, want 0", ingester.count())
	}
}

func TestWebhookReceived_ReadReceiptOnlyPayload_AcksWithoutStoring(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-4": decodePayload(t, messengerReadReceiptPayload)}}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := ingestionEvent{OrganizationID: "org-1", ProviderKey: "facebook", Data: map[string]any{"webhookEventId": "wh-4"}}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 0 {
		t.Errorf("a read-receipt-only webhook stored %d events, want 0", ingester.count())
	}
}

func TestWebhookReceived_UnsupportedProvider_AcksWithoutFetching(t *testing.T) {
	fetcher := &fakeWebhookFetcher{}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := ingestionEvent{OrganizationID: "org-1", ProviderKey: "slack", Data: map[string]any{"webhookEventId": "wh-5"}}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if fetcher.fetchCalls != 0 {
		t.Errorf("fetched payload for an unsupported provider (fetchCalls=%d)", fetcher.fetchCalls)
	}
}

func TestWebhookReceived_MalformedEvent_AcksWithoutFetching(t *testing.T) {
	fetcher := &fakeWebhookFetcher{}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	cases := []ingestionEvent{
		{ProviderKey: "whatsapp", Data: map[string]any{"webhookEventId": "wh-6"}},  // missing org
		{OrganizationID: "org-1", ProviderKey: "whatsapp", Data: map[string]any{}}, // missing webhookEventId
	}
	for i, ev := range cases {
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("case %d outcome = %v, want outcomeAck", i, got)
		}
	}
	if fetcher.fetchCalls != 0 {
		t.Errorf("fetched payload for a malformed event (fetchCalls=%d)", fetcher.fetchCalls)
	}
}

func TestWebhookReceived_FetchError_Retries(t *testing.T) {
	fetcher := &fakeWebhookFetcher{fetchErr: fmt.Errorf("connection refused")}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-7")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry on a transient fetch failure", got)
	}
}

func TestWebhookReceived_StoreError_Retries(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-8": decodePayload(t, whatsAppMessagePayload)}}
	ingester := &fakeIngester{err: fmt.Errorf("db unavailable")}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-8")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry on a transient store failure", got)
	}
}

func TestWebhookReceived_InvalidInputOnStore_AcksWithoutRetry(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-9": decodePayload(t, whatsAppMessagePayload)}}
	ingester := &fakeIngester{err: fmt.Errorf("%w: sender is required", conversation.ErrInvalidInput)}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-9")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck for a terminal validation error", got)
	}
}

func TestWebhookReceived_EventConnectionIDTakesPrecedenceOverLookup(t *testing.T) {
	fetcher := &fakeWebhookFetcher{
		payloads:     map[string]map[string]any{"wh-10": decodePayload(t, whatsAppMessagePayload)},
		connectionID: "conn-should-not-be-used",
	}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := whatsAppWebhookEvent("org-1", "wh-10")
	ev.ConnectionID = "conn-from-event"
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.events[0].ConnectionID != "conn-from-event" {
		t.Errorf("connection_id = %q, want the event's own connectionId to take precedence", ingester.events[0].ConnectionID)
	}
	if len(fetcher.connectionKeys) != 0 {
		t.Errorf("looked up a connection despite the event already carrying one: %+v", fetcher.connectionKeys)
	}
}
