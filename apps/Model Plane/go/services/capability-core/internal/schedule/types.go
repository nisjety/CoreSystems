// Package schedule documents the durable owner for scheduled work —
// cron-driven jobs, remote triggers, and recurring workflows — surfaced as
// a read-only catalog. Actual dispatch and next-fire computation belong to
// downstream scheduler services; capability-core only publishes the
// authoritative catalog.
package schedule

// Kind enumerates the supported scheduled-work trigger shapes.
type Kind string

// Scheduled work kinds.
const (
	KindCron          Kind = "cron"
	KindRemoteTrigger Kind = "remoteTrigger"
	KindRecurring     Kind = "recurring"
)

// ScheduledWork is a durable scheduled unit of work.
type ScheduledWork struct {
	ID             string            `json:"id"`
	IdempotencyKey string            `json:"idempotencyKey"`
	OrgID          string            `json:"orgId"`
	Kind           Kind              `json:"kind"`
	CronExpr       string            `json:"cronExpr,omitempty"`
	NextFireAt     string            `json:"nextFireAt,omitempty"`
	Payload        map[string]string `json:"payload,omitempty"`
	Description    string            `json:"description,omitempty"`
}

// Catalog is the read-only list of scheduled work entries.
type Catalog struct {
	ScheduledWork []ScheduledWork `json:"scheduledWork"`
}

// Load returns the seeded scheduled-work catalog.
func Load() Catalog { return catalog }
