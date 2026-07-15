package subscriber

import "testing"

func TestEventAuthorityMatchesExactV2Subject(t *testing.T) {
	t.Parallel()

	if !eventAuthorityMatches(
		"audit",
		"velion.audit.v2.control.auth-core.signed_in",
		"control",
		"auth-core",
		"signed_in",
		"control",
	) {
		t.Fatal("exact v2 audit authority was rejected")
	}
	if !eventAuthorityMatches(
		"usage",
		"velion.usage.v2.model.session-core.tokens",
		"model",
		"session-core",
		"tokens",
		"model",
	) {
		t.Fatal("exact v2 usage authority was rejected")
	}
}

func TestEventAuthorityRejectsLegacyAndForgedSubjects(t *testing.T) {
	t.Parallel()

	tests := map[string]struct {
		subject        string
		payloadPlane   string
		producer       string
		event          string
		authorityPlane string
	}{
		"legacy-v1":       {"velion.audit.v1.control.auth-core.signed_in", "control", "auth-core", "signed_in", "control"},
		"wrong-plane":     {"velion.audit.v2.model.auth-core.signed_in", "control", "auth-core", "signed_in", "control"},
		"wrong-producer":  {"velion.audit.v2.control.user-core.signed_in", "control", "auth-core", "signed_in", "control"},
		"wrong-event":     {"velion.audit.v2.control.auth-core.role_changed", "control", "auth-core", "signed_in", "control"},
		"extra-segment":   {"velion.audit.v2.control.auth-core.signed_in.forged", "control", "auth-core", "signed_in", "control"},
		"empty-segment":   {"velion.audit.v2.control..signed_in", "control", "auth-core", "signed_in", "control"},
		"wrong-authority": {"velion.audit.v2.control.auth-core.signed_in", "control", "auth-core", "signed_in", "model"},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if eventAuthorityMatches("audit", tc.subject, tc.payloadPlane, tc.producer, tc.event, tc.authorityPlane) {
				t.Fatal("forged subject was accepted")
			}
		})
	}
}

func TestSubscriberUsesV3ConsumerForV2AuthorityContract(t *testing.T) {
	t.Parallel()

	if got := New(nil, nil, "model", "model").consumerName("audit"); got != "audit-core-model-v3-audit" {
		t.Fatalf("consumer name = %q", got)
	}
	if got := planeSubject("audit", "model"); got != "velion.audit.v2.model.>" {
		t.Fatalf("plane subject = %q", got)
	}
}

func TestDeadLetterMessageIDIsStableAndSourceBound(t *testing.T) {
	t.Parallel()

	first := deadLetterMessageID("model", "audit", "malformed", 42)
	if first == "" {
		t.Fatal("dead-letter message id is empty")
	}
	if again := deadLetterMessageID("model", "audit", "malformed", 42); again != first {
		t.Fatalf("message id changed across retry: %q != %q", again, first)
	}
	if changed := deadLetterMessageID("model", "audit", "malformed", 43); changed == first {
		t.Fatal("separate source sequence reused dead-letter message id")
	}
}

func TestConsumerHealthReportsLastSuccessfulAck(t *testing.T) {
	t.Parallel()

	subscriber := New(nil, nil)
	subscriber.recordAck("audit")
	health := subscriber.Health()
	if health.Audit.LastAckAt.IsZero() {
		t.Fatal("audit last successful ACK was not exposed")
	}
	if !health.Usage.LastAckAt.IsZero() {
		t.Fatal("usage ACK timestamp changed without an ACK")
	}
}
