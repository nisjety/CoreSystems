// Package eventtrigger is the fire path of an event trigger (EP-7, F-t3-047, Wave 4 slices W4-F T2 and T5).
//
// A durable consumer of integration-corev2's webhook_received event records one pending fire for each live
// trigger bound to that event's connection, keyed on the webhook event id, so a redelivered event adds nothing.
// A sweeper then decides each pending fire: the trigger must still be active, the trigger and its org must be
// under their hourly budgets, a trigger with a wake filter must pass the wake gate (wakegate.go), and Control must
// authorise the fire afresh through the same FireAuthorizer the cron sweeper uses. An authorised fire is recorded as
// authorized, and the starter turns it into a budgeted run. The wake gate alone reads a webhook body, and only for a
// trigger that carries a filter: it holds the body in memory for one decision and never stores or logs it.
package eventtrigger

import (
	"encoding/json"
	"fmt"
	"strings"
	"unicode/utf8"
)

const (
	// WebhookReceivedSubject is the subject integration-corev2 publishes once it has stored a webhook body. The
	// event carries metadata only, so the subject is all this package needs.
	WebhookReceivedSubject = "verevon.ingestion.integration.webhook_received"
	// WebhookStream is the stream WebhookReceivedSubject lives on. The Ingestion Plane owns it.
	WebhookStream = "VEREVON_INGESTION"
	// WebhookDurable is the durable this package binds. Its provisioning, a consumer on WebhookStream filtered to
	// WebhookReceivedSubject, belongs to the stream owner. This package never creates it.
	WebhookDurable = "capability-core-event-triggers-v1"
)

// maxIDChars is the longest org, connection or webhook event id the fire table keeps.
const maxIDChars = 128

// WebhookReceived is the part of a webhook_received event this package reads.
type WebhookReceived struct {
	OrgID          string
	ConnectionID   string
	WebhookEventID string
}

// webhookEnvelope is integration-corev2's events.Event as the webhook_received message carries it.
type webhookEnvelope struct {
	OrganizationID string         `json:"organizationId"`
	ConnectionID   string         `json:"connectionId"`
	Data           map[string]any `json:"data"`
}

// ParseWebhookReceived decodes one webhook_received envelope. An event without an org, a connection or a webhook
// event id names no trigger, so it is refused rather than recorded.
func ParseWebhookReceived(payload []byte) (WebhookReceived, error) {
	var env webhookEnvelope
	if err := json.Unmarshal(payload, &env); err != nil {
		return WebhookReceived{}, fmt.Errorf("decode webhook_received: %w", err)
	}
	event := WebhookReceived{
		OrgID:          strings.TrimSpace(env.OrganizationID),
		ConnectionID:   strings.TrimSpace(env.ConnectionID),
		WebhookEventID: textField(env.Data, "webhookEventId"),
	}
	required := []struct{ name, value string }{
		{"organizationId", event.OrgID},
		{"connectionId", event.ConnectionID},
		{"webhookEventId", event.WebhookEventID},
	}
	for _, field := range required {
		if field.value == "" {
			return WebhookReceived{}, fmt.Errorf("webhook_received %s is required", field.name)
		}
		if utf8.RuneCountInString(field.value) > maxIDChars {
			return WebhookReceived{}, fmt.Errorf("webhook_received %s is longer than %d characters", field.name, maxIDChars)
		}
	}
	return event, nil
}

func textField(fields map[string]any, key string) string {
	value, _ := fields[key].(string)
	return strings.TrimSpace(value)
}
