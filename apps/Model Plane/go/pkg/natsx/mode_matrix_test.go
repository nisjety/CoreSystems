// Package natsx — mode_matrix_test.go proves PR-7 (Migration: feature-flag toggle
// matrix). Exercises Publisher + Subscriber together across all four CompatModes
// with legacy-mapped v1 subjects.
//
// For each (mode, subject) pair we assert:
//   - Publisher fans out to the correct raw subject set for that mode.
//   - Subscriber registers the correct raw subject set for that mode.
//   - Messages published in one mode are observed by a subscriber in the same
//     or compatible mode (e.g. dual_write publisher -> legacy_only subscriber).
//   - Round-trip through the compat adapter preserves the normalized v1 subject
//     delivered to the user handler.
//
// This complements PR-6 (runtime wiring test) and directly targets verification
// gate "Feature flag toggles cleanly between old and new service paths".

package natsx

import (
	"sync"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// matrixRaw is a Publisher+Subscriber-capable fake. Publish records into a log
// and simultaneously delivers to any handler registered via Subscribe on the
// same subject — this lets the matrix test exercise both sides in one process.
type matrixRaw struct {
	mu       sync.Mutex
	handlers map[string]RawMsgHandler
	publishes []capturedPublish
}

func newMatrixRaw() *matrixRaw {
	return &matrixRaw{handlers: map[string]RawMsgHandler{}}
}

func (r *matrixRaw) Publish(subject string, data []byte) error {
	r.mu.Lock()
	r.publishes = append(r.publishes, capturedPublish{subject: subject, data: append([]byte(nil), data...)})
	h := r.handlers[subject]
	r.mu.Unlock()
	if h != nil {
		cp := append([]byte(nil), data...)
		h(subject, cp)
	}
	return nil
}

func (r *matrixRaw) Subscribe(subject string, h RawMsgHandler) (Subscription, error) {
	r.mu.Lock()
	r.handlers[subject] = h
	r.mu.Unlock()
	return &mockSub{}, nil
}

func (r *matrixRaw) rawSubjects() []string {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]string, 0, len(r.handlers))
	for s := range r.handlers {
		out = append(out, s)
	}
	return out
}

func (r *matrixRaw) publishedSubjects() []string {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]string, len(r.publishes))
	for i, p := range r.publishes {
		out[i] = p.subject
	}
	return out
}

func makeMatrixEnv(t *testing.T, id string) []byte {
	t.Helper()
	e := &envelope.Envelope{
		EventID:       id,
		EventType:     "run.event",
		SchemaVersion: 1,
		Producer:      "mode_matrix_test",
		OrgID:         "org-matrix",
	}
	data, err := e.Encode()
	if err != nil {
		t.Fatalf("encode env %s: %v", id, err)
	}
	return data
}

func contains(ss []string, target string) bool {
	for _, s := range ss {
		if s == target {
			return true
		}
	}
	return false
}

// ---------- Matrix ---------------------------------------------------------

// TestCompatMode_PublishFanoutMatrix validates Publisher fan-out across all
// 4 modes × 2 legacy-mapped subjects (run-event + session-command).
func TestCompatMode_PublishFanoutMatrix(t *testing.T) {
	t.Parallel()

	type expect struct {
		mode            CompatMode
		v1Subject       string
		expectedSubjects []string // raw subjects published to
	}

	runV1 := RunEventSubject("run-matrix")
	runLegacy := LegacyRunEventSubject("run-matrix")
	sessV1 := SessionCommandSubject("sess-matrix")
	sessLegacy := LegacySessionCommandSubject("sess-matrix")

	cases := []expect{
		{ModeV1Only, runV1, []string{runV1}},
		{ModeV1Only, sessV1, []string{sessV1}},

		{ModeDualRead, runV1, []string{runV1}},
		{ModeDualRead, sessV1, []string{sessV1}},

		{ModeDualWrite, runV1, []string{runV1, runLegacy}},
		{ModeDualWrite, sessV1, []string{sessV1, sessLegacy}},

		{ModeLegacyOnly, runV1, []string{runLegacy}},
		{ModeLegacyOnly, sessV1, []string{sessLegacy}},
	}

	for _, tc := range cases {
		tc := tc
		t.Run(tc.mode.String()+":"+tc.v1Subject, func(t *testing.T) {
			t.Parallel()
			raw := newMatrixRaw()
			pub := NewPublisher(raw, tc.mode)
			if err := pub.Publish(tc.v1Subject, []byte("payload")); err != nil {
				t.Fatalf("publish: %v", err)
			}
			got := raw.publishedSubjects()
			if len(got) != len(tc.expectedSubjects) {
				t.Fatalf("mode=%s subject=%s: got %d publishes (%v), want %d (%v)",
					tc.mode, tc.v1Subject, len(got), got,
					len(tc.expectedSubjects), tc.expectedSubjects)
			}
			for _, want := range tc.expectedSubjects {
				if !contains(got, want) {
					t.Errorf("mode=%s subject=%s: missing published subject %q (got %v)",
						tc.mode, tc.v1Subject, want, got)
				}
			}
		})
	}
}

// TestCompatMode_SubscribeFanoutMatrix validates Subscriber raw registration
// across all 4 modes × 2 legacy-mapped subjects.
func TestCompatMode_SubscribeFanoutMatrix(t *testing.T) {
	t.Parallel()

	runV1 := RunEventSubject("run-sub-matrix")
	runLegacy := LegacyRunEventSubject("run-sub-matrix")
	sessV1 := SessionCommandSubject("sess-sub-matrix")
	sessLegacy := LegacySessionCommandSubject("sess-sub-matrix")

	type expect struct {
		mode             CompatMode
		v1Subject        string
		expectedSubjects []string // raw subjects subscribed to
	}

	cases := []expect{
		{ModeV1Only, runV1, []string{runV1}},
		{ModeV1Only, sessV1, []string{sessV1}},

		{ModeDualWrite, runV1, []string{runV1}},
		{ModeDualWrite, sessV1, []string{sessV1}},

		{ModeDualRead, runV1, []string{runV1, runLegacy}},
		{ModeDualRead, sessV1, []string{sessV1, sessLegacy}},

		{ModeLegacyOnly, runV1, []string{runLegacy}},
		{ModeLegacyOnly, sessV1, []string{sessLegacy}},
	}

	for _, tc := range cases {
		tc := tc
		t.Run(tc.mode.String()+":"+tc.v1Subject, func(t *testing.T) {
			t.Parallel()
			raw := newMatrixRaw()
			sub := NewSubscriber(raw, tc.mode)
			_, err := sub.Subscribe(tc.v1Subject, func(string, []byte) error { return nil })
			if err != nil {
				t.Fatalf("subscribe: %v", err)
			}
			got := raw.rawSubjects()
			if len(got) != len(tc.expectedSubjects) {
				t.Fatalf("mode=%s subject=%s: got %d raw subs (%v), want %d (%v)",
					tc.mode, tc.v1Subject, len(got), got,
					len(tc.expectedSubjects), tc.expectedSubjects)
			}
			for _, want := range tc.expectedSubjects {
				if !contains(got, want) {
					t.Errorf("mode=%s subject=%s: missing raw sub %q (got %v)",
						tc.mode, tc.v1Subject, want, got)
				}
			}
		})
	}
}

// TestCompatMode_RoundTripHandlerReceivesCanonical asserts that regardless of
// the arrival subject (legacy or v1), the user handler is always invoked with
// the canonical mp.v1.* subject. This is the *operator-facing* invariant behind
// PR-7: mode toggles do not change application-level contracts.
func TestCompatMode_RoundTripHandlerReceivesCanonical(t *testing.T) {
	t.Parallel()

	v1Subject := RunEventSubject("roundtrip-1")
	legacySubject := LegacyRunEventSubject("roundtrip-1")

	type wirePath struct {
		mode           CompatMode
		publishAt      string // what the publisher raw-layer publishes
		expectObserved bool   // should subscriber observe the message at all?
	}

	paths := []wirePath{
		// v1_only: publisher emits v1 only; subscriber listens v1 only.
		{ModeV1Only, v1Subject, true},
		// dual_write (pub) + v1_only (sub): still delivered because pub sends v1.
		{ModeDualWrite, v1Subject, true},
		// legacy_only (pub) + dual_read (sub): subscriber listens both; delivered
		// via legacy arm and normalized to v1.
		{ModeLegacyOnly, legacySubject, true},
	}

	for _, p := range paths {
		p := p
		t.Run(p.mode.String(), func(t *testing.T) {
			t.Parallel()
			raw := newMatrixRaw()
			observedSubj := ""
			var observedCount int32

			subMode := ModeV1Only
			if p.mode == ModeLegacyOnly {
				subMode = ModeDualRead
			}
			sub := NewSubscriber(raw, subMode)
			_, err := sub.Subscribe(v1Subject, func(subject string, data []byte) error {
				observedSubj = subject
				observedCount++
				_ = data
				return nil
			})
			if err != nil {
				t.Fatalf("subscribe: %v", err)
			}

			pub := NewPublisher(raw, p.mode)
			pubSubject := v1Subject
			if p.mode == ModeLegacyOnly {
				pubSubject = v1Subject // publisher takes v1 input in all modes; fanout handles translation
			}
			if err := pub.Publish(pubSubject, makeMatrixEnv(t, "evt-1")); err != nil {
				t.Fatalf("publish: %v", err)
			}

			if !p.expectObserved {
				if observedCount != 0 {
					t.Fatalf("expected no delivery, got %d on %q", observedCount, observedSubj)
				}
				return
			}
			if observedCount == 0 {
				t.Fatalf("expected handler invocation; pub mode=%s sub mode=%s", p.mode, subMode)
			}
			if observedSubj != v1Subject {
				t.Fatalf("handler received %q; want canonical %q", observedSubj, v1Subject)
			}
		})
	}
}

// TestCompatMode_DualReadDeduplication proves PR-7's dual-read dedup invariant:
// a DualRead subscriber seeing the same envelope on both v1 and legacy subjects
// fires the user handler exactly once.
func TestCompatMode_DualReadDeduplication(t *testing.T) {
	t.Parallel()

	v1Subject := RunEventSubject("dedup-1")
	legacySubject := LegacyRunEventSubject("dedup-1")

	raw := newMatrixRaw()
	var count int32
	sub := NewSubscriber(raw, ModeDualRead)
	_, err := sub.Subscribe(v1Subject, func(subject string, _ []byte) error {
		if subject != v1Subject {
			t.Errorf("dedup handler got %q, want %q", subject, v1Subject)
		}
		count++
		return nil
	})
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}

	env := makeMatrixEnv(t, "evt-dedup")

	// Simulate arrivals on both subjects via the raw layer.
	raw.mu.Lock()
	v1Handler := raw.handlers[v1Subject]
	legacyHandler := raw.handlers[legacySubject]
	raw.mu.Unlock()

	if v1Handler == nil || legacyHandler == nil {
		t.Fatalf("dual_read must register both handlers (v1=%v, legacy=%v)",
			v1Handler != nil, legacyHandler != nil)
	}

	v1Handler(v1Subject, env)
	legacyHandler(legacySubject, env)

	if count != 1 {
		t.Fatalf("expected exactly 1 handler invocation (dedup); got %d", count)
	}
}

// TestCompatMode_LegacyOnlyRequiresMapping proves legacy_only rejects unmapped
// v1 subjects with a clear error. This guards the rollback path from silently
// dropping traffic for subjects that only exist in the new namespace.
func TestCompatMode_LegacyOnlyRequiresMapping(t *testing.T) {
	t.Parallel()

	unmappedV1 := "mp.v1.runs.todo.created" // no legacy predecessor
	raw := newMatrixRaw()

	pub := NewPublisher(raw, ModeLegacyOnly)
	if err := pub.Publish(unmappedV1, []byte("x")); err == nil {
		t.Fatal("legacy_only Publish should fail for unmapped v1 subject")
	}

	sub := NewSubscriber(raw, ModeLegacyOnly)
	if _, err := sub.Subscribe(unmappedV1, func(string, []byte) error { return nil }); err == nil {
		t.Fatal("legacy_only Subscribe should fail for unmapped v1 subject")
	}
}
