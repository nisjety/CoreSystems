package quarrycontracts

import "time"

// SnapshotWire mirrors quarry_core::resources::Snapshot — the list-view
// shape served by GET /v1/snapshots. quarry-control's cycle24 MountSnapshotsV2
// projects quarry_snapshots_v2 rows into this shape so the edge's Rust
// forward_list::<Snapshot> decoder accepts the page. Field names are pinned to
// the Rust structs' serde output (snake_case); CapturedAt marshals as RFC3339
// via time.Time (serde DateTime<Utc> rejects unix-millis integers).
//
// SourceID is nil for standalone scrapes; PrevFingerprint is nil for the first
// version of a URL; ArtifactID points at the stored capture bytes when one
// exists — all three reproduce #[serde(skip_serializing_if = "Option::is_none")]
// semantics: absent on the wire, never null.
type SnapshotWire struct {
	SnapshotID      string    `json:"snapshot_id"`
	OrgID           string    `json:"org_id"`
	SourceID        *string   `json:"source_id,omitempty"`
	URL             string    `json:"url"`
	Fingerprint     string    `json:"fingerprint"`
	PrevFingerprint *string   `json:"prev_fingerprint,omitempty"`
	ChangeStatus    string    `json:"change_status"` // unchanged | modified | new
	CapturedAt      time.Time `json:"captured_at"`
	ArtifactID      *string   `json:"artifact_id,omitempty"`
}
