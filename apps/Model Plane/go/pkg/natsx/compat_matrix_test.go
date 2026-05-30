// Package natsx — compat_matrix_test.go closes verification gate #4
// ("Compat adapter translates all legacy subjects correctly") and adds the
// mechanism-level proof for PR-6 invariants (dual-write byte equality and
// dual-read equivalence).
//
// Design rules:
//   - Data-driven. Iterate `LegacyMappings` directly so every new entry is
//     auto-exercised without editing the test.
//   - Assert *properties*, not hand-picked strings: "forward translation changed
//     the subject", "result lives in mp.v1.* namespace", "round-trip is lossless
//     when the mapping is reversible".
//   - Keep all tests parallelizable; no shared state, no env vars.

package natsx

import (
	"strings"
	"sync/atomic"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// concreteLegacyFor produces a realistic legacy subject for a given pattern so
// forward translation actually exercises the mapping's NewSubjectFn. Patterns
// with `*` wildcards get a placeholder segment; exact strings pass through.
func concreteLegacyFor(pattern string) string {
	if !strings.Contains(pattern, "*") {
		return pattern
	}
	parts := strings.Split(pattern, ".")
	for i, p := range parts {
		if p == "*" {
			parts[i] = "id-matrix-" + stringDigits(i)
		}
	}
	return strings.Join(parts, ".")
}

func stringDigits(n int) string {
	const digits = "0123456789"
	if n < 0 {
		n = 0
	}
	return string(digits[n%len(digits)])
}

// TestLegacyMappings_EveryEntryTranslates asserts that every declared
// LegacyMapping entry (a) fires its translator for a representative input and
// (b) emits a subject in the mp.v1.* namespace. Additions to `LegacyMappings`
// extend coverage automatically.
func TestLegacyMappings_EveryEntryTranslates(t *testing.T) {
	t.Parallel()
	if len(LegacyMappings) == 0 {
		t.Fatal("LegacyMappings is empty; contract violated")
	}

	for _, m := range LegacyMappings {
		m := m
		t.Run(m.LegacyPattern, func(t *testing.T) {
			t.Parallel()
			legacy := concreteLegacyFor(m.LegacyPattern)

			got := TranslateLegacySubject(legacy)
			if got == legacy {
				t.Fatalf("forward translation did not fire for %q (got identity)", legacy)
			}
			if !strings.HasPrefix(got, "mp.v1.") {
				t.Fatalf("translated subject %q is not in mp.v1.* namespace (legacy=%q)", got, legacy)
			}
		})
	}
}

// TestLegacyMappings_PatternsAreUnique guards against silent shadowing — two
// entries matching the same legacy subject would make behavior depend on
// declaration order, which is a correctness hazard during migration.
func TestLegacyMappings_PatternsAreUnique(t *testing.T) {
	t.Parallel()
	seen := map[string]int{}
	for i, m := range LegacyMappings {
		if prev, dup := seen[m.LegacyPattern]; dup {
			t.Errorf("duplicate LegacyPattern %q at indices %d and %d", m.LegacyPattern, prev, i)
		}
		seen[m.LegacyPattern] = i
	}
}

// TestLegacyMappings_ReversibleSubjectsRoundTrip asserts lossless round-trip
// for subjects whose forward mapping preserves a run_id or session_id. Aqencia
// entries are deliberately skipped — their mapping is lossy (compat shim), and
// that lossiness is itself part of the contract (tested in TestTranslateNewToLegacy).
func TestLegacyMappings_ReversibleSubjectsRoundTrip(t *testing.T) {
	t.Parallel()

	reversible := []string{
		"velion.agent.run.run-reversible-1.event",
		"velion.agent.run.01HXYZABCDEF.event",
		"velion.session.sess-reversible.command",
	}
	for _, legacy := range reversible {
		legacy := legacy
		t.Run(legacy, func(t *testing.T) {
			t.Parallel()
			v1 := TranslateLegacySubject(legacy)
			back := TranslateNewToLegacy(v1)
			if back != legacy {
				t.Fatalf("round-trip broken: %q → %q → %q", legacy, v1, back)
			}
		})
	}
}

// TestLegacyMappings_UnknownSubjectIsIdentity guards the failure-open invariant:
// a subject with no matching pattern must pass through unchanged so operators
// can observe malformed traffic rather than have it silently rewritten.
func TestLegacyMappings_UnknownSubjectIsIdentity(t *testing.T) {
	t.Parallel()
	for _, s := range []string{
		"some.unknown.subject",
		"velion.agent.run",            // incomplete
		"aqencia.other.thing.happened", // outside known aqencia set
		"",
	} {
		if got := TranslateLegacySubject(s); got != s {
			t.Errorf("unknown subject %q was rewritten to %q", s, got)
		}
	}
}

// ---------- PR-6: Dual-write byte-equality ---------------------------------

// TestPR6_DualWriteDeliversIdenticalPayloadToBothSubjects asserts that a single
// publish under ModeDualWrite results in byte-identical payloads delivered to
// independently registered v1 and legacy subscribers.
//
// Closes the mechanism-level half of PR-6. Service-level wiring in
// orchestrator-core is a separate gate (to land alongside runtime handler
// registration).
func TestPR6_DualWriteDeliversIdenticalPayloadToBothSubjects(t *testing.T) {
	t.Parallel()

	v1 := RunEventSubject("pr6-dual-write")
	legacy := LegacyRunEventSubject("pr6-dual-write")

	raw := newMatrixRaw()

	var v1Payload, legacyPayload []byte
	var v1Count, legacyCount int32

	if _, err := raw.Subscribe(v1, func(subject string, data []byte) {
		if subject != v1 {
			t.Errorf("v1 handler got subject %q, want %q", subject, v1)
		}
		v1Payload = append([]byte(nil), data...)
		atomic.AddInt32(&v1Count, 1)
	}); err != nil {
		t.Fatalf("subscribe v1: %v", err)
	}
	if _, err := raw.Subscribe(legacy, func(subject string, data []byte) {
		if subject != legacy {
			t.Errorf("legacy handler got subject %q, want %q", subject, legacy)
		}
		legacyPayload = append([]byte(nil), data...)
		atomic.AddInt32(&legacyCount, 1)
	}); err != nil {
		t.Fatalf("subscribe legacy: %v", err)
	}

	env := makeMatrixEnv(t, "pr6-dual-write-evt")
	pub := NewPublisher(raw, ModeDualWrite)
	if err := pub.Publish(v1, env); err != nil {
		t.Fatalf("publish: %v", err)
	}

	if atomic.LoadInt32(&v1Count) != 1 {
		t.Errorf("v1 handler invocation count = %d, want 1", v1Count)
	}
	if atomic.LoadInt32(&legacyCount) != 1 {
		t.Errorf("legacy handler invocation count = %d, want 1", legacyCount)
	}
	if len(v1Payload) == 0 || len(legacyPayload) == 0 {
		t.Fatalf("missing payloads (v1=%d bytes, legacy=%d bytes)", len(v1Payload), len(legacyPayload))
	}
	if !bytesEqual(v1Payload, legacyPayload) {
		t.Fatalf("dual-write byte inequality:\n  v1=%q\n  legacy=%q", v1Payload, legacyPayload)
	}

	// Raw publishes must also record both subjects.
	published := raw.publishedSubjects()
	if !contains(published, v1) || !contains(published, legacy) {
		t.Fatalf("raw publish list missing subjects: got %v", published)
	}
}

// ---------- PR-6: Dual-read equivalence ------------------------------------

// TestPR6_DualReadEquivalentToLegacyOnlyAndV1Only asserts that for the same
// envelope sent separately on each wire path, a ModeDualRead subscriber sees
// the same handler invocation count (exactly one after dedup) as either a
// ModeV1Only or a ModeLegacyOnly subscriber sees when its chosen wire arm is
// exercised alone. "Equivalent" here means "handler is invoked once with the
// same canonical subject and payload".
//
// The invariant: switching from v1_only → dual_read → legacy_only must not
// cause a consumer to miss or duplicate events (modulo order — ordering is a
// JetStream-level property tested separately).
func TestPR6_DualReadEquivalentToLegacyOnlyAndV1Only(t *testing.T) {
	t.Parallel()

	v1Subject := RunEventSubject("pr6-dual-read")
	legacySubject := LegacyRunEventSubject("pr6-dual-read")
	env := makeMatrixEnv(t, "pr6-dual-read-evt")

	type observation struct {
		subject string
		data    []byte
		count   int32
	}

	// v1_only baseline: envelope arrives on v1 path only.
	v1OnlyRaw := newMatrixRaw()
	v1Obs := &observation{}
	v1Sub := NewSubscriber(v1OnlyRaw, ModeV1Only)
	if _, err := v1Sub.Subscribe(v1Subject, func(subject string, data []byte) error {
		v1Obs.subject = subject
		v1Obs.data = append([]byte(nil), data...)
		atomic.AddInt32(&v1Obs.count, 1)
		return nil
	}); err != nil {
		t.Fatalf("v1_only subscribe: %v", err)
	}
	v1OnlyRaw.mu.Lock()
	if h := v1OnlyRaw.handlers[v1Subject]; h != nil {
		h(v1Subject, env)
	}
	v1OnlyRaw.mu.Unlock()

	// legacy_only baseline: envelope arrives on legacy path only.
	legacyOnlyRaw := newMatrixRaw()
	legacyObs := &observation{}
	legacySub := NewSubscriber(legacyOnlyRaw, ModeLegacyOnly)
	if _, err := legacySub.Subscribe(v1Subject, func(subject string, data []byte) error {
		legacyObs.subject = subject
		legacyObs.data = append([]byte(nil), data...)
		atomic.AddInt32(&legacyObs.count, 1)
		return nil
	}); err != nil {
		t.Fatalf("legacy_only subscribe: %v", err)
	}
	legacyOnlyRaw.mu.Lock()
	if h := legacyOnlyRaw.handlers[legacySubject]; h != nil {
		h(legacySubject, env)
	}
	legacyOnlyRaw.mu.Unlock()

	// dual_read: envelope arrives on BOTH paths; dedup must collapse to one.
	dualRaw := newMatrixRaw()
	dualObs := &observation{}
	dualSub := NewSubscriber(dualRaw, ModeDualRead)
	if _, err := dualSub.Subscribe(v1Subject, func(subject string, data []byte) error {
		dualObs.subject = subject
		dualObs.data = append([]byte(nil), data...)
		atomic.AddInt32(&dualObs.count, 1)
		return nil
	}); err != nil {
		t.Fatalf("dual_read subscribe: %v", err)
	}
	dualRaw.mu.Lock()
	v1Handler := dualRaw.handlers[v1Subject]
	legacyHandler := dualRaw.handlers[legacySubject]
	dualRaw.mu.Unlock()
	if v1Handler == nil || legacyHandler == nil {
		t.Fatalf("dual_read must register both handlers (v1=%v legacy=%v)",
			v1Handler != nil, legacyHandler != nil)
	}
	v1Handler(v1Subject, env)
	legacyHandler(legacySubject, env)

	// Assertions: each baseline saw exactly 1 invocation with canonical subject.
	for _, o := range []struct {
		name string
		obs  *observation
	}{
		{"v1_only", v1Obs},
		{"legacy_only", legacyObs},
		{"dual_read", dualObs},
	} {
		if n := atomic.LoadInt32(&o.obs.count); n != 1 {
			t.Errorf("%s count = %d, want 1", o.name, n)
		}
		if o.obs.subject != v1Subject {
			t.Errorf("%s handler received %q, want canonical %q", o.name, o.obs.subject, v1Subject)
		}
		if !bytesEqual(o.obs.data, env) {
			t.Errorf("%s payload mismatch", o.name)
		}
	}

	// Equivalence: all three observations are identical at the handler contract
	// level (subject, count, payload).
	if v1Obs.count != legacyObs.count || legacyObs.count != dualObs.count {
		t.Fatalf("mode equivalence broken: v1_only=%d legacy_only=%d dual_read=%d",
			v1Obs.count, legacyObs.count, dualObs.count)
	}
}

func bytesEqual(a, b []byte) bool {
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

// silence unused-import warnings if envelope helpers ever get trimmed.
var _ = envelope.Envelope{}
