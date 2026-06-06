package events

import "testing"

func TestNATSPublisherSubjectPrefix(t *testing.T) {
	publisher := &NATSPublisher{subjectPrefix: "velion.events"}
	if got := publisher.subject("integration.connected"); got != "velion.events.integration.connected" {
		t.Fatalf("subject = %q, want velion.events.integration.connected", got)
	}
}

func TestNATSPublisherSubjectWithoutPrefix(t *testing.T) {
	publisher := &NATSPublisher{}
	if got := publisher.subject("integration.connected"); got != "integration.connected" {
		t.Fatalf("subject = %q, want integration.connected", got)
	}
}
