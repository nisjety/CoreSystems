package natsx

import (
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// --- mock RawSubscriber ---

type mockSub struct{ unsubscribed bool }

func (m *mockSub) Unsubscribe() error { m.unsubscribed = true; return nil }

type mockRawSub struct {
	mu   sync.Mutex
	subs map[string]RawMsgHandler
	errs map[string]error
}

func newMockRawSub() *mockRawSub {
	return &mockRawSub{subs: map[string]RawMsgHandler{}, errs: map[string]error{}}
}

func (r *mockRawSub) Subscribe(subject string, h RawMsgHandler) (Subscription, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err, ok := r.errs[subject]; ok {
		return nil, err
	}
	r.subs[subject] = h
	return &mockSub{}, nil
}

func (r *mockRawSub) deliver(subject string, data []byte) {
	r.mu.Lock()
	h := r.subs[subject]
	r.mu.Unlock()
	if h == nil {
		return
	}
	cp := make([]byte, len(data))
	copy(cp, data)
	h(subject, cp)
}

func (r *mockRawSub) subjects() []string {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]string, 0, len(r.subs))
	for s := range r.subs {
		out = append(out, s)
	}
	return out
}

// --- fixtures ---

func makeEnv(t *testing.T, id string) []byte {
	t.Helper()
	e := &envelope.Envelope{
		EventID:       id,
		EventType:     "run.event",
		SchemaVersion: 1,
		Producer:      "test",
		OrgID:         "org-1",
	}
	data, err := e.Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	return data
}

// --- tests ---

func TestSubscriber_V1Only_SubscribesV1Only(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeV1Only)
	subj := RunEventSubject("abc")
	subs, err := s.Subscribe(subj, func(string, []byte) error { return nil })
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if len(subs) != 1 {
		t.Fatalf("want 1 sub, got %d", len(subs))
	}
	got := raw.subjects()
	if len(got) != 1 || got[0] != subj {
		t.Fatalf("want [%s], got %v", subj, got)
	}
}

func TestSubscriber_DualWrite_SubscribesV1Only(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualWrite)
	subj := RunEventSubject("abc")
	subs, err := s.Subscribe(subj, func(string, []byte) error { return nil })
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if len(subs) != 1 {
		t.Fatalf("want 1 sub, got %d", len(subs))
	}
	got := raw.subjects()
	if len(got) != 1 || got[0] != subj {
		t.Fatalf("want [%s], got %v", subj, got)
	}
}

func TestSubscriber_DualRead_SubscribesBoth(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualRead)
	v1 := RunEventSubject("abc")
	legacy := LegacyRunEventSubject("abc")
	subs, err := s.Subscribe(v1, func(string, []byte) error { return nil })
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if len(subs) != 2 {
		t.Fatalf("want 2 subs, got %d", len(subs))
	}
	got := raw.subjects()
	if len(got) != 2 {
		t.Fatalf("want 2 subjects, got %v", got)
	}
	foundV1, foundLegacy := false, false
	for _, g := range got {
		if g == v1 {
			foundV1 = true
		}
		if g == legacy {
			foundLegacy = true
		}
	}
	if !foundV1 || !foundLegacy {
		t.Fatalf("want both %s and %s, got %v", v1, legacy, got)
	}
}

func TestSubscriber_DualRead_DedupByEventID(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualRead)
	v1 := RunEventSubject("abc")
	legacy := LegacyRunEventSubject("abc")
	var calls int32
	_, err := s.Subscribe(v1, func(string, []byte) error {
		atomic.AddInt32(&calls, 1)
		return nil
	})
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	data := makeEnv(t, "evt-1")
	raw.deliver(v1, data)
	raw.deliver(legacy, data)
	if got := atomic.LoadInt32(&calls); got != 1 {
		t.Fatalf("dedup: want 1 call, got %d", got)
	}
}

func TestSubscriber_DualRead_NormalizesLegacySubjectToV1(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualRead)
	v1 := RunEventSubject("abc")
	legacy := LegacyRunEventSubject("abc")
	var seen string
	var mu sync.Mutex
	_, err := s.Subscribe(v1, func(subject string, _ []byte) error {
		mu.Lock()
		seen = subject
		mu.Unlock()
		return nil
	})
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	raw.deliver(legacy, makeEnv(t, "evt-1"))
	mu.Lock()
	got := seen
	mu.Unlock()
	if got != v1 {
		t.Fatalf("want normalized %s, got %s", v1, got)
	}
}

func TestSubscriber_DualRead_SkipsLegacyWhenNoReverseMapping(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualRead)
	subj := IngressSubject("accepted")
	subs, err := s.Subscribe(subj, func(string, []byte) error { return nil })
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if len(subs) != 1 {
		t.Fatalf("want 1 sub (no reverse mapping), got %d", len(subs))
	}
	got := raw.subjects()
	if len(got) != 1 || got[0] != subj {
		t.Fatalf("want [%s], got %v", subj, got)
	}
}

func TestSubscriber_DualRead_FailOpenOnBadJSON(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeDualRead)
	v1 := RunEventSubject("abc")
	var calls int32
	_, err := s.Subscribe(v1, func(string, []byte) error {
		atomic.AddInt32(&calls, 1)
		return nil
	})
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	raw.deliver(v1, []byte("not json"))
	if got := atomic.LoadInt32(&calls); got != 1 {
		t.Fatalf("fail-open: want 1 call, got %d", got)
	}
}

func TestSubscriber_LegacyOnly_WithMapping(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeLegacyOnly)
	v1 := RunEventSubject("abc")
	legacy := LegacyRunEventSubject("abc")
	subs, err := s.Subscribe(v1, func(string, []byte) error { return nil })
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if len(subs) != 1 {
		t.Fatalf("want 1 sub, got %d", len(subs))
	}
	got := raw.subjects()
	if len(got) != 1 || got[0] != legacy {
		t.Fatalf("want [%s], got %v", legacy, got)
	}
}

func TestSubscriber_LegacyOnly_NoMapping_ReturnsError(t *testing.T) {
	raw := newMockRawSub()
	s := NewSubscriber(raw, ModeLegacyOnly)
	subj := IngressSubject("accepted")
	_, err := s.Subscribe(subj, func(string, []byte) error { return nil })
	if err == nil {
		t.Fatalf("want error, got nil")
	}
	if !strings.Contains(err.Error(), "natsx:") || !strings.Contains(err.Error(), "no legacy mapping") {
		t.Fatalf("want error containing natsx: and no legacy mapping, got %v", err)
	}
}
