package natsx_test

import (
	"testing"

	"github.com/triodelab/model-plane/pkg/natsx"
)

func TestTranslateLegacySubject(t *testing.T) {
	tests := []struct {
		legacy string
		want   string
	}{
		{
			legacy: "verevon.agent.run.01HXYZ.event",
			want:   "mp.v1.run.01HXYZ.event",
		},
		{
			legacy: "verevon.session.abc-123.command",
			want:   "mp.v1.session.abc-123.command",
		},
		{
			legacy: "aqencia.reasoning.reasoning.started",
			want:   "mp.v1.ingress.run_started_compat",
		},
		{
			legacy: "aqencia.reasoning.reasoning.completed",
			want:   "mp.v1.ingress.run_completed_compat",
		},
		{
			legacy: "aqencia.reasoning.usage.recorded",
			want:   "mp.v1.ingress.usage",
		},
		{
			legacy: "aqencia.reasoning.decision.made",
			want:   "mp.v1.ingress.decision",
		},
		{
			legacy: "aqencia.reasoning.quota.exceeded",
			want:   "mp.v1.ingress.quota_exceeded",
		},
		{
			legacy: "some.unknown.subject",
			want:   "some.unknown.subject",
		},
	}

	for _, tt := range tests {
		t.Run(tt.legacy, func(t *testing.T) {
			got := natsx.TranslateLegacySubject(tt.legacy)
			if got != tt.want {
				t.Errorf("TranslateLegacySubject(%q) = %q, want %q", tt.legacy, got, tt.want)
			}
		})
	}
}

func TestTranslateNewToLegacy(t *testing.T) {
	tests := []struct {
		name string
		v1   string
		want string
	}{
		{"run event", "mp.v1.run.01HXYZ.event", "verevon.agent.run.01HXYZ.event"},
		{"session command", "mp.v1.session.abc-123.command", "verevon.session.abc-123.command"},
		{"ingress usage reverse", "mp.v1.ingress.usage", "aqencia.reasoning.usage.recorded"},
		{"ingress decision reverse", "mp.v1.ingress.decision", "aqencia.reasoning.decision.made"},
		{"ingress quota reverse", "mp.v1.ingress.quota_exceeded", "aqencia.reasoning.quota.exceeded"},
		{"ingress lossy (accepted)", "mp.v1.ingress.accepted", ""},
		{"unknown", "some.other.subject", ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := natsx.TranslateNewToLegacy(tt.v1)
			if got != tt.want {
				t.Errorf("TranslateNewToLegacy(%q) = %q, want %q", tt.v1, got, tt.want)
			}
		})
	}
}

func TestRoundTripVerevon(t *testing.T) {
	cases := []string{
		"verevon.agent.run.01HXYZ.event",
		"verevon.session.abc-123.command",
	}
	for _, legacy := range cases {
		t.Run(legacy, func(t *testing.T) {
			v1 := natsx.TranslateLegacySubject(legacy)
			back := natsx.TranslateNewToLegacy(v1)
			if back != legacy {
				t.Errorf("round-trip %q -> %q -> %q", legacy, v1, back)
			}
		})
	}
}
