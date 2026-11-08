package workflows

import "github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"

// The batch check (WS14.S5). The edge checks every URL of a batch before the job
// exists (no network work there). The workflow then sends a `crawl` context of kind
// `batch` with each page, so the edge can apply the robots mode, and it counts a
// page the edge refused as blocked rather than failed. All of it is behind the
// `batch-check-v1` version marker in BatchJobWF: a history recorded before the
// marker runs as it did.

// batchCheckVersion is the GetVersion id of the batch check. Its id is kept in the
// order the markers were introduced (H19 of the W2 queue).
const batchCheckVersion = "batch-check-v1"

// batchRobots is the robots mode a batch runs with. The edge stamps respect; an
// absent or unknown value is respect too (the safer reading). Ignoring robots is
// not reachable from a batch until WS14.S7.
func batchRobots(raw string) string {
	if raw == activities.RobotsIgnore {
		return activities.RobotsIgnore
	}
	return activities.RobotsRespect
}

// batchCrawlContext is the `crawl` object of a batch page. A batch never follows
// links, so its scope is empty and the edge evaluates no link (crawl_scope in
// quarry-runtime returns no links for kind batch).
func batchCrawlContext(robots string) *activities.CrawlContext {
	return &activities.CrawlContext{
		V:         activities.CrawlContractVersion,
		Kind:      activities.CrawlKindBatch,
		Depth:     0,
		MaxDepth:  0,
		Scope:     activities.CrawlScope{},
		ScopeMode: activities.ScopeModeObserve,
		Robots:    batchRobots(robots),
	}
}
