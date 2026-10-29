package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strconv"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

const webhookReceivedDurable = "conversation-core-webhook-received"

// webhookReceivedSubject is the Ingestion Plane subject integration-corev2
// publishes on POST /api/v1/webhooks/:provider. It lives in the ingestion
// namespace (not application/model), so it needs its own JetStream stream —
// see eventing.EnsureIngestionStream.
const webhookReceivedSubject = "verevon.ingestion.integration.webhook_received"

// ingestionEvent decodes integration-corev2's events.Event envelope. Its JSON
// tags are camelCase (organizationId, providerKey, ...) — deliberately NOT
// conversation.LifecycleEvent, whose tags are snake_case (org_id, ...) for a
// different, Application-Plane-internal event shape. Unmarshaling this
// message into LifecycleEvent would silently leave OrgID/ProviderKey empty.
type ingestionEvent struct {
	Type           string         `json:"type"`
	OrganizationID string         `json:"organizationId"`
	ConnectionID   string         `json:"connectionId,omitempty"`
	ProviderKey    string         `json:"providerKey,omitempty"`
	Data           map[string]any `json:"data,omitempty"`
}

// WebhookFetcher is the narrow integration-corev2 surface the consumer needs:
// fetch the full stored webhook payload, and best-effort resolve a connection
// to reply through. *integration.Client satisfies it.
type WebhookFetcher interface {
	FetchWebhookEvent(ctx context.Context, orgID, webhookEventID string) (*integration.WebhookEvent, error)
	FetchActiveConnectionID(ctx context.Context, orgID string, providerKeys ...string) (string, error)
}

// EventIngester is the narrow conversation-core surface the consumer needs:
// validate, dedup, and store one normalized inbound event. *conversation.Service
// satisfies it.
type EventIngester interface {
	IngestEvent(ctx context.Context, event conversation.InboundEvent) (*conversation.StoredEventResult, error)
}

// DeliveryReceiptRecorder persists a provider callback only after matching its
// exact provider message identifier in the organization-scoped outbound ledger.
// *conversation.Service satisfies it.
type DeliveryReceiptRecorder interface {
	RecordProviderDeliveryReceipt(ctx context.Context, input conversation.ProviderDeliveryReceiptInput) (bool, error)
}

// WebhookReceivedConsumer bridges the Ingestion Plane → Application Plane: when
// integration-corev2 publishes webhook_received for a Meta WhatsApp/Messenger
// event, this fetches the full payload, normalizes inbound content into
// InboundEvents, and records only delivery callbacks that name an exact,
// organization-scoped outbound provider message. It closes the inbound leg of
// the provider messaging loop (task #24) without guessing delivery from a
// generic read watermark or a provider acknowledgement.
type WebhookReceivedConsumer struct {
	consumer *DurableConsumer
	fetcher  WebhookFetcher
	ingester EventIngester
	receipts DeliveryReceiptRecorder
}

func NewWebhookReceivedConsumer(js nats.JetStreamContext, fetcher WebhookFetcher, ingester EventIngester, receipts DeliveryReceiptRecorder) *WebhookReceivedConsumer {
	return &WebhookReceivedConsumer{
		consumer: NewDurableConsumer(js, "webhook-received"),
		fetcher:  fetcher,
		ingester: ingester,
		receipts: receipts,
	}
}

// Start binds the durable consumer on the webhook received subject. The
// ingestion stream must exist first (see eventing.EnsureIngestionStream).
func (c *WebhookReceivedConsumer) Start(_ context.Context) error {
	return c.consumer.BindProvisioned(webhookReceivedSubject, applicationIngestionStream, webhookReceivedDurable, c.handle)
}

// Stop drains the subscription.
func (c *WebhookReceivedConsumer) Stop() { c.consumer.Stop() }

func (c *WebhookReceivedConsumer) handle(msg *nats.Msg) {
	var ev ingestionEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		// Poison message — ack to avoid an infinite redelivery loop.
		log.Printf("[cc-go/webhook-received] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	switch c.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/webhook-received] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[cc-go/webhook-received] ack: %v", err)
		}
	}
}

// process decodes one webhook_received event, fetches its full payload, and —
// if it carries inbound content — normalizes and stores it; if it carries an
// exact provider delivery receipt, records that evidence independently. Every
// other case (missing ids, unsupported provider, uncorrelatable callback, or
// invalid content) acks as a terminal no-op; only a transient fetch/store error
// retries. It is the testable core (no NATS required).
func (c *WebhookReceivedConsumer) process(ctx context.Context, ev ingestionEvent) outcome {
	orgID := strings.TrimSpace(ev.OrganizationID)
	webhookEventID := stringFromData(ev.Data, "webhookEventId")
	if orgID == "" || webhookEventID == "" {
		log.Printf("[cc-go/webhook-received] malformed event (missing organizationId/webhookEventId); skipping")
		return outcomeAck
	}

	providerKey := strings.ToLower(strings.TrimSpace(ev.ProviderKey))
	switch providerKey {
	case "whatsapp", "facebook", "meta", "instagram", "slack":
	default:
		// Not a channel this consumer handles (e.g. github, shopify).
		return outcomeAck
	}

	stored, err := c.fetcher.FetchWebhookEvent(ctx, orgID, webhookEventID)
	if err != nil {
		log.Printf("[cc-go/webhook-received] fetch payload (org=%s id=%s): %v", orgID, webhookEventID, err)
		return outcomeRetry
	}
	if stored == nil {
		log.Printf("[cc-go/webhook-received] no payload returned (org=%s id=%s); skipping", orgID, webhookEventID)
		return outcomeAck
	}

	var events []conversation.InboundEvent
	var receipts []conversation.ProviderDeliveryReceiptInput
	if providerKey == "slack" {
		events, err = normalizeSlackWebhookPayload(stored.Payload)
	} else {
		events, err = normalizeMetaWebhookPayload(stored.Payload)
		receipts = normalizeMetaDeliveryReceipts(stored.Payload)
	}
	if err != nil {
		log.Printf("[cc-go/webhook-received] normalize payload (org=%s id=%s): %v", orgID, webhookEventID, err)
		return outcomeAck
	}
	if len(events) == 0 && len(receipts) == 0 {
		// A real callback with neither inbound content nor a correlatable receipt
		// (for example a verification ping, bot echo, or generic read watermark).
		return outcomeAck
	}

	if c.receipts != nil {
		for i := range receipts {
			receipt := receipts[i]
			receipt.OrgID = orgID
			if _, receiptErr := c.receipts.RecordProviderDeliveryReceipt(ctx, receipt); receiptErr != nil {
				if conversation.IsInvalidInput(receiptErr) {
					log.Printf("[cc-go/webhook-received] invalid delivery receipt (org=%s provider=%s): %v", orgID, receipt.Provider, receiptErr)
					continue
				}
				log.Printf("[cc-go/webhook-received] store delivery receipt (org=%s provider=%s): %v", orgID, receipt.Provider, receiptErr)
				return outcomeRetry
			}
		}
	}

	anyStoreFailure := false
	for i := range events {
		event := events[i]
		event.OrgID = orgID
		event.ConnectionID = c.resolveConnectionID(ctx, orgID, event.Provider, ev.ConnectionID)
		if _, err := c.ingester.IngestEvent(ctx, event); err != nil {
			if conversation.IsInvalidInput(err) {
				log.Printf("[cc-go/webhook-received] invalid normalized event (org=%s provider=%s): %v", orgID, event.Provider, err)
				continue
			}
			log.Printf("[cc-go/webhook-received] store inbound event (org=%s provider=%s): %v", orgID, event.Provider, err)
			anyStoreFailure = true
		}
	}
	if anyStoreFailure {
		return outcomeRetry
	}
	return outcomeAck
}

// resolveConnectionID best-effort resolves a connection to reply through.
// Meta webhooks are account-wide, not connection-scoped, so the event itself
// rarely carries one; this tries the event's own connectionId first (some
// integration-corev2 publish sites do set it), then the channel-appropriate
// provider keys. A failed lookup is logged and treated as "no connection" —
// the inbound message is still worth storing even if a reply target can't be
// resolved yet.
func (c *WebhookReceivedConsumer) resolveConnectionID(ctx context.Context, orgID, channel, eventConnectionID string) string {
	if strings.TrimSpace(eventConnectionID) != "" {
		return eventConnectionID
	}
	var candidates []string
	switch channel {
	case "whatsapp":
		candidates = []string{"whatsapp", "meta"}
	case "messenger":
		candidates = []string{"facebook", "meta"}
	case "instagram":
		// IG DMs ride the Messenger Platform via the linked Page, so any of
		// the Meta-family connections can carry the reply.
		candidates = []string{"instagram", "meta", "facebook"}
	case "slack":
		candidates = []string{"slack"}
	default:
		return ""
	}
	connectionID, err := c.fetcher.FetchActiveConnectionID(ctx, orgID, candidates...)
	if err != nil {
		log.Printf("[cc-go/webhook-received] resolve connection (org=%s channel=%s): %v", orgID, channel, err)
		return ""
	}
	return connectionID
}

// normalizeMetaWebhookPayload extracts WhatsApp Cloud API, Messenger, and
// Instagram Messaging inbound message events from a raw Meta webhook body (the
// "object" + "entry" envelope common to all Meta webhook callbacks). The
// top-level "object" field is authoritative for the channel: Instagram DMs use
// the same entry[].messaging[] shape as Messenger, so shape-sniffing alone
// mislabels them (confirmed 2026-07-07). Payloads without a recognized object
// fall back to shape-sniffing for backward compatibility with stored events.
// Non-message entries (statuses, delivery, read, postback) are skipped here,
// not errored. Correlatable delivery evidence is extracted separately by
// normalizeMetaDeliveryReceipts; generic read watermarks remain unrecorded.
func normalizeMetaWebhookPayload(payload map[string]any) ([]conversation.InboundEvent, error) {
	if payload == nil {
		return nil, nil
	}
	object := strings.ToLower(strings.TrimSpace(stringFromMap(payload, "object")))
	entries, _ := payload["entry"].([]any)
	var events []conversation.InboundEvent
	for _, rawEntry := range entries {
		entry, ok := rawEntry.(map[string]any)
		if !ok {
			continue
		}
		switch object {
		case "whatsapp_business_account":
			events = append(events, normalizeWhatsAppEntry(entry)...)
		case "page":
			events = append(events, normalizeMessengerEntry(entry)...)
		case "instagram":
			events = append(events, normalizeInstagramEntry(entry)...)
		default:
			events = append(events, normalizeWhatsAppEntry(entry)...)
			events = append(events, normalizeMessengerEntry(entry)...)
		}
	}
	return events, nil
}

// normalizeMetaDeliveryReceipts extracts only callbacks that name a concrete
// outbound provider message. Messenger and Instagram read watermarks do not
// identify a message, so they are intentionally excluded rather than guessed.
func normalizeMetaDeliveryReceipts(payload map[string]any) []conversation.ProviderDeliveryReceiptInput {
	if payload == nil {
		return nil
	}
	object := strings.ToLower(strings.TrimSpace(stringFromMap(payload, "object")))
	entries, _ := payload["entry"].([]any)
	var receipts []conversation.ProviderDeliveryReceiptInput
	for _, rawEntry := range entries {
		entry, ok := rawEntry.(map[string]any)
		if !ok {
			continue
		}
		switch object {
		case "whatsapp_business_account":
			receipts = append(receipts, normalizeWhatsAppDeliveryReceipts(entry)...)
		case "page":
			receipts = append(receipts, normalizeMessengerDeliveryReceipts(entry, "messenger")...)
		case "instagram":
			receipts = append(receipts, normalizeMessengerDeliveryReceipts(entry, "instagram")...)
		}
	}
	return receipts
}

func normalizeWhatsAppDeliveryReceipts(entry map[string]any) []conversation.ProviderDeliveryReceiptInput {
	var receipts []conversation.ProviderDeliveryReceiptInput
	changes, _ := entry["changes"].([]any)
	for _, rawChange := range changes {
		change, ok := rawChange.(map[string]any)
		if !ok {
			continue
		}
		value, _ := change["value"].(map[string]any)
		statuses, _ := value["statuses"].([]any)
		for _, rawStatus := range statuses {
			status, ok := rawStatus.(map[string]any)
			if !ok {
				continue
			}
			messageID := stringFromMap(status, "id")
			occurredAt := providerReceiptTimestamp(status["timestamp"])
			normalizedStatus := normalizeWhatsAppDeliveryStatus(stringFromMap(status, "status"))
			if messageID == "" || occurredAt.IsZero() || normalizedStatus == "" {
				continue
			}
			errorCode := ""
			if normalizedStatus == conversation.ProviderDeliveryFailed {
				// Meta includes errors[].code only for a failed status. Preserve
				// that bounded numeric machine code for operator diagnosis, but
				// never persist the provider title/message/error_data details:
				// those may contain customer-specific material and are not needed
				// to prove the delivery state.
				errorCode = whatsAppFailureCode(status)
			}
			receipts = append(receipts, conversation.ProviderDeliveryReceiptInput{
				Provider:          "whatsapp",
				ProviderMessageID: messageID,
				Status:            normalizedStatus,
				OccurredAt:        occurredAt,
				ErrorCode:         errorCode,
			})
		}
	}
	return receipts
}

func whatsAppFailureCode(status map[string]any) string {
	errors, _ := status["errors"].([]any)
	for _, rawError := range errors {
		providerError, ok := rawError.(map[string]any)
		if !ok {
			continue
		}
		if code := numericProviderErrorCode(providerError["code"]); code != "" {
			return code
		}
	}
	return "provider_reported_failure"
}

// numericProviderErrorCode admits only a short decimal provider code. It
// deliberately rejects provider prose and nested diagnostic objects so receipt
// audit metadata stays content-free.
func numericProviderErrorCode(value any) string {
	var code string
	switch typed := value.(type) {
	case string:
		code = strings.TrimSpace(typed)
	case float64:
		if typed != float64(int64(typed)) {
			return ""
		}
		code = strconv.FormatInt(int64(typed), 10)
	case json.Number:
		code = typed.String()
	default:
		return ""
	}
	if len(code) == 0 || len(code) > 16 {
		return ""
	}
	for _, runeValue := range code {
		if runeValue < '0' || runeValue > '9' {
			return ""
		}
	}
	return code
}

func normalizeWhatsAppDeliveryStatus(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "delivered":
		return conversation.ProviderDeliveryDelivered
	case "read":
		return conversation.ProviderDeliveryRead
	case "failed":
		return conversation.ProviderDeliveryFailed
	default:
		// "sent" is the provider's acknowledgement, already represented by
		// the submitted outbound intent; it is not delivery evidence.
		return ""
	}
}

func normalizeMessengerDeliveryReceipts(entry map[string]any, provider string) []conversation.ProviderDeliveryReceiptInput {
	var receipts []conversation.ProviderDeliveryReceiptInput
	messaging, _ := entry["messaging"].([]any)
	for _, rawItem := range messaging {
		item, ok := rawItem.(map[string]any)
		if !ok {
			continue
		}
		delivery, ok := item["delivery"].(map[string]any)
		if !ok {
			continue
		}
		occurredAt := providerReceiptTimestamp(delivery["watermark"])
		if occurredAt.IsZero() {
			occurredAt = providerReceiptTimestamp(item["timestamp"])
		}
		messageIDs, _ := delivery["mids"].([]any)
		if occurredAt.IsZero() || len(messageIDs) == 0 {
			continue
		}
		for _, rawID := range messageIDs {
			messageID, ok := rawID.(string)
			if !ok || strings.TrimSpace(messageID) == "" {
				continue
			}
			receipts = append(receipts, conversation.ProviderDeliveryReceiptInput{
				Provider:          provider,
				ProviderMessageID: strings.TrimSpace(messageID),
				Status:            conversation.ProviderDeliveryDelivered,
				OccurredAt:        occurredAt,
			})
		}
	}
	return receipts
}

func providerReceiptTimestamp(value any) time.Time {
	var raw string
	switch typed := value.(type) {
	case string:
		raw = typed
	case float64:
		raw = strconv.FormatInt(int64(typed), 10)
	case json.Number:
		raw = typed.String()
	default:
		return time.Time{}
	}
	seconds, err := strconv.ParseInt(strings.TrimSpace(raw), 10, 64)
	if err != nil || seconds <= 0 {
		return time.Time{}
	}
	// Messenger watermarks are milliseconds; WhatsApp timestamps are seconds.
	if seconds > 100_000_000_000 {
		seconds /= 1000
	}
	return time.Unix(seconds, 0).UTC()
}

func normalizeWhatsAppEntry(entry map[string]any) []conversation.InboundEvent {
	var events []conversation.InboundEvent
	changes, _ := entry["changes"].([]any)
	for _, rawChange := range changes {
		change, ok := rawChange.(map[string]any)
		if !ok {
			continue
		}
		value, _ := change["value"].(map[string]any)
		if value == nil {
			continue
		}
		messages, _ := value["messages"].([]any)
		if len(messages) == 0 {
			continue
		}
		metadata, _ := value["metadata"].(map[string]any)
		phoneNumberID := stringFromMap(metadata, "phone_number_id")

		profileNames := map[string]string{}
		contacts, _ := value["contacts"].([]any)
		for _, rawContact := range contacts {
			contact, ok := rawContact.(map[string]any)
			if !ok {
				continue
			}
			profile, _ := contact["profile"].(map[string]any)
			waID := stringFromMap(contact, "wa_id")
			if waID != "" {
				profileNames[waID] = stringFromMap(profile, "name")
			}
		}

		for _, rawMessage := range messages {
			message, ok := rawMessage.(map[string]any)
			if !ok {
				continue
			}
			from := stringFromMap(message, "from")
			if from == "" {
				continue
			}
			text := whatsAppMessageText(message)
			if text == "" {
				// Media/interactive/unsupported message types with no textual
				// body — nothing to store yet.
				continue
			}
			name := profileNames[from]
			if name == "" {
				name = from
			}
			events = append(events, conversation.InboundEvent{
				Provider:          "whatsapp",
				ProviderEventID:   stringFromMap(message, "id"),
				ProviderMessageID: stringFromMap(message, "id"),
				ProviderThreadID:  fmt.Sprintf("%s:%s", phoneNumberID, from),
				Direction:         conversation.DirectionInbound,
				Subject:           "WhatsApp message",
				From:              conversation.ParticipantInput{Name: name, Phone: from},
				BodyText:          text,
			})
		}
	}
	return events
}

func whatsAppMessageText(message map[string]any) string {
	if text, ok := message["text"].(map[string]any); ok {
		return strings.TrimSpace(stringFromMap(text, "body"))
	}
	return ""
}

func normalizeMessengerEntry(entry map[string]any) []conversation.InboundEvent {
	var events []conversation.InboundEvent
	messaging, _ := entry["messaging"].([]any)
	pageID := stringFromMap(entry, "id")
	for _, rawItem := range messaging {
		item, ok := rawItem.(map[string]any)
		if !ok {
			continue
		}
		message, ok := item["message"].(map[string]any)
		if !ok {
			// delivery/read/postback events — not a message to store.
			continue
		}
		if isTrue(message, "is_echo") {
			continue
		}
		sender, _ := item["sender"].(map[string]any)
		psid := stringFromMap(sender, "id")
		text := strings.TrimSpace(stringFromMap(message, "text"))
		if psid == "" || text == "" {
			continue
		}
		events = append(events, conversation.InboundEvent{
			Provider:          "messenger",
			ProviderEventID:   stringFromMap(message, "mid"),
			ProviderMessageID: stringFromMap(message, "mid"),
			ProviderThreadID:  fmt.Sprintf("%s:%s", pageID, psid),
			Direction:         conversation.DirectionInbound,
			Subject:           "Messenger message",
			From:              conversation.ParticipantInput{Name: psid},
			BodyText:          text,
		})
	}
	return events
}

// normalizeInstagramEntry extracts Instagram Messaging inbound DMs. The wire
// shape is identical to Messenger (entry[].messaging[]) — only the top-level
// "object" distinguishes them — but the ids differ: entry.id is the IG
// business-account id (not a page id) and sender.id is an IGSID. The composite
// thread id therefore carries "igAccountId:igsid", which the outbound send op
// (instagram.messages.send) resolves to the linked Page server-side.
func normalizeInstagramEntry(entry map[string]any) []conversation.InboundEvent {
	var events []conversation.InboundEvent
	messaging, _ := entry["messaging"].([]any)
	igAccountID := stringFromMap(entry, "id")
	for _, rawItem := range messaging {
		item, ok := rawItem.(map[string]any)
		if !ok {
			continue
		}
		message, ok := item["message"].(map[string]any)
		if !ok {
			// delivery/read/reaction events — not a message to store.
			continue
		}
		if isTrue(message, "is_echo") {
			continue
		}
		sender, _ := item["sender"].(map[string]any)
		igsid := stringFromMap(sender, "id")
		text := strings.TrimSpace(stringFromMap(message, "text"))
		if igsid == "" || text == "" {
			continue
		}
		events = append(events, conversation.InboundEvent{
			Provider:          "instagram",
			ProviderEventID:   stringFromMap(message, "mid"),
			ProviderMessageID: stringFromMap(message, "mid"),
			ProviderThreadID:  fmt.Sprintf("%s:%s", igAccountID, igsid),
			Direction:         conversation.DirectionInbound,
			Subject:           "Instagram message",
			From:              conversation.ParticipantInput{Name: igsid},
			BodyText:          text,
		})
	}
	return events
}

func isTrue(m map[string]any, key string) bool {
	value, _ := m[key].(bool)
	return value
}

func stringFromMap(m map[string]any, key string) string {
	if m == nil {
		return ""
	}
	if value, ok := m[key].(string); ok {
		return strings.TrimSpace(value)
	}
	return ""
}

// normalizeSlackWebhookPayload extracts inbound human messages from a Slack
// Events API callback (the signed event_callback envelope). Only
// event.type=="message" items become Inbox messages; bot-authored events
// (bot_id set) and every message subtype (message_changed, message_deleted,
// bot_message, channel_join, …) are skipped — subtypes are edits/system
// notices, and storing bot echoes would loop our own replies back into the
// inbox. The thread ref is "channel" for top-level messages and
// "channel:thread_ts" for threaded replies, which buildSendOperation splits so
// outbound replies land in the right Slack thread.
func normalizeSlackWebhookPayload(payload map[string]any) ([]conversation.InboundEvent, error) {
	if payload == nil {
		return nil, fmt.Errorf("payload is empty")
	}
	if stringFromMap(payload, "type") != "event_callback" {
		// url_verification handshakes and other envelope types carry no
		// message content.
		return nil, nil
	}
	event, ok := payload["event"].(map[string]any)
	if !ok {
		return nil, nil
	}
	if stringFromMap(event, "type") != "message" {
		return nil, nil
	}
	if stringFromMap(event, "bot_id") != "" || stringFromMap(event, "subtype") != "" {
		return nil, nil
	}
	channel := stringFromMap(event, "channel")
	user := stringFromMap(event, "user")
	text := strings.TrimSpace(stringFromMap(event, "text"))
	ts := stringFromMap(event, "ts")
	if channel == "" || user == "" || text == "" || ts == "" {
		return nil, nil
	}

	threadRef := channel
	if threadTS := stringFromMap(event, "thread_ts"); threadTS != "" && threadTS != ts {
		threadRef = channel + ":" + threadTS
	}
	eventID := stringFromMap(payload, "event_id")
	if eventID == "" {
		eventID = channel + ":" + ts
	}
	return []conversation.InboundEvent{{
		Provider:          "slack",
		ProviderEventID:   eventID,
		ProviderMessageID: ts,
		ProviderThreadID:  threadRef,
		Direction:         conversation.DirectionInbound,
		Subject:           "Slack message",
		From:              conversation.ParticipantInput{Name: user},
		BodyText:          text,
		OccurredAt:        slackTimestamp(ts),
	}}, nil
}

// slackTimestamp converts a Slack ts ("1712345678.123456", seconds.micros)
// to a wall-clock time; zero time on parse failure (normalizeInboundEvent
// stamps now() for zero OccurredAt).
func slackTimestamp(ts string) time.Time {
	seconds, _, _ := strings.Cut(ts, ".")
	parsed, err := strconv.ParseInt(seconds, 10, 64)
	if err != nil || parsed <= 0 {
		return time.Time{}
	}
	return time.Unix(parsed, 0).UTC()
}
