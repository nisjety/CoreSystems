// Package compat — e2e_test.go closes PR-6 at the orchestrator-core service
// layer. Proves dual-write consistency and dual-read equivalence end-to-end
// through the same wiring main.go uses in production: a natsx.Publisher plus a
// compat.Subscriber handler registered against legacy wildcard subjects.
//
// A fake in-process raw bus satisfies both natsx.RawPublisher and
// natsx.RawSubscriber so the test exercises the full translation chain without
// needing a real NATS broker.
//
// Gates closed by this file:
//   - "Dual-write consistency verified where adapters are active" (PR-6 service)
//   - "Dual-read returns identical results from old and new paths" (PR-6 service)
//   - Self-loop protection in ModeDualWrite (regression guard)

package compat

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
)

// matchNATSSubject implements NATS subject matching semantics: `*` matches
// exactly one token; `>` matches one or more trailing tokens. Used by fakeBus
// so wildcard subscriptions behave like a real broker.
func matchNATSSubject(pattern, subject string) bool {
	if pattern == subject {
		return true
	}
	pt := strings.Split(pattern, ".")
	st := strings.Split(subject, ".")
	for i, p := range pt {
		if p == ">" {
			return i <= len(st) // `>` matches remainder (zero or more)
		}
		if i >= len(st) {
			return false
		}
		if p == "*" {
			continue
		}
		if p != st[i] {
			return false
		}
	}
	return len(pt) == len(st)
}

// fakeBus is an in-process raw bus that implements both RawPublisher and
// RawSubscriber. Publishes are recorded and also fan out to any registered
// handler on the matching subject so the test can observe consumer behaviour.
type fakeBus struct {
	mu        sync.Mutex
	handlers  map[string]natsx.RawMsgHandler
	published []busMsg
}

type busMsg struct {
	subject string
	data    []byte
}

func newFakeBus() *fakeBus {
	return &fakeBus{handlers: map[string]natsx.RawMsgHandler{}}
}

func (b *fakeBus) Publish(subject string, data []byte) error {
	b.mu.Lock()
	b.published = append(b.published, busMsg{subject: subject, data: append([]byte(nil), data...)})
	// Deliver to every handler whose registered pattern matches the subject.
	type match struct {
		pattern string
		h       natsx.RawMsgHandler
	}
	var matches []match
	for pattern, h := range b.handlers {
		if matchNATSSubject(pattern, subject) {
			matches = append(matches, match{pattern, h})
		}
	}
	b.mu.Unlock()
	for _, m := range matches {
		m.h(subject, append([]byte(nil), data...))
	}
	return nil
}

// fakeSub is the minimal Subscription value returned by Subscribe.
type fakeSub struct{}

func (fakeSub) Unsubscribe() error { return nil }

func (b *fakeBus) Subscribe(subject string, h natsx.RawMsgHandler) (natsx.Subscription, error) {
	b.mu.Lock()
	b.handlers[subject] = h
	b.mu.Unlock()
	return fakeSub{}, nil
}

func (b *fakeBus) publishes() []busMsg {
	b.mu.Lock()
	defer b.mu.Unlock()
	out := make([]busMsg, len(b.published))
	copy(out, b.published)
	return out
}

func (b *fakeBus) publishesFor(subject string) []busMsg {
	out := []busMsg{}
	for _, m := range b.publishes() {
		if m.subject == subject {
			out = append(out, m)
		}
	}
	return out
}

// wireService mirrors the production wiring in cmd/main.go §compat adapter:
// given a mode, it returns a Publisher wired to the bus and installs a
// compat.Subscriber handler on legacy wildcard subjects. Returned *Subscriber
// is exposed so tests can assert on its behaviour if needed.
func wireService(t *testing.T, mode natsx.CompatMode) (*fakeBus, *natsx.Publisher, *Subscriber) {
	t.Helper()
	bus := newFakeBus()
	pub := natsx.NewPublisher(bus, mode)
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	compatSub := NewSubscriber(pub, logger)

	handler := func(subject string, data []byte) {
		if err := compatSub.HandleLegacyMessage(context.Background(), subject, data); err != nil {
			t.Logf("compat handler error: %v", err)
		}
	}

	for _, subj := range []string{
		natsx.LegacyRunEventsWildcard,
		natsx.LegacySessionCommandWildcard,
		natsx.LegacyAqenciaWildcard,
	} {
		if _, err := bus.Subscribe(subj, handler); err != nil {
			t.Fatalf("bus.Subscribe(%q): %v", subj, err)
		}
	}
	return bus, pub, compatSub
}

// -------------------------------------------------------------------------
// PR-6: dual-write consistency through service wiring
// -------------------------------------------------------------------------

// TestServiceE2E_DualWriteConsistency proves that, under ModeDualWrite, a
// service-level publish to a v1 subject results in both a v1 and a legacy copy
// on the bus, AND the self-loop guard prevents the compat adapter from
// re-translating the legacy mirror of the v1 publisher's own output.
//
// Without the self-loop guard this test would hang or blow the stack.
func TestServiceE2E_DualWriteConsistency(t *testing.T) {
	t.Parallel()
	bus, pub, _ := wireService(t, natsx.ModeDualWrite)

	// Direct subscriber to observe whatever lands on the v1 run-events subject.
	v1Subject := natsx.RunEventSubject("run-pr6-dw")
	var v1Received int32
	if _, err := bus.Subscribe(v1Subject, func(_ string, _ []byte) {
		atomic.AddInt32(&v1Received, 1)
	}); err != nil {
		t.Fatalf("subscribe v1: %v", err)
	}

	// Producer publishes a v1 envelope — the realistic business path.
	env := &envelope.Envelope{
		EventID:       "evt-e2e-dw",
		EventType:     "run.event",
		SchemaVersion: 1,
		Producer:      "execution-core", // NOT compat-adapter: this is real business traffic.
		OrgID:         "org-dw",
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if err := pub.Publish(v1Subject, data); err != nil {
		t.Fatalf("publish: %v", err)
	}

	// Dual-write: bus must have recorded one v1 publish and one legacy mirror publish.
	legacySubject := natsx.LegacyRunEventSubject("run-pr6-dw")
	if got := len(bus.publishesFor(v1Subject)); got == 0 {
		t.Fatalf("no v1 publish recorded on %q", v1Subject)
	}
	if got := len(bus.publishesFor(legacySubject)); got == 0 {
		t.Fatalf("no legacy mirror publish recorded on %q", legacySubject)
	}

	// Consumer on v1 saw exactly one delivery (not amplified by compat adapter
	// re-translating the legacy mirror — that's the self-loop guard working).
	if count := atomic.LoadInt32(&v1Received); count != 1 {
		t.Fatalf("v1 consumer count = %d, want 1 (self-loop guard may be broken)", count)
	}

	// Byte equality across v1 + legacy publishes.
	v1Msgs := bus.publishesFor(v1Subject)
	legacyMsgs := bus.publishesFor(legacySubject)
	if len(v1Msgs) != len(legacyMsgs) || len(v1Msgs) != 1 {
		t.Fatalf("expected 1 v1 + 1 legacy publish, got %d + %d", len(v1Msgs), len(legacyMsgs))
	}
	if !bytesEqualE2E(v1Msgs[0].data, legacyMsgs[0].data) {
		t.Fatalf("dual-write byte inequality between v1 and legacy mirrors")
	}
}

// TestServiceE2E_DualWriteLegacyArrivalStillTranslated proves that when a
// genuine legacy-only producer (not a compat mirror) emits a message, the
// compat subscriber still translates and republishes it to the v1 namespace
// even while the publisher is in ModeDualWrite. This is the consumer-side
// migration story: downstream v1 consumers see everything.
func TestServiceE2E_DualWriteLegacyArrivalStillTranslated(t *testing.T) {
	t.Parallel()
	bus, _, _ := wireService(t, natsx.ModeDualWrite)

	v1Subject := natsx.RunEventSubject("run-legacy-arrival")
	legacySubject := natsx.LegacyRunEventSubject("run-legacy-arrival")

	// v1 consumer
	var v1Delivered []byte
	var v1Count int32
	if _, err := bus.Subscribe(v1Subject, func(_ string, data []byte) {
		v1Delivered = append([]byte(nil), data...)
		atomic.AddInt32(&v1Count, 1)
	}); err != nil {
		t.Fatalf("subscribe v1: %v", err)
	}

	// Legacy-only producer publishes directly on the legacy subject.
	legacyPayload, _ := json.Marshal(map[string]any{
		"event_id":       "evt-legacy-orig",
		"event_type":     "RUN_STARTED",
		"correlation_id": "corr-legacy",
		"org_id":         "org-legacy",
		"run_id":         "run-legacy-arrival",
		// Note: no producer field → compat subscriber treats it as external.
	})
	if err := bus.Publish(legacySubject, legacyPayload); err != nil {
		t.Fatalf("legacy publish: %v", err)
	}

	// compat subscriber should have fired, translating + republishing on v1.
	if count := atomic.LoadInt32(&v1Count); count != 1 {
		t.Fatalf("v1 delivery count = %d, want 1 (legacy→v1 bridge broken)", count)
	}
	var env envelope.Envelope
	if err := json.Unmarshal(v1Delivered, &env); err != nil {
		t.Fatalf("decode v1 envelope: %v", err)
	}
	if env.Producer != producerCompatAdapter {
		t.Errorf("v1 envelope Producer = %q, want %q (compat should tag own output)",
			env.Producer, producerCompatAdapter)
	}
	if env.EventID != "evt-legacy-orig" {
		t.Errorf("v1 envelope EventID = %q, want evt-legacy-orig (lineage preservation)",
			env.EventID)
	}
}

// -------------------------------------------------------------------------
// PR-6: dual-read equivalence through service wiring
// -------------------------------------------------------------------------

// TestServiceE2E_DualReadEquivalence proves that under ModeDualRead, a v1
// consumer sees the same logical event regardless of whether the original
// producer emitted on the v1 subject or on the legacy subject — and exactly
// once each (no duplicates from translation + direct delivery both landing on
// the same v1 listener).
//
// Setup:
//   - orchestrator wired in ModeDualRead (publishers emit v1 only; subscribers
//     read both arms and dedup).
//   - compat subscriber installed on legacy wildcards (same as main.go).
//   - one v1-subject consumer registered on the bus.
//
// Actions:
//   - Producer A emits a v1 envelope directly on the v1 subject.
//   - Producer B emits a legacy envelope on the legacy subject (simulating an
//     unmigrated emitter).
//
// Assertion: the v1 consumer sees each of the two events exactly once (total
// count = 2, EventIDs distinct), with correct canonical subjects.
func TestServiceE2E_DualReadEquivalence(t *testing.T) {
	t.Parallel()
	bus, pub, _ := wireService(t, natsx.ModeDualRead)

	v1Subject := natsx.RunEventSubject("run-pr6-dr")
	legacySubject := natsx.LegacyRunEventSubject("run-pr6-dr")

	type delivery struct {
		subject string
		eventID string
	}
	var (
		mu         sync.Mutex
		deliveries []delivery
	)
	if _, err := bus.Subscribe(v1Subject, func(subject string, data []byte) {
		var env envelope.Envelope
		if err := json.Unmarshal(data, &env); err != nil {
			t.Errorf("v1 subscriber decode: %v", err)
			return
		}
		mu.Lock()
		deliveries = append(deliveries, delivery{subject: subject, eventID: env.EventID})
		mu.Unlock()
	}); err != nil {
		t.Fatalf("subscribe v1: %v", err)
	}

	// Producer A: native v1 producer.
	envA := &envelope.Envelope{
		EventID:       "evt-native-v1",
		EventType:     "run.event",
		SchemaVersion: 1,
		Producer:      "execution-core",
		OrgID:         "org-dr",
	}
	dataA, _ := envA.Encode()
	if err := pub.Publish(v1Subject, dataA); err != nil {
		t.Fatalf("publish v1: %v", err)
	}

	// Producer B: legacy-only producer emits on legacy arm.
	legacyPayload, _ := json.Marshal(map[string]any{
		"event_id":       "evt-legacy-only",
		"event_type":     "RUN_STARTED",
		"correlation_id": "corr-dr",
		"org_id":         "org-dr",
		"run_id":         "run-pr6-dr",
	})
	if err := bus.Publish(legacySubject, legacyPayload); err != nil {
		t.Fatalf("legacy publish: %v", err)
	}

	mu.Lock()
	defer mu.Unlock()
	if len(deliveries) != 2 {
		t.Fatalf("v1 consumer saw %d deliveries, want 2 (got %+v)", len(deliveries), deliveries)
	}

	seenIDs := map[string]bool{}
	for _, d := range deliveries {
		if d.subject != v1Subject {
			t.Errorf("delivery subject = %q, want canonical %q", d.subject, v1Subject)
		}
		if seenIDs[d.eventID] {
			t.Errorf("duplicate EventID %q in deliveries (dedup failed)", d.eventID)
		}
		seenIDs[d.eventID] = true
	}
	if !seenIDs["evt-native-v1"] {
		t.Error("missing delivery for evt-native-v1 (v1 direct path broken)")
	}
	if !seenIDs["evt-legacy-only"] {
		t.Error("missing delivery for evt-legacy-only (legacy→v1 compat path broken)")
	}
}

// TestServiceE2E_SelfLoopGuard proves the loop protection directly: if the
// compat subscriber receives a payload whose `producer` field is already
// "compat-adapter", it must drop the message (no republish). This is the
// invariant that makes ModeDualWrite safe.
func TestServiceE2E_SelfLoopGuard(t *testing.T) {
	t.Parallel()
	bus, _, _ := wireService(t, natsx.ModeDualWrite)

	legacySubject := natsx.LegacyRunEventSubject("run-loop")
	selfMirror, _ := json.Marshal(map[string]any{
		"event_id":  "evt-mirror",
		"producer":  producerCompatAdapter,
		"org_id":    "org-loop",
		"run_id":    "run-loop",
	})

	startCount := len(bus.publishes())
	if err := bus.Publish(legacySubject, selfMirror); err != nil {
		t.Fatalf("legacy publish: %v", err)
	}

	// One publish recorded (the direct legacy push); compat subscriber must not
	// have produced any additional v1 publishes for this mirrored message.
	after := bus.publishes()
	newlyAdded := 0
	for _, m := range after[startCount:] {
		if m.subject != legacySubject {
			newlyAdded++
		}
	}
	if newlyAdded != 0 {
		t.Fatalf("self-loop guard failed: compat subscriber produced %d extra publishes", newlyAdded)
	}
}

func bytesEqualE2E(a, b []byte) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
