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
	failAt  int  // 1-based index; 0 = never fail
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
	if f.calls[1].subject != "velion.agent.run.run-1.event" {
		t.Errorf("second call should be legacy velion, got %q", f.calls[1].subject)
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
	if f.calls[0].subject != "velion.session.sess-9.command" {
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
