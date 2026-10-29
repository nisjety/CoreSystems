package events

import (
	"context"
	"testing"
)

func TestNoopPublisher_NeverErrors(t *testing.T) {
	var p Publisher = NoopPublisher{}
	if err := p.Publish(context.Background(), Event{Type: "booking.delivered"}); err != nil {
		t.Fatalf("NoopPublisher.Publish returned an error: %v", err)
	}
}

func TestNATSPublisher_Subject_PrefixesEventType(t *testing.T) {
	p := &NATSPublisher{subjectPrefix: "shipping-core"}
	if got := p.subject("booking.delivered"); got != "shipping-core.booking.delivered" {
		t.Errorf("subject() = %q, want %q", got, "shipping-core.booking.delivered")
	}
}

func TestNATSPublisher_Subject_NoPrefixPassesThrough(t *testing.T) {
	p := &NATSPublisher{subjectPrefix: ""}
	if got := p.subject("booking.delivered"); got != "booking.delivered" {
		t.Errorf("subject() = %q, want %q", got, "booking.delivered")
	}
}

func TestNATSPublisher_Subject_TrimsStrayDotsAndSpaces(t *testing.T) {
	p := &NATSPublisher{subjectPrefix: " shipping-core. "}
	if got := p.subject(" .booking.delivered. "); got != "shipping-core.booking.delivered" {
		t.Errorf("subject() = %q, want %q", got, "shipping-core.booking.delivered")
	}
}

func TestNewConfigFromEnv_DefaultsSubjectPrefix(t *testing.T) {
	t.Setenv("NATS_URL", "")
	t.Setenv("NATS_SUBJECT_PREFIX", "")
	cfg := NewConfigFromEnv()
	if cfg.SubjectPrefix != "shipping-core" {
		t.Errorf("SubjectPrefix = %q, want default shipping-core", cfg.SubjectPrefix)
	}
	if cfg.URL != "" {
		t.Errorf("URL = %q, want empty (not configured)", cfg.URL)
	}
}
