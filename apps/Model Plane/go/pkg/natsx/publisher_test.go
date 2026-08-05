package natsx

import (
	"errors"
	"testing"
)

type capturedPublish struct {
	subject string
	data    []byte
}

type fakeRaw struct {
	calls   []capturedPublish
	failAt  int // 1-based index; 0 = never fail
	callNum int
}

func (f *fakeRaw) Publish(subject string, data []byte) error {
	f.callNum++
	if f.failAt != 0 && f.callNum == f.failAt {
		return errors.New("boom")
	}
	f.calls = append(f.calls, capturedPublish{subject: subject, data: append([]byte(nil), data...)})
	return nil
}

func TestPublisher_V1Only(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeV1Only)
	if err := p.Publish("mp.v1.run.events.run-abc", []byte("payload")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 1 {
		t.Fatalf("expected 1 call, got %d", len(f.calls))
	}
	if f.calls[0].subject != "mp.v1.run.events.run-abc" {
		t.Errorf("expected v1 subject, got %q", f.calls[0].subject)
	}
}

func TestPublisher_DualRead_PublishesOnlyV1(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeDualRead)
	if err := p.Publish("mp.v1.run.events.run-xyz", []byte("x")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 1 || f.calls[0].subject != "mp.v1.run.events.run-xyz" {
		t.Errorf("dual_read should publish v1 only, got %+v", f.calls)
	}
}

func TestPublisher_DualWrite_WithMapping(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeDualWrite)
	if err := p.Publish("mp.v1.run.run-1.event", []byte("d")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 2 {
		t.Fatalf("expected 2 calls, got %d", len(f.calls))
	}
	if f.calls[0].subject != "mp.v1.run.run-1.event" {
		t.Errorf("first call should be v1, got %q", f.calls[0].subject)
	}
	if f.calls[1].subject != "verevon.agent.run.run-1.event" {
		t.Errorf("second call should be legacy verevon, got %q", f.calls[1].subject)
	}
}

func TestPublisher_DualWrite_NoMapping(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeDualWrite)
	// Subject that has no legacy reverse mapping.
	if err := p.Publish("mp.v1.unmapped.thing", []byte("d")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 1 {
		t.Fatalf("expected 1 call (v1 only, no legacy mapping), got %d", len(f.calls))
	}
	if f.calls[0].subject != "mp.v1.unmapped.thing" {
		t.Errorf("expected v1 subject, got %q", f.calls[0].subject)
	}
}

func TestPublisher_DualWrite_V1FailureShortCircuits(t *testing.T) {
	f := &fakeRaw{failAt: 1}
	p := NewPublisher(f, ModeDualWrite)
	err := p.Publish("mp.v1.run.events.run-2", []byte("d"))
	if err == nil {
		t.Fatal("expected error from v1 publish failure")
	}
	if len(f.calls) != 0 {
		t.Errorf("no calls should be recorded when first publish fails, got %d", len(f.calls))
	}
}

func TestPublisher_LegacyOnly_WithMapping(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeLegacyOnly)
	if err := p.Publish("mp.v1.session.sess-9.command", []byte("d")); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(f.calls) != 1 {
		t.Fatalf("expected 1 call, got %d", len(f.calls))
	}
	if f.calls[0].subject != "verevon.session.sess-9.command" {
		t.Errorf("expected legacy subject, got %q", f.calls[0].subject)
	}
}

func TestPublisher_LegacyOnly_NoMappingErrors(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeLegacyOnly)
	err := p.Publish("mp.v1.unmapped.thing", []byte("d"))
	if err == nil {
		t.Fatal("expected error when no legacy mapping exists in legacy_only mode")
	}
	if len(f.calls) != 0 {
		t.Errorf("no calls should be made when mapping is missing, got %d", len(f.calls))
	}
}

func TestPublisher_Mode(t *testing.T) {
	p := NewPublisher(&fakeRaw{}, ModeDualWrite)
	if p.Mode() != ModeDualWrite {
		t.Errorf("Mode() = %v, want ModeDualWrite", p.Mode())
	}
}

// ── Zero Data Retention suppression ─────────────────────────────────────────

// TestPublisher_ZDREnvelopeNeverReachesAnyBackend is the Go twin of
// model-gateway's Rust `zdr_envelopes_never_enter_any_publisher_backend`. It runs
// across EVERY compat mode because dual-write would otherwise mirror a
// no-retention envelope onto the legacy subject even if the v1 leg were guarded.
func TestPublisher_ZDREnvelopeNeverReachesAnyBackend(t *testing.T) {
	const zdrEnvelope = `{"event_id":"e","event_type":"RUN_COMPLETED","org_id":"o",` +
		`"payload":{"summary":"must-not-persist"},"zdr":true}`

	for _, mode := range []CompatMode{ModeV1Only, ModeDualRead, ModeDualWrite, ModeLegacyOnly} {
		t.Run(mode.String(), func(t *testing.T) {
			f := &fakeRaw{}
			p := NewPublisher(f, mode)
			// A suppressed publish is a success, not an error: the Rust twin
			// returns Ok(()), and failing it would turn a satisfied retention
			// rule into a producer-side retry storm.
			if err := p.Publish("mp.v1.run.run-1.event", []byte(zdrEnvelope)); err != nil {
				t.Fatalf("suppressed publish must not error: %v", err)
			}
			if f.callNum != 0 {
				t.Fatalf("backend was called %d time(s); a ZDR envelope must reach no backend", f.callNum)
			}
			if len(f.calls) != 0 {
				t.Fatalf("expected 0 published messages, got %d", len(f.calls))
			}
		})
	}
}

// TestPublisher_ZDRSuppressionIsExactlyZDRTrue guards the blast radius: only an
// explicit `zdr: true` is suppressed. Dropping envelopes with an ABSENT posture
// would silently delete lifecycle signal that downstream run counters depend on,
// and non-envelope payloads must pass through untouched.
func TestPublisher_ZDRSuppressionIsExactlyZDRTrue(t *testing.T) {
	for _, tc := range []struct {
		name        string
		data        string
		wantPublish bool
	}{
		{name: "zdr true suppressed", data: `{"zdr":true}`, wantPublish: false},
		{name: "zdr false published", data: `{"zdr":false}`, wantPublish: true},
		{name: "posture absent published", data: `{"event_id":"e"}`, wantPublish: true},
		{name: "zdr null published", data: `{"zdr":null}`, wantPublish: true},
		{name: "non-json payload published", data: `not-an-envelope`, wantPublish: true},
		{name: "empty payload published", data: ``, wantPublish: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f := &fakeRaw{}
			p := NewPublisher(f, ModeV1Only)
			if err := p.Publish("mp.v1.run.run-1.event", []byte(tc.data)); err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			got := len(f.calls) == 1
			if got != tc.wantPublish {
				t.Fatalf("published = %v, want %v (calls: %d)", got, tc.wantPublish, len(f.calls))
			}
		})
	}
}
