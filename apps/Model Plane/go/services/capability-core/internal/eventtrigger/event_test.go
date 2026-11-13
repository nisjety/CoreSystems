package eventtrigger

import (
	"strings"
	"testing"
	"time"
)

const validEnvelope = `{"type":"verevon.ingestion.integration.webhook_received","organizationId":" org-1 ",` +
	`"connectionId":"conn-1","providerKey":"whatsapp","data":{"eventType":"message","webhookEventId":"wh-1",` +
	`"normalizedBy":"meta"}}`

// timedEnvelope is validEnvelope with the createdAt integration-corev2 stamps on every webhook_received event.
const timedEnvelope = `{"type":"verevon.ingestion.integration.webhook_received","organizationId":" org-1 ",` +
	`"connectionId":"conn-1","createdAt":"2026-11-08T11:59:30Z","providerKey":"whatsapp","data":{"eventType":"message",` +
	`"webhookEventId":"wh-1","normalizedBy":"meta"}}`

// eventCreatedAt is the createdAt timedEnvelope carries.
var eventCreatedAt = time.Date(2026, 11, 8, 11, 59, 30, 0, time.UTC)

func TestParseWebhookReceivedReadsOnlyTheMetadata(t *testing.T) {
	got, err := ParseWebhookReceived([]byte(validEnvelope))
	if err != nil {
		t.Fatalf("ParseWebhookReceived: %v", err)
	}
	want := WebhookReceived{OrgID: "org-1", ConnectionID: "conn-1", WebhookEventID: "wh-1"}
	if got != want {
		t.Fatalf("ParseWebhookReceived = %+v; want %+v", got, want)
	}
}

func TestParseWebhookReceivedReadsTheEventsCreationTime(t *testing.T) {
	got, err := ParseWebhookReceived([]byte(timedEnvelope))
	if err != nil {
		t.Fatalf("ParseWebhookReceived: %v", err)
	}
	if !got.EventAt.Equal(eventCreatedAt) || got.WebhookEventID != "wh-1" {
		t.Fatalf("ParseWebhookReceived = %+v; want the envelope's createdAt %v", got, eventCreatedAt)
	}
}

func TestParseWebhookReceivedLeavesTheTimeZeroWhenTheEnvelopeHasNoCreationTime(t *testing.T) {
	got, err := ParseWebhookReceived([]byte(validEnvelope))
	if err != nil {
		t.Fatalf("ParseWebhookReceived: %v", err)
	}
	if !got.EventAt.IsZero() {
		t.Fatalf("EventAt = %v; want zero, so the consumer falls back to the broker's time", got.EventAt)
	}
}

func TestParseWebhookReceivedRefusesAnEventThatNamesNoTrigger(t *testing.T) {
	for _, payload := range []string{
		`{"connectionId":"conn-1","data":{"webhookEventId":"wh-1"}}`,
		`{"organizationId":"org-1","data":{"webhookEventId":"wh-1"}}`,
		`{"organizationId":"org-1","connectionId":"conn-1","data":{}}`,
		`{"organizationId":"org-1","connectionId":"conn-1","data":{"webhookEventId":7}}`,
		`{`,
	} {
		if _, err := ParseWebhookReceived([]byte(payload)); err == nil {
			t.Errorf("accepted %s", payload)
		}
	}
}

func TestParseWebhookReceivedBoundsEveryID(t *testing.T) {
	long := strings.Repeat("x", maxIDChars+1)
	for _, payload := range []string{
		`{"organizationId":"` + long + `","connectionId":"conn-1","data":{"webhookEventId":"wh-1"}}`,
		`{"organizationId":"org-1","connectionId":"` + long + `","data":{"webhookEventId":"wh-1"}}`,
		`{"organizationId":"org-1","connectionId":"conn-1","data":{"webhookEventId":"` + long + `"}}`,
	} {
		if _, err := ParseWebhookReceived([]byte(payload)); err == nil {
			t.Errorf("accepted an id longer than %d characters", maxIDChars)
		}
	}
	edge := strings.Repeat("x", maxIDChars)
	exact := `{"organizationId":"org-1","connectionId":"conn-1","data":{"webhookEventId":"` + edge + `"}}`
	if _, err := ParseWebhookReceived([]byte(exact)); err != nil {
		t.Fatalf("an id of exactly %d characters was refused: %v", maxIDChars, err)
	}
}

func TestLimitsFromEnvKeepsEveryBudgetPositive(t *testing.T) {
	getenv := func(name string) string {
		if name == "EVENT_TRIGGER_FIRES_PER_TRIGGER_HOUR" {
			return "7"
		}
		return "0"
	}
	got := LimitsFromEnv(getenv)
	if got.TriggerPerHour != 7 || got.OrgPerHour != DefaultOrgFiresPerHour || got.MaxAttempts != DefaultMaxAuthAttempts {
		t.Fatalf("LimitsFromEnv = %+v; a zero org budget must take its default", got)
	}
	unparsable := LimitsFromEnv(func(string) string { return "many" })
	if unparsable.TriggerPerHour != DefaultTriggerFiresPerHour || unparsable.OrgPerHour != DefaultOrgFiresPerHour {
		t.Fatalf("LimitsFromEnv = %+v; an unparsable budget must take its default", unparsable)
	}
	if LimitsFromEnv(nil).TriggerPerHour != DefaultTriggerFiresPerHour {
		t.Fatal("a nil environment reader must take the defaults")
	}
}
