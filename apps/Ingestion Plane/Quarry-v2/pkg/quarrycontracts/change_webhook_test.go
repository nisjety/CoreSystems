package quarrycontracts

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// changeWebhookGolden is a payload exactly as the Rust edge serializes it:
// every quarry_core::change_history field name in snake_case, optionals
// ABSENT (skip_serializing_if) rather than null, datetimes RFC3339 UTC.
// It mirrors change_webhook.rs's changed_record() test fixture plus the
// injected subject/emitted_at envelope keys.
const changeWebhookGolden = `{
  "subject": "quarry.change.detected",
  "emitted_at": "2026-08-24T17:00:06Z",
  "source_url": "https://example.com/pricing",
  "org_id": "org_123",
  "status": "changed",
  "new_baseline": {
    "baseline_id": "bln_01J5Z8M4",
    "org_id": "org_123",
    "source_url": "https://example.com/pricing",
    "fingerprint": "blake3:newcontent",
    "artifact_id": "art_01J5Z9QK",
    "prev_baseline_id": "bln_01J4X1AA",
    "captured_at": "2026-08-24T17:00:00Z",
    "run_id": "run_01J5Z7BB"
  },
  "prev_baseline": {
    "baseline_id": "bln_01J4X1AA",
    "org_id": "org_123",
    "source_url": "https://example.com/pricing",
    "fingerprint": "blake3:oldcontent",
    "captured_at": "2026-08-23T16:00:00Z"
  },
  "diff_id": "diff_01J5ZACC",
  "checked_at": "2026-08-24T17:00:05Z"
}`

// mustDecode decodes src into out, failing t on any error.
func mustDecode(t *testing.T, src string, out any) {
	t.Helper()
	dec := json.NewDecoder(strings.NewReader(src))
	if err := dec.Decode(out); err != nil {
		t.Fatalf("decode: %v", err)
	}
}

// canonicalize marshals v and normalizes it through a generic decode so
// comparisons are key-order independent across language serializers.
func canonicalize(t *testing.T, v any) []byte {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var generic any
	if err := json.Unmarshal(raw, &generic); err != nil {
		t.Fatalf("re-decode: %v", err)
	}
	out, err := json.Marshal(generic)
	if err != nil {
		t.Fatalf("re-marshal: %v", err)
	}
	return out
}

// TestChangeWebhook_RoundTripFromRustProducer decodes the Rust producer's
// exact wire shape and asserts every field lands where the Go mirror says
// it must, including pointer-backed Option fields.
func TestChangeWebhook_RoundTripFromRustProducer(t *testing.T) {
	t.Parallel()

	var got ChangeWebhook
	mustDecode(t, changeWebhookGolden, &got)

	if got.Subject != SubjectChangeDetected {
		t.Fatalf("subject=%q want=%q", got.Subject, SubjectChangeDetected)
	}
	wantEmitted := time.Date(2026, 8, 24, 17, 0, 6, 0, time.UTC)
	if got.EmittedAt == nil || !got.EmittedAt.Equal(wantEmitted) {
		t.Fatalf("emitted_at=%v want=%v", got.EmittedAt, wantEmitted)
	}
	if got.SourceURL != "https://example.com/pricing" || got.OrgID != "org_123" {
		t.Fatalf("record header mismatch: %+v", got.ChangeRecord)
	}
	if got.Status != ChangeChanged || !got.Status.Valid() {
		t.Fatalf("status=%q want=changed", got.Status)
	}

	newB := got.NewBaseline
	if newB == nil {
		t.Fatal("new_baseline missing")
	}
	if newB.BaselineID != "bln_01J5Z8M4" || newB.OrgID != "org_123" ||
		newB.SourceURL != "https://example.com/pricing" || newB.Fingerprint != "blake3:newcontent" {
		t.Fatalf("new_baseline scalar fields: %+v", newB)
	}
	if newB.ArtifactID == nil || *newB.ArtifactID != "art_01J5Z9QK" {
		t.Fatalf("new_baseline.artifact_id=%v", newB.ArtifactID)
	}
	if newB.PrevBaselineID == nil || *newB.PrevBaselineID != "bln_01J4X1AA" {
		t.Fatalf("new_baseline.prev_baseline_id=%v", newB.PrevBaselineID)
	}
	if newB.RunID == nil || *newB.RunID != "run_01J5Z7BB" {
		t.Fatalf("new_baseline.run_id=%v", newB.RunID)
	}
	wantCaptured := time.Date(2026, 8, 24, 17, 0, 0, 0, time.UTC)
	if !newB.CapturedAt.Equal(wantCaptured) {
		t.Fatalf("new_baseline.captured_at=%v want=%v", newB.CapturedAt, wantCaptured)
	}

	prevB := got.PrevBaseline
	if prevB == nil {
		t.Fatal("prev_baseline missing")
	}
	// The prev baseline carries NO optional fields on the wire — they must
	// decode as nil pointers, never empty strings that would re-serialize
	// as null.
	if prevB.ArtifactID != nil || prevB.PrevBaselineID != nil || prevB.RunID != nil {
		t.Fatalf("prev_baseline optionals should be nil: %+v", prevB)
	}
	if prevB.Fingerprint != "blake3:oldcontent" {
		t.Fatalf("prev_baseline.fingerprint=%q", prevB.Fingerprint)
	}

	if got.DiffID == nil || *got.DiffID != "diff_01J5ZACC" {
		t.Fatalf("diff_id=%v", got.DiffID)
	}
	wantChecked := time.Date(2026, 8, 24, 17, 0, 5, 0, time.UTC)
	if !got.CheckedAt.Equal(wantChecked) {
		t.Fatalf("checked_at=%v want=%v", got.CheckedAt, wantChecked)
	}

	// Re-serialization must be value-identical to what the producer sent
	// (key-order independent), so control-side consumers that decode the
	// stored payload see the same document the edge produced.
	if want, have := canonicalize(t, mustDecodeGeneric(t, changeWebhookGolden)), canonicalize(t, got); !bytes.Equal(want, have) {
		t.Fatalf("roundtrip mismatch:\nwant=%s\nhave=%s", want, have)
	}
}

// TestChangeWebhook_OptionalsOmitNotNull pins #[serde(skip_serializing_if)]
// parity: a minimal record (unreachable check) serializes WITHOUT nulls —
// Rust deserializers accept missing keys but serde's default deny_null for
// Option-free structs means nulls are hostile to the sibling parser.
func TestChangeWebhook_OptionalsOmitNotNull(t *testing.T) {
	t.Parallel()

	rec := ChangeWebhook{
		Subject: SubjectChangeDetected,
		EmittedAt: func() *time.Time {
			ts := time.Date(2026, 8, 24, 17, 30, 0, 0, time.UTC)
			return &ts
		}(),
		ChangeRecord: ChangeRecord{
			SourceURL: "https://example.com/down",
			OrgID:     "org_123",
			Status:    ChangeUnreachable,
			CheckedAt: time.Date(2026, 8, 24, 17, 29, 59, 0, time.UTC),
		},
	}
	raw, err := json.Marshal(rec)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, banned := range []string{"null"} {
		if strings.Contains(string(raw), banned) {
			t.Fatalf("serialized body contains %q: %s", banned, raw)
		}
	}
	for _, forbiddenKey := range []string{
		"new_baseline", "prev_baseline", "diff_id",
	} {
		if strings.Contains(string(raw), `"`+forbiddenKey+`"`) {
			t.Fatalf("optional key %q leaked into minimal body: %s", forbiddenKey, raw)
		}
	}

	// Embedded ChangeRecord must FLATTEN — no nested "ChangeRecord" key,
	// and the envelope keys sit beside the record keys at top level.
	var flat map[string]json.RawMessage
	if err := json.Unmarshal(raw, &flat); err != nil {
		t.Fatalf("decode: %v", err)
	}
	for _, key := range []string{"subject", "emitted_at", "source_url", "org_id", "status", "checked_at"} {
		if _, ok := flat[key]; !ok {
			t.Fatalf("top-level key %q missing from %s", key, raw)
		}
	}
}

func mustDecodeGeneric(t *testing.T, src string) any {
	t.Helper()
	var v any
	if err := json.Unmarshal([]byte(src), &v); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	return v
}
