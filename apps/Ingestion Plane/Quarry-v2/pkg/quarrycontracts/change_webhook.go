package quarrycontracts

import "time"

// ChangeWebhook is the exact body the Quarry-v2 Rust edge producer POSTs to
// POST /v1/webhooks/change (quarry_edge::change_webhook). It is a serialized
// quarry_core::change_history::ChangeRecord with two envelope fields injected
// at the top level: "subject" (always "quarry.change.detected") and
// "emitted_at" (RFC3339 UTC, set by change_webhook_payload). Field names are
// pinned byte-for-byte to the Rust structs' serde renames (snake_case) —
// see change_webhook_test.go for the golden wire-shape test.
//
// The producer signs the request with internal_auth::apply_to_request
// (X-Quarry-Sig / X-Quarry-Sig-TS / X-Quarry-Sig-Nonce over
// "/v1/webhooks/change?org_id=<org>") and stamps an Idempotency-Key header;
// the Go receiver verifies via httpx.HMACVerifier and dedupes on the key.
//
// Zero Data Retention: the producer refuses to emit when the org's session
// is ZDR-flagged (fail-closed upstream). The receiver additionally rejects
// any body that carries a truthy "zdr" key defensively — see the receiver's
// probe in services/quarry-control/internal/resources/change_webhook.go.
type ChangeWebhook struct {
	Subject   string `json:"subject,omitempty"`
	EmittedAt *time.Time `json:"emitted_at,omitempty"`
	ChangeRecord
}

// ChangeStatus itself lives in output.go (ChangeNew/ChangeChanged/
// ChangeUnchanged) — quarry_core uses the SAME snake_case enum for both the
// normalized-output shape and change_history, so this file only adds the
// fourth variant change_history introduces.
const ChangeUnreachable ChangeStatus = "unreachable"

// Valid reports whether s is one of the four Rust enum variants.
func (s ChangeStatus) Valid() bool {
	switch s {
	case ChangeNew, ChangeChanged, ChangeUnchanged, ChangeUnreachable:
		return true
	default:
		return false
	}
}

// SubjectChangeDetected is CHANGE_DETECTED_SUBJECT in
// crates/quarry-edge/src/change_webhook.rs.
const SubjectChangeDetected = "quarry.change.detected"

// ChangeRecord mirrors quarry_core::change_history::ChangeRecord — the
// response body of POST /v1/change/check. Optionals use pointer types with
// omitempty to reproduce #[serde(default, skip_serializing_if)] semantics:
// absent on the wire, never null.
type ChangeRecord struct {
	SourceURL string       `json:"source_url"`
	OrgID     string       `json:"org_id"`
	Status    ChangeStatus `json:"status"`
	// NewBaseline is None when the URL was unreachable on this check.
	NewBaseline *BaselineSnapshot `json:"new_baseline,omitempty"`
	// PrevBaseline is None the first time a URL has been seen.
	PrevBaseline *BaselineSnapshot `json:"prev_baseline,omitempty"`
	// DiffID points into the diff store; present only for status=changed.
	DiffID    *string   `json:"diff_id,omitempty"`
	CheckedAt time.Time `json:"checked_at"`
}

// BaselineSnapshot mirrors quarry_core::change_history::BaselineSnapshot.
// Fingerprint is blake3 of the canonicalized content; equality implies the
// content did not materially change.
type BaselineSnapshot struct {
	BaselineID  string `json:"baseline_id"`
	OrgID       string `json:"org_id"`
	SourceURL   string `json:"source_url"`
	Fingerprint string `json:"fingerprint"`
	// ArtifactID references the ArtifactKind holding the raw bytes
	// (markdown/html); absent for metadata-only snapshots.
	ArtifactID *string `json:"artifact_id,omitempty"`
	// PrevBaselineID forms a singly-linked chain so consumers can walk
	// history without a separate LIST query.
	PrevBaselineID *string `json:"prev_baseline_id,omitempty"`
	CapturedAt     time.Time `json:"captured_at"`
	// RunID is the run that produced this baseline; absent for manual
	// operator imports.
	RunID *string `json:"run_id,omitempty"`
}
