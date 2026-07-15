package nats

import "testing"

func TestBillingPlanChangeMessageIDIsStableAndRevisionBound(t *testing.T) {
	if got, want := billingPlanChangeMessageID("org-1", 42), "billing-plan:org-1:42"; got != want {
		t.Fatalf("billingPlanChangeMessageID()=%q; want %q", got, want)
	}
}
