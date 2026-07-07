package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strings"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

const webhookReceivedDurable = "conversation-core-webhook-received"

// webhookReceivedSubject is the Ingestion Plane subject integration-corev2
// publishes on POST /api/v1/webhooks/:provider. It lives in the ingestion
// namespace (not application/model), so it needs its own JetStream stream —
// see eventing.EnsureIngestionStream.
const webhookReceivedSubject = "velion.ingestion.integration.webhook_received"

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

// WebhookReceivedConsumer bridges the Ingestion Plane → Application Plane: when
// integration-corev2 publishes webhook_received for a Meta WhatsApp/Messenger
// event, this fetches the full payload, normalizes it into an InboundEvent, and
// stores it as a conversation message — closing the inbound leg of the provider
// messaging loop (task #24). Non-message webhook events (delivery receipts,
// read receipts, other providers) are acked as a no-op; this consumer only
// handles conversational inbound content.
type WebhookReceivedConsumer struct {
	consumer *DurableConsumer
	fetcher  WebhookFetcher
	ingester EventIngester
}

func NewWebhookReceivedConsumer(js nats.JetStreamContext, fetcher WebhookFetcher, ingester EventIngester) *WebhookReceivedConsumer {
	return &WebhookReceivedConsumer{
		consumer: NewDurableConsumer(js, "webhook-received"),
		fetcher:  fetcher,
		ingester: ingester,
	}
}

// Start binds the durable consumer on the webhook received subject. The
// ingestion stream must exist first (see eventing.EnsureIngestionStream).
func (c *WebhookReceivedConsumer) Start(_ context.Context) error {
	return c.consumer.Bind(webhookReceivedSubject, webhookReceivedDurable, c.handle)
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
// if it carries a WhatsApp or Messenger message — normalizes and stores it.
// Every other case (missing ids, unsupported provider, non-message payload,
// invalid content) acks as a terminal no-op; only a transient fetch/store
// error retries. It is the testable core (no NATS required).
func (c *WebhookReceivedConsumer) process(ctx context.Context, ev ingestionEvent) outcome {
	orgID := strings.TrimSpace(ev.OrganizationID)
	webhookEventID := stringFromData(ev.Data, "webhookEventId")
	if orgID == "" || webhookEventID == "" {
		log.Printf("[cc-go/webhook-received] malformed event (missing organizationId/webhookEventId); skipping")
		return outcomeAck
	}

	providerKey := strings.ToLower(strings.TrimSpace(ev.ProviderKey))
	if providerKey != "whatsapp" && providerKey != "facebook" && providerKey != "meta" {
		// Not a channel this consumer handles (e.g. slack, github, shopify).
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

	events, err := normalizeMetaWebhookPayload(stored.Payload)
	if err != nil {
		log.Printf("[cc-go/webhook-received] normalize payload (org=%s id=%s): %v", orgID, webhookEventID, err)
		return outcomeAck
	}
	if len(events) == 0 {
		// A real Meta callback with no message content (status/delivery/read
		// receipts, verification pings) — nothing to store.
		return outcomeAck
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

// normalizeMetaWebhookPayload extracts WhatsApp Cloud API and Messenger Send
// API inbound message events from a raw Meta webhook body (the "object" +
// "entry" envelope common to all Meta webhook callbacks). Non-message entries
// (statuses, delivery, read, postback) are skipped, not errored — they are
// valid Meta callbacks this consumer simply has nothing to store for.
func normalizeMetaWebhookPayload(payload map[string]any) ([]conversation.InboundEvent, error) {
	if payload == nil {
		return nil, nil
	}
	entries, _ := payload["entry"].([]any)
	var events []conversation.InboundEvent
	for _, rawEntry := range entries {
		entry, ok := rawEntry.(map[string]any)
		if !ok {
			continue
		}
		events = append(events, normalizeWhatsAppEntry(entry)...)
		events = append(events, normalizeMessengerEntry(entry)...)
	}
	return events, nil
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

func stringFromMap(m map[string]any, key string) string {
	if m == nil {
		return ""
	}
	if value, ok := m[key].(string); ok {
		return strings.TrimSpace(value)
	}
	return ""
}
