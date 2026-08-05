// Package natsx — Phase 1 Slice 2 RED tests.
//
// These tests pin byte-exact parity with the Rust mp-events crate's
// subjects.rs. Test vectors mirror the tests in
// apps/Model Plane/rust/crates/mp-events/src/subjects.rs.
package natsx

import (
	"errors"
	"reflect"
	"testing"
)

// ---------------------------------------------------------------------------
// 1. Additional subject builders and wildcards (parity with Rust).
// ---------------------------------------------------------------------------

func TestUsageSubject_Parity(t *testing.T) {
	got := UsageSubject("org-1")
	want := "mp.v1.usage.org-1"
	if got != want {
		t.Fatalf("UsageSubject: got %q want %q", got, want)
	}
}

func TestStreamSubject_Parity(t *testing.T) {
	got := StreamSubject("started")
	want := "mp.v1.stream.started"
	if got != want {
		t.Fatalf("StreamSubject: got %q want %q", got, want)
	}
}

func TestUsageWildcard_Parity(t *testing.T) {
	if UsageWildcard != "mp.v1.usage.*" {
		t.Fatalf("UsageWildcard: got %q want %q", UsageWildcard, "mp.v1.usage.*")
	}
}

func TestStreamWildcard_Parity(t *testing.T) {
	if StreamWildcard != "mp.v1.stream.*" {
		t.Fatalf("StreamWildcard: got %q want %q", StreamWildcard, "mp.v1.stream.*")
	}
}

// ---------------------------------------------------------------------------
// 2. Named legacy aqencia constants (parity with Rust LEGACY_AQENCIA_*).
// ---------------------------------------------------------------------------

func TestLegacyAqenciaConstants_Parity(t *testing.T) {
	cases := []struct {
		name string
		got  string
		want string
	}{
		{"LegacyAqenciaReasoningStarted", LegacyAqenciaReasoningStarted, "aqencia.reasoning.reasoning.started"},
		{"LegacyAqenciaReasoningCompleted", LegacyAqenciaReasoningCompleted, "aqencia.reasoning.reasoning.completed"},
		{"LegacyAqenciaUsageRecorded", LegacyAqenciaUsageRecorded, "aqencia.reasoning.usage.recorded"},
		{"LegacyAqenciaDecisionMade", LegacyAqenciaDecisionMade, "aqencia.reasoning.decision.made"},
		{"LegacyAqenciaQuotaExceeded", LegacyAqenciaQuotaExceeded, "aqencia.reasoning.quota.exceeded"},
	}
	for _, c := range cases {
		if c.got != c.want {
			t.Errorf("%s: got %q want %q", c.name, c.got, c.want)
		}
	}
}

// ---------------------------------------------------------------------------
// 3. matchesPattern must support trailing `>` (parity with Rust).
// ---------------------------------------------------------------------------

func TestMatchesPattern_TrailingGreater(t *testing.T) {
	cases := []struct {
		subject string
		pattern string
		want    bool
	}{
		{"aqencia.reasoning.foo.bar", "aqencia.reasoning.>", true},
		{"aqencia.reasoning.x", "aqencia.reasoning.>", true},
		{"aqencia.other.foo", "aqencia.reasoning.>", false},
		{"mp.v1.run.r1.event", "mp.v1.>", true},
		{"a.b", "a.b.>", false}, // `>` requires at least one token
	}
	for _, c := range cases {
		got := matchesPattern(c.subject, c.pattern)
		if got != c.want {
			t.Errorf("matchesPattern(%q, %q) = %v want %v", c.subject, c.pattern, got, c.want)
		}
	}
}

// ---------------------------------------------------------------------------
// 4. TranslateNewToLegacy regressions (mirrors Rust translate_new_to_legacy).
// ---------------------------------------------------------------------------

func TestTranslateNewToLegacy_Parity(t *testing.T) {
	cases := []struct {
		in   string
		want string // "" means None
	}{
		{"mp.v1.run.01HXYZ.event", "verevon.agent.run.01HXYZ.event"},
		{"mp.v1.session.abc-123.command", "verevon.session.abc-123.command"},
		{"mp.v1.ingress.usage", "aqencia.reasoning.usage.recorded"},
		{"mp.v1.ingress.decision", "aqencia.reasoning.decision.made"},
		{"mp.v1.ingress.quota_exceeded", "aqencia.reasoning.quota.exceeded"},
		{"mp.v1.ingress.accepted", ""},
	}
	for _, c := range cases {
		got := TranslateNewToLegacy(c.in)
		if got != c.want {
			t.Errorf("TranslateNewToLegacy(%q) = %q want %q", c.in, got, c.want)
		}
	}
}

// ---------------------------------------------------------------------------
// 5. SubscriberSubjects + SubjectSelectionError (mirrors Rust subscriber_subjects).
//
// Semantics (per Rust):
//   - If canonical does NOT start with "mp.v1" → passthrough for ALL modes.
//   - V1Only | DualWrite → canonical only.
//   - DualRead → canonical + (legacy if reverse mapping exists).
//   - LegacyOnly → legacy only; error NoLegacyMapping if no reverse.
// ---------------------------------------------------------------------------

func TestSubscriberSubjects_V1Only(t *testing.T) {
	got, err := SubscriberSubjects("mp.v1.run.r1.event", ModeV1Only)
	if err != nil {
		t.Fatalf("unexpected err: %v", err)
	}
	want := []string{"mp.v1.run.r1.event"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("V1Only: got %v want %v", got, want)
	}
}

func TestSubscriberSubjects_DualWrite_CanonicalOnly(t *testing.T) {
	got, err := SubscriberSubjects("mp.v1.run.r1.event", ModeDualWrite)
	if err != nil {
		t.Fatalf("unexpected err: %v", err)
	}
	want := []string{"mp.v1.run.r1.event"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("DualWrite: got %v want %v", got, want)
	}
}

func TestSubscriberSubjects_DualRead_CanonicalPlusLegacy(t *testing.T) {
	got, err := SubscriberSubjects("mp.v1.run.r1.event", ModeDualRead)
	if err != nil {
		t.Fatalf("unexpected err: %v", err)
	}
	want := []string{"mp.v1.run.r1.event", "verevon.agent.run.r1.event"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("DualRead: got %v want %v", got, want)
	}
}

func TestSubscriberSubjects_LegacyOnly_Success(t *testing.T) {
	got, err := SubscriberSubjects("mp.v1.run.r1.event", ModeLegacyOnly)
	if err != nil {
		t.Fatalf("unexpected err: %v", err)
	}
	want := []string{"verevon.agent.run.r1.event"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("LegacyOnly: got %v want %v", got, want)
	}
}

func TestSubscriberSubjects_NonCanonicalPassthrough_AllModes(t *testing.T) {
	modes := []CompatMode{ModeV1Only, ModeDualWrite, ModeDualRead, ModeLegacyOnly}
	for _, m := range modes {
		got, err := SubscriberSubjects("tools.completions.*", m)
		if err != nil {
			t.Errorf("mode %s: unexpected err: %v", m, err)
			continue
		}
		want := []string{"tools.completions.*"}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("mode %s non-canonical passthrough: got %v want %v", m, got, want)
		}
	}
}

func TestSubscriberSubjects_LegacyOnly_NoMapping_ReturnsError(t *testing.T) {
	_, err := SubscriberSubjects("mp.v1.ingress.accepted", ModeLegacyOnly)
	if err == nil {
		t.Fatalf("expected error, got nil")
	}
	var sse *SubjectSelectionError
	if !errors.As(err, &sse) {
		t.Fatalf("expected *SubjectSelectionError, got %T: %v", err, err)
	}
	if sse.Subject != "mp.v1.ingress.accepted" {
		t.Fatalf("Subject: got %q want %q", sse.Subject, "mp.v1.ingress.accepted")
	}
}
