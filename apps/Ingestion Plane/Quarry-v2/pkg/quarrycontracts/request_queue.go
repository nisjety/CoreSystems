package quarrycontracts

import "time"

// RequestQueueSummaryWire mirrors quarry_core::resources::RequestQueueSummary
// (crates/quarry-core/src/resources.rs) — the list-view shape served by
// GET /v1/request-queues. quarry-control's cycle24 MountRequestQueuesV2
// projects its store rows into this type so the edge's Rust
// forward_list::<RequestQueueSummary> decoder accepts the page. Field names
// are pinned to the Rust structs' serde output (snake_case); CreatedAt
// marshals as RFC3339 via time.Time, matching chrono DateTime<Utc>.
//
// Stats counters are the four frontier numbers the Rust RequestQueueStats
// struct requires on every decode: acked/failed come from dedicated columns,
// queued/in_flight are derived from frontier tables at read time.
type RequestQueueSummaryWire struct {
	QueueID   string                `json:"queue_id"`
	OrgID     string                `json:"org_id"`
	Name      string                `json:"name"`
	Kind      string                `json:"kind"`   // crawl | batch | scrape | search (informational)
	Status    string                `json:"status"` // active | draining | deleted
	CreatedAt time.Time             `json:"created_at"`
	Stats     RequestQueueStatsWire `json:"stats"`
}

// RequestQueueStatsWire mirrors quarry_core::resources::RequestQueueStats.
type RequestQueueStatsWire struct {
	Queued   uint64 `json:"queued"`
	InFlight uint64 `json:"in_flight"`
	Acked    uint64 `json:"acked"`
	Failed   uint64 `json:"failed"`
}
