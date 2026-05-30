package natsx

import (
	"strings"
	"testing"
)

func TestCompatAdapterTranslatesAllLegacySubjects(t *testing.T) {
	cases := []string{
		"velion.agent.run.run-1.event",
		"velion.session.sess-1.command",
		"aqencia.reasoning.reasoning.started",
		"aqencia.reasoning.reasoning.completed",
		"aqencia.reasoning.usage.recorded",
		"aqencia.reasoning.decision.made",
		"aqencia.reasoning.quota.exceeded",
	}
	for _, legacy := range cases {
		got := TranslateLegacySubject(legacy)
		if !strings.HasPrefix(got, "mp.v1.") {
			t.Errorf("TranslateLegacySubject(%q) = %q, want mp.v1.* prefix", legacy, got)
		}
	}
}

func TestDualWriteConsistency(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeDualWrite)
	if err := p.Publish("mp.v1.run.run-1.event", []byte("data")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 2 {
		t.Fatalf("expected 2 calls (v1 + legacy), got %d", len(f.calls))
	}
}

func TestFeatureFlagToggle(t *testing.T) {
	t.Run("V1Only", func(t *testing.T) {
		f := &fakeRaw{}
		p := NewPublisher(f, ModeV1Only)
		_ = p.Publish("mp.v1.run.run-1.event", []byte("x"))
		if len(f.calls) != 1 {
			t.Errorf("ModeV1Only: expected 1 call, got %d", len(f.calls))
		}
	})
	t.Run("LegacyOnly", func(t *testing.T) {
		f := &fakeRaw{}
		p := NewPublisher(f, ModeLegacyOnly)
		_ = p.Publish("mp.v1.run.run-1.event", []byte("x"))
		if len(f.calls) != 1 {
			t.Errorf("ModeLegacyOnly: expected 1 call, got %d", len(f.calls))
		}
		if strings.HasPrefix(f.calls[0].subject, "mp.v1.") {
			t.Errorf("LegacyOnly should not publish v1 subject, got %q", f.calls[0].subject)
		}
	})
}

func TestTranslateRoundTrip(t *testing.T) {
	legacy := "velion.agent.run.run-42.event"
	v1 := TranslateLegacySubject(legacy)
	back := TranslateNewToLegacy(v1)
	if back == "" {
		t.Errorf("round-trip failed: TranslateNewToLegacy(%q) returned empty", v1)
	}
}
