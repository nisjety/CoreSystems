package nats

import (
	"strings"
	"testing"

	gonats "github.com/nats-io/nats.go"
)

func TestDecodeUsageMessageUsesStableIdentityAcrossRedelivery(t *testing.T) {
	body := []byte(`{"org_id":"org-usage","metric":"api_calls","quantity":2,"source":"model-plane","occurred_at":"2026-07-15T10:00:00Z","metadata":{"run_id":"run-1"}}`)
	first, err := decodeUsageMessage(&gonats.Msg{Subject: "usage.model", Data: body})
	if err != nil {
		t.Fatalf("decode first delivery: %v", err)
	}
	second, err := decodeUsageMessage(&gonats.Msg{Subject: "usage.model", Data: append([]byte(nil), body...)})
	if err != nil {
		t.Fatalf("decode redelivery: %v", err)
	}
	if first.EventID != second.EventID || !strings.HasPrefix(first.EventID, "evt_") {
		t.Fatalf("redelivery identity first=%q second=%q", first.EventID, second.EventID)
	}
	if !first.OccurredAt.Equal(second.OccurredAt) {
		t.Fatalf("redelivery timestamps first=%s second=%s", first.OccurredAt, second.OccurredAt)
	}
}

func TestDecodeUsageMessagePrefersProducerIdentityThenNATSIdentity(t *testing.T) {
	bodyWithID := []byte(`{"event_id":"producer_usage_01","org_id":"org-usage","metric":"api_calls","quantity":2,"occurred_at":"2026-07-15T10:00:00Z"}`)
	msg := &gonats.Msg{Data: bodyWithID, Header: gonats.Header{}}
	msg.Header.Set("Nats-Msg-Id", "nats_usage_01")
	usage, err := decodeUsageMessage(msg)
	if err != nil {
		t.Fatalf("decode producer identity: %v", err)
	}
	if usage.EventID != "producer_usage_01" {
		t.Fatalf("event id=%q; want producer identity", usage.EventID)
	}

	bodyWithoutID := []byte(`{"org_id":"org-usage","metric":"api_calls","quantity":2,"occurred_at":"2026-07-15T10:00:00Z"}`)
	msg = &gonats.Msg{Data: bodyWithoutID, Header: gonats.Header{}}
	msg.Header.Set("Nats-Msg-Id", "nats_usage_01")
	usage, err = decodeUsageMessage(msg)
	if err != nil {
		t.Fatalf("decode NATS identity: %v", err)
	}
	if usage.EventID != "nats_usage_01" {
		t.Fatalf("event id=%q; want NATS identity", usage.EventID)
	}
}

func TestDecodeUsageMessageFailsClosedWithoutValidOccurredAt(t *testing.T) {
	for _, body := range []string{
		`{"event_id":"usage_01","org_id":"org-usage","metric":"api_calls","quantity":2}`,
		`{"event_id":"usage_01","org_id":"org-usage","metric":"api_calls","quantity":2,"occurred_at":"later"}`,
	} {
		if _, err := decodeUsageMessage(&gonats.Msg{Data: []byte(body)}); err == nil {
			t.Fatalf("invalid durable usage timestamp accepted: %s", body)
		}
	}
}

func TestDecodeUsageMessageRejectsInvalidProducerIdentity(t *testing.T) {
	body := []byte(`{"event_id":"usage id","org_id":"org-usage","metric":"api_calls","quantity":2,"occurred_at":"2026-07-15T10:00:00Z"}`)
	if _, err := decodeUsageMessage(&gonats.Msg{Data: body}); err == nil {
		t.Fatal("invalid caller event id was replaced instead of rejected")
	}
}
