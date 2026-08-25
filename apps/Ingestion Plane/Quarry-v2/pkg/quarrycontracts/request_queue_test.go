package quarrycontracts

import (
	"encoding/json"
	"testing"
	"time"
)

// The JSON here is exactly what Rust serde produces for
// quarry_core::resources::RequestQueueSummary (snake_case fields, RFC3339
// created_at) — the shape the edge's forward_list decoder emits and
// quarry-control's MountRequestQueuesV2 must produce.
func TestRequestQueueSummaryWireDecodesRustSerdeOutput(t *testing.T) {
	payload := `{
		"queue_id": "q_01ARZ3NDEKTSV4RRFFQ69G5FAV",
		"org_id": "org_demo",
		"name": "primary crawl queue",
		"kind": "crawl",
		"status": "active",
		"created_at": "2026-08-25T12:00:00Z",
		"stats": {
			"queued": 12,
			"in_flight": 3,
			"acked": 100,
			"failed": 1
		}
	}`

	var summary RequestQueueSummaryWire
	if err := json.Unmarshal([]byte(payload), &summary); err != nil {
		t.Fatalf("decode failed: %v", err)
	}

	if summary.QueueID != "q_01ARZ3NDEKTSV4RRFFQ69G5FAV" {
		t.Errorf("queue_id = %q", summary.QueueID)
	}
	if summary.OrgID != "org_demo" || summary.Name != "primary crawl queue" {
		t.Errorf("org_id/name = %q/%q", summary.OrgID, summary.Name)
	}
	if summary.Kind != "crawl" || summary.Status != "active" {
		t.Errorf("kind/status = %q/%q", summary.Kind, summary.Status)
	}
	want := time.Date(2026, 8, 25, 12, 0, 0, 0, time.UTC)
	if !summary.CreatedAt.Equal(want) {
		t.Errorf("created_at = %v, want %v", summary.CreatedAt, want)
	}
	if summary.Stats.Queued != 12 || summary.Stats.InFlight != 3 ||
		summary.Stats.Acked != 100 || summary.Stats.Failed != 1 {
		t.Errorf("stats = %+v", summary.Stats)
	}
}

func TestRequestQueueSummaryWireMarshalsSnakeCaseRFC3339(t *testing.T) {
	wire := RequestQueueSummaryWire{
		QueueID:   "q_01ARZ3NDEKTSV4RRFFQ69G5FAV",
		OrgID:     "org_demo",
		Name:      "primary crawl queue",
		Kind:      "crawl",
		Status:    "active",
		CreatedAt: time.Date(2026, 8, 25, 12, 0, 0, 0, time.UTC),
		Stats:     RequestQueueStatsWire{Queued: 1, InFlight: 2, Acked: 3, Failed: 4},
	}
	encoded, err := json.Marshal(wire)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	want := `{"queue_id":"q_01ARZ3NDEKTSV4RRFFQ69G5FAV","org_id":"org_demo",` +
		`"name":"primary crawl queue","kind":"crawl","status":"active",` +
		`"created_at":"2026-08-25T12:00:00Z",` +
		`"stats":{"queued":1,"in_flight":2,"acked":3,"failed":4}}`
	if string(encoded) != want {
		t.Errorf("wire shape drift:\n got %s\nwant %s", encoded, want)
	}
}
