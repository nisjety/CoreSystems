package postgres

import (
	"testing"
	"time"
)

func TestBillingUsageSeparatesSubscriptionAndInputOutput(t *testing.T) {
	at := time.Now().UTC()
	events := billingUsagePayloads("request", "subscription", 100, 20, at)
	if len(events) != 3 || events[0].Metric != "ai.subscription.requests" || events[1].Quantity != 100 || events[2].Quantity != 20 {
		t.Fatalf("incorrect meters: %+v", events)
	}
	for _, e := range events {
		if !e.OccurredAt.Equal(at) {
			t.Fatal("event time lost")
		}
	}
	if len(billingUsagePayloads("request", "", 100, 20, at)) != 0 {
		t.Fatal("unknown basis must not be billed")
	}
	if len(billingUsagePayloads("request", "metered", 0, 0, at)) != 1 {
		t.Fatal("zero tokens still count as a request")
	}
}
func TestBillingDeliveryConfigurationRejectsCredentialLeakTargets(t *testing.T) {
	for _, base := range []string{"https://user:pass@example.com", "https://example.com?token=x", "ftp://example.com", "https://example.com/#fragment"} {
		if _, err := NewBillingDelivery(base, "abcdefghijklmnopqrstuvwxyz1234567890"); err == nil {
			t.Fatalf("accepted %s", base)
		}
	}
}
