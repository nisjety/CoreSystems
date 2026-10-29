package nats

import "testing"

func TestPlanChangeMessageIDIsStableAndRevisionBound(t *testing.T) {
	if got, want := planChangeMessageID("org-1", 42), "organization-plan:org-1:42"; got != want {
		t.Fatalf("planChangeMessageID()=%q; want %q", got, want)
	}
}
