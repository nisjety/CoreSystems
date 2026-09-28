package learning

import "testing"

func TestHashIdempotencyKeyHashesAndValidatesInput(t *testing.T) {
	first, err := HashIdempotencyKey("attempt-client-key-0001")
	if err != nil {
		t.Fatalf("valid key rejected: %v", err)
	}
	second, err := HashIdempotencyKey("attempt-client-key-0002")
	if err != nil {
		t.Fatalf("second valid key rejected: %v", err)
	}
	if len(first) != 64 || first == second || first == "attempt-client-key-0001" {
		t.Fatalf("unexpected key digest behavior: first=%q second=%q", first, second)
	}
	for _, key := range []string{"", "short", "key with spaces!!", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"} {
		if _, err := HashIdempotencyKey(key); err == nil {
			t.Errorf("invalid key accepted: %q", key)
		}
	}
}

func TestNewLearningHandlerRegistersAttemptRoutes(t *testing.T) {
	if NewHandler(nil, nil) == nil {
		t.Fatal("Learning API handler was not created")
	}
}

func TestAttemptModesAndEventsAreBounded(t *testing.T) {
	for _, mode := range []string{"guide", "demonstrate", "practice"} {
		if !validAttemptMode(mode) {
			t.Errorf("valid mode rejected: %q", mode)
		}
	}
	if validAttemptMode("automatic-mastery") {
		t.Fatal("unsupported attempt mode accepted")
	}
	for _, eventType := range []string{"step_started", "assistance_used", "learner_reported_complete", "blocked"} {
		if !validAttemptEvent(eventType, "title-slide") {
			t.Errorf("valid step event rejected: %q", eventType)
		}
		if validAttemptEvent(eventType, "") {
			t.Errorf("step event without a step ID accepted: %q", eventType)
		}
	}
	if !validAttemptEvent("attempt_finished", "") ||
		!validAttemptEvent("attempt_abandoned", "") ||
		validAttemptEvent("attempt_finished", "title-slide") ||
		validAttemptEvent("step_completed", "title-slide") {
		t.Fatal("attempt-finish or unsupported event validation failed")
	}
}
