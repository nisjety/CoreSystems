package events

import "testing"

func TestNATSPublisherSubjectPrefix(t *testing.T) {
	publisher := &NATSPublisher{subjectPrefix: "verevon.events"}
	if got := publisher.subject("integration.connected"); got != "verevon.events.integration.connected" {
		t.Fatalf("subject = %q, want verevon.events.integration.connected", got)
	}
}

func TestNATSPublisherSubjectWithoutPrefix(t *testing.T) {
	publisher := &NATSPublisher{}
	if got := publisher.subject("integration.connected"); got != "integration.connected" {
		t.Fatalf("subject = %q, want integration.connected", got)
	}
}
