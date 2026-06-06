package hotpath

import (
	"encoding/json"
	"os"
	"testing"
)

func TestNormalizeWebhookBuildsStableReplayID(t *testing.T) {
	body := []byte(`{"id":"evt_1","type":"account.updated","organizationId":"org-1"}`)

	first, err := NormalizeWebhook(WebhookInput{
		ProviderKey: "stripe",
		Headers:     map[string]string{"Stripe-Signature": "t=1,v1=abc"},
		Body:        body,
	})
	if err != nil {
		t.Fatalf("NormalizeWebhook error: %v", err)
	}
	second, err := NormalizeWebhook(WebhookInput{
		ProviderKey: "stripe",
		Headers:     map[string]string{"Stripe-Signature": "t=2,v1=def"},
		Body:        body,
	})
	if err != nil {
		t.Fatalf("NormalizeWebhook second error: %v", err)
	}
	if first.EventID != second.EventID {
		t.Fatalf("EventID changed for same provider/type/payload id: %q != %q", first.EventID, second.EventID)
	}
	if first.EventType != "account.updated" {
		t.Fatalf("EventType = %q, want account.updated", first.EventType)
	}
	if first.OrganizationID != "org-1" {
		t.Fatalf("OrganizationID = %q, want org-1", first.OrganizationID)
	}
	if first.SignatureHash == "" {
		t.Fatal("SignatureHash is empty")
	}
	if first.Payload["id"] != "evt_1" {
		t.Fatalf("Payload id = %#v, want evt_1", first.Payload["id"])
	}
}

func TestNormalizeWebhookUsesProviderHeaders(t *testing.T) {
	normalized, err := NormalizeWebhook(WebhookInput{
		ProviderKey: "github",
		Headers: map[string]string{
			"x-github-delivery": "delivery-1",
			"X-GitHub-Event":    "push",
			"X-Org-ID":          "org-1",
		},
		Body: []byte(`{"repository":{"name":"demo"}}`),
	})
	if err != nil {
		t.Fatalf("NormalizeWebhook error: %v", err)
	}
	if normalized.EventType != "push" {
		t.Fatalf("EventType = %q, want push", normalized.EventType)
	}
	if normalized.ReplayKey != "delivery-1" {
		t.Fatalf("ReplayKey = %q, want delivery-1", normalized.ReplayKey)
	}
	if normalized.OrganizationID != "org-1" {
		t.Fatalf("OrganizationID = %q, want org-1", normalized.OrganizationID)
	}
}

func TestNormalizeWebhookRejectsInvalidJSON(t *testing.T) {
	if _, err := NormalizeWebhook(WebhookInput{ProviderKey: "stripe", Body: []byte(`not-json`)}); err == nil {
		t.Fatal("NormalizeWebhook returned nil error for invalid JSON")
	}
}

func TestNormalizeWebhookMatchesSharedFixtures(t *testing.T) {
	fixtures := loadWebhookFixtures(t)
	for _, fixture := range fixtures {
		t.Run(fixture.Name, func(t *testing.T) {
			normalized, err := NormalizeWebhook(WebhookInput{
				ProviderKey: fixture.ProviderKey,
				Headers:     fixture.Headers,
				Body:        []byte(fixture.Body),
			})
			if err != nil {
				t.Fatalf("NormalizeWebhook error: %v", err)
			}
			if normalized.SchemaVersion != fixture.Expected.SchemaVersion {
				t.Fatalf("SchemaVersion = %d, want %d", normalized.SchemaVersion, fixture.Expected.SchemaVersion)
			}
			if normalized.ProviderKey != fixture.Expected.ProviderKey {
				t.Fatalf("ProviderKey = %q, want %q", normalized.ProviderKey, fixture.Expected.ProviderKey)
			}
			if normalized.EventType != fixture.Expected.EventType {
				t.Fatalf("EventType = %q, want %q", normalized.EventType, fixture.Expected.EventType)
			}
			if normalized.OrganizationID != fixture.Expected.OrganizationID {
				t.Fatalf("OrganizationID = %q, want %q", normalized.OrganizationID, fixture.Expected.OrganizationID)
			}
			if normalized.ReplayKey != fixture.Expected.ReplayKey {
				t.Fatalf("ReplayKey = %q, want %q", normalized.ReplayKey, fixture.Expected.ReplayKey)
			}
			if normalized.EventID != fixture.Expected.EventID {
				t.Fatalf("EventID = %q, want %q", normalized.EventID, fixture.Expected.EventID)
			}
		})
	}
}

type webhookFixture struct {
	Name        string            `json:"name"`
	ProviderKey string            `json:"providerKey"`
	Headers     map[string]string `json:"headers"`
	Body        string            `json:"body"`
	Expected    struct {
		SchemaVersion  int    `json:"schemaVersion"`
		ProviderKey    string `json:"providerKey"`
		EventType      string `json:"eventType"`
		OrganizationID string `json:"organizationId"`
		ReplayKey      string `json:"replayKey"`
		EventID        string `json:"eventId"`
	} `json:"expected"`
}

func loadWebhookFixtures(t *testing.T) []webhookFixture {
	t.Helper()
	data, err := os.ReadFile("../../services/webhook-normalizer-rs/testdata/webhook_fixtures.json")
	if err != nil {
		t.Fatalf("read webhook fixtures: %v", err)
	}
	var fixtures []webhookFixture
	if err := json.Unmarshal(data, &fixtures); err != nil {
		t.Fatalf("decode webhook fixtures: %v", err)
	}
	return fixtures
}
