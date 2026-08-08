package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
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

type fakeDeliveryReceiptRecorder struct {
	mu       sync.Mutex
	receipts []conversation.ProviderDeliveryReceiptInput
	err      error
}

func (f *fakeDeliveryReceiptRecorder) RecordProviderDeliveryReceipt(_ context.Context, receipt conversation.ProviderDeliveryReceiptInput) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return false, f.err
	}
	f.receipts = append(f.receipts, receipt)
	return true, nil
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
			"statuses": [{"id": "wamid.ABC", "status": "delivered", "timestamp": "1700000000", "recipient_id": "4790000001"}]
			}
		}]
	}]
}`

const whatsAppFailedStatusPayload = `{
	"object": "whatsapp_business_account",
	"entry": [{
		"id": "WABA_ID",
		"changes": [{
			"field": "messages",
			"value": {
				"metadata": {"phone_number_id": "PHONE_NUMBER_ID"},
				"statuses": [{
					"id": "wamid.FAILED",
					"status": "failed",
					"timestamp": "1700000001",
					"errors": [{"code": 131026, "title": "Message Undeliverable", "error_data": {"details": "Customer-specific provider detail must not enter the audit code."}}]
				}]
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

func TestWebhookReceived_UnifiedMetaMessenger_StoresWithEventConnection(t *testing.T) {
	fetcher := &fakeWebhookFetcher{
		payloads: map[string]map[string]any{"meta-1": decodePayload(t, messengerMessagePayload)},
	}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := ingestionEvent{
		OrganizationID: "org-meta",
		ConnectionID:   "conn-meta",
		ProviderKey:    "meta",
		Data:           map[string]any{"webhookEventId": "meta-1"},
	}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 1 {
		t.Fatalf("IngestEvent called %d times, want 1", ingester.count())
	}
	event := ingester.events[0]
	if event.Provider != "messenger" || event.ConnectionID != "conn-meta" {
		t.Fatalf("stored unified Meta event = %+v", event)
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

func TestWebhookReceived_WhatsAppDeliveryReceiptUpdatesOnlyTheMatchingOutboundEvidence(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-delivery": decodePayload(t, whatsAppStatusPayload)}}
	ingester := &fakeIngester{}
	recorder := &fakeDeliveryReceiptRecorder{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester, receipts: recorder}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-delivery")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 0 {
		t.Fatalf("delivery callback stored %d inbound events, want 0", ingester.count())
	}
	if len(recorder.receipts) != 1 {
		t.Fatalf("delivery receipts = %#v, want one", recorder.receipts)
	}
	got := recorder.receipts[0]
	if got.OrgID != "org-1" || got.Provider != "whatsapp" || got.ProviderMessageID != "wamid.ABC" || got.Status != conversation.ProviderDeliveryDelivered {
		t.Fatalf("receipt = %#v", got)
	}
	if got.OccurredAt.IsZero() {
		t.Fatal("receipt timestamp is missing")
	}
}

func TestWebhookReceived_WhatsAppFailedReceiptPreservesOnlyTheProviderErrorCode(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-failed": decodePayload(t, whatsAppFailedStatusPayload)}}
	ingester := &fakeIngester{}
	recorder := &fakeDeliveryReceiptRecorder{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester, receipts: recorder}

	if got := c.process(context.Background(), whatsAppWebhookEvent("org-1", "wh-failed")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 0 || len(recorder.receipts) != 1 {
		t.Fatalf("inbound=%d receipts=%#v, want no inbound and one receipt", ingester.count(), recorder.receipts)
	}
	receipt := recorder.receipts[0]
	if receipt.ProviderMessageID != "wamid.FAILED" || receipt.Status != conversation.ProviderDeliveryFailed {
		t.Fatalf("receipt = %#v", receipt)
	}
	if receipt.ErrorCode != "131026" {
		t.Fatalf("error code = %q, want exact provider code", receipt.ErrorCode)
	}
	if strings.Contains(receipt.ErrorCode, "Customer-specific") || strings.Contains(receipt.ErrorCode, "Undeliverable") {
		t.Fatalf("receipt error code leaked provider detail: %q", receipt.ErrorCode)
	}
}

func TestNumericProviderErrorCodeKeepsOnlyBoundedDecimalCodes(t *testing.T) {
	cases := []struct {
		name  string
		value any
		want  string
	}{
		{name: "webhook JSON number", value: float64(131026), want: "131026"},
		{name: "string-backed code", value: "131026", want: "131026"},
		{name: "provider title", value: "Message Undeliverable", want: ""},
		{name: "nested diagnostic", value: map[string]any{"details": "customer material"}, want: ""},
		{name: "decimal", value: float64(131026.5), want: ""},
		{name: "oversized", value: "12345678901234567", want: ""},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if got := numericProviderErrorCode(test.value); got != test.want {
				t.Fatalf("numericProviderErrorCode(%#v) = %q, want %q", test.value, got, test.want)
			}
		})
	}
}

func TestWebhookReceived_UncorrelatableReadWatermarkDoesNotCreateDeliveryEvidence(t *testing.T) {
	fetcher := &fakeWebhookFetcher{payloads: map[string]map[string]any{"wh-read": decodePayload(t, messengerReadReceiptPayload)}}
	ingester := &fakeIngester{}
	recorder := &fakeDeliveryReceiptRecorder{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester, receipts: recorder}

	ev := ingestionEvent{OrganizationID: "org-1", ProviderKey: "facebook", Data: map[string]any{"webhookEventId": "wh-read"}}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(recorder.receipts) != 0 {
		t.Fatalf("read watermark created receipts = %#v", recorder.receipts)
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

	// github is a genuinely unhandled channel here (slack graduated to a
	// supported inbox channel in the Slack-inbound build).
	ev := ingestionEvent{OrganizationID: "org-1", ProviderKey: "github", Data: map[string]any{"webhookEventId": "wh-5"}}
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

// --- object-field channel branching (Instagram vs Messenger) ----------------

func metaMessagingPayload(object, entryID, senderID, text string) map[string]any {
	return map[string]any{
		"object": object,
		"entry": []any{
			map[string]any{
				"id": entryID,
				"messaging": []any{
					map[string]any{
						"sender":  map[string]any{"id": senderID},
						"message": map[string]any{"mid": "m-1", "text": text},
					},
				},
			},
		},
	}
}

func TestNormalize_InstagramObject_LabelsInstagram(t *testing.T) {
	events, err := normalizeMetaWebhookPayload(metaMessagingPayload("instagram", "17841400000000000", "893000000000001", "Er dette på lager?"))
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("want 1 event, got %d", len(events))
	}
	if events[0].Provider != "instagram" {
		t.Fatalf("provider = %q, want instagram", events[0].Provider)
	}
	if events[0].Subject != "Instagram message" {
		t.Fatalf("subject = %q", events[0].Subject)
	}
	if events[0].ProviderThreadID != "17841400000000000:893000000000001" {
		t.Fatalf("thread = %q, want igAccountId:igsid composite", events[0].ProviderThreadID)
	}
}

func TestNormalize_PageObject_StaysMessenger(t *testing.T) {
	events, err := normalizeMetaWebhookPayload(metaMessagingPayload("page", "page-1", "psid-1", "hello"))
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 1 || events[0].Provider != "messenger" {
		t.Fatalf("want 1 messenger event, got %+v", events)
	}
}

func TestNormalizeMetaSkipsMessengerAndInstagramEchoes(t *testing.T) {
	for _, object := range []string{"page", "instagram"} {
		t.Run(object, func(t *testing.T) {
			payload := metaMessagingPayload(object, "account-1", "sender-1", "our reply")
			entry := payload["entry"].([]any)[0].(map[string]any)
			item := entry["messaging"].([]any)[0].(map[string]any)
			item["message"].(map[string]any)["is_echo"] = true
			events, err := normalizeMetaWebhookPayload(payload)
			if err != nil {
				t.Fatalf("normalizeMetaWebhookPayload: %v", err)
			}
			if len(events) != 0 {
				t.Fatalf("events = %+v, want outbound echo skipped", events)
			}
		})
	}
}

func TestNormalize_UnknownObject_FallsBackToShapeSniffing(t *testing.T) {
	payload := metaMessagingPayload("", "page-1", "psid-1", "hello")
	delete(payload, "object")
	events, err := normalizeMetaWebhookPayload(payload)
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 1 || events[0].Provider != "messenger" {
		t.Fatalf("legacy payloads without object must keep working; got %+v", events)
	}
}

func TestNormalize_WhatsAppObject_NeverRunsMessagingExtractors(t *testing.T) {
	// A whatsapp_business_account payload that ALSO carries a messaging[] shape
	// must not produce a messenger/instagram event.
	payload := metaMessagingPayload("whatsapp_business_account", "waba-1", "psid-1", "hello")
	events, err := normalizeMetaWebhookPayload(payload)
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 0 {
		t.Fatalf("want 0 events (no changes[].value.messages), got %+v", events)
	}
}

// ── Slack ────────────────────────────────────────────────────────────────────

const slackMessagePayload = `{
	"type": "event_callback",
	"team_id": "T0EXAMPLE",
	"event_id": "Ev12345678",
	"event_time": 1751968800,
	"event": {
		"type": "message",
		"channel": "C0GENERAL",
		"channel_type": "channel",
		"user": "U0KARI",
		"text": "Hei, trenger hjelp med faktura",
		"ts": "1751968800.000100"
	}
}`

const slackThreadedPayload = `{
	"type": "event_callback",
	"team_id": "T0EXAMPLE",
	"event_id": "Ev87654321",
	"event": {
		"type": "message",
		"channel": "C0GENERAL",
		"user": "U0KARI",
		"text": "Svar i tråden",
		"ts": "1751968900.000200",
		"thread_ts": "1751968800.000100"
	}
}`

const slackBotEchoPayload = `{
	"type": "event_callback",
	"team_id": "T0EXAMPLE",
	"event_id": "Ev00000001",
	"event": {
		"type": "message",
		"channel": "C0GENERAL",
		"user": "U0BOT",
		"bot_id": "B0OURBOT",
		"text": "Automated reply",
		"ts": "1751969000.000300"
	}
}`

const slackEditedPayload = `{
	"type": "event_callback",
	"team_id": "T0EXAMPLE",
	"event_id": "Ev00000002",
	"event": {
		"type": "message",
		"subtype": "message_changed",
		"channel": "C0GENERAL",
		"user": "U0KARI",
		"text": "edited text",
		"ts": "1751969100.000400"
	}
}`

func TestNormalizeSlack_TopLevelMessage(t *testing.T) {
	events, err := normalizeSlackWebhookPayload(decodePayload(t, slackMessagePayload))
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("events = %d, want 1", len(events))
	}
	event := events[0]
	if event.Provider != "slack" || event.ProviderEventID != "Ev12345678" || event.ProviderMessageID != "1751968800.000100" {
		t.Errorf("identity fields: %+v", event)
	}
	if event.ProviderThreadID != "C0GENERAL" {
		t.Errorf("thread ref = %q, want bare channel for top-level messages", event.ProviderThreadID)
	}
	if event.From.Name != "U0KARI" || event.BodyText != "Hei, trenger hjelp med faktura" {
		t.Errorf("content fields: %+v", event)
	}
	if event.OccurredAt.IsZero() {
		t.Error("occurredAt should parse from ts")
	}
}

func TestNormalizeSlack_ThreadedMessageCompositeRef(t *testing.T) {
	events, err := normalizeSlackWebhookPayload(decodePayload(t, slackThreadedPayload))
	if err != nil {
		t.Fatalf("normalize: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("events = %d, want 1", len(events))
	}
	if events[0].ProviderThreadID != "C0GENERAL:1751968800.000100" {
		t.Errorf("thread ref = %q, want channel:thread_ts composite", events[0].ProviderThreadID)
	}
}

func TestNormalizeSlack_SkipsBotEchoesAndSubtypes(t *testing.T) {
	for name, payload := range map[string]string{
		"bot echo":  slackBotEchoPayload,
		"edited":    slackEditedPayload,
		"handshake": `{"type": "url_verification", "challenge": "abc"}`,
	} {
		events, err := normalizeSlackWebhookPayload(decodePayload(t, payload))
		if err != nil {
			t.Fatalf("%s: normalize: %v", name, err)
		}
		if len(events) != 0 {
			t.Errorf("%s: events = %d, want 0", name, len(events))
		}
	}
}

func TestWebhookReceived_SlackMessage_StoresInboundEvent(t *testing.T) {
	fetcher := &fakeWebhookFetcher{
		payloads:     map[string]map[string]any{"wh-slack": decodePayload(t, slackMessagePayload)},
		connectionID: "conn-slack-1",
	}
	ingester := &fakeIngester{}
	c := &WebhookReceivedConsumer{fetcher: fetcher, ingester: ingester}

	ev := ingestionEvent{
		OrganizationID: "org-1",
		ProviderKey:    "slack",
		Data:           map[string]any{"webhookEventId": "wh-slack"},
	}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if ingester.count() != 1 {
		t.Fatalf("IngestEvent called %d times, want 1", ingester.count())
	}
	event := ingester.events[0]
	if event.OrgID != "org-1" || event.Provider != "slack" {
		t.Errorf("org/provider mismatch: %+v", event)
	}
	if event.ConnectionID != "conn-slack-1" {
		t.Errorf("connection_id = %q, want resolved slack connection", event.ConnectionID)
	}
	if len(fetcher.connectionKeys) != 1 || fetcher.connectionKeys[0][0] != "slack" {
		t.Errorf("expected slack connection candidates, got %+v", fetcher.connectionKeys)
	}
}
