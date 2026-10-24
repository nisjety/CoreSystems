package workflows

import (
	"net/url"
	"sort"
	"strings"

	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// The crawl scope (WS14.S3). A crawl whose input carries a non-legacy scope
// sends a `crawl` context with every page, applies the edge's report to its
// frontier and summarises what the scope refused. Everything here is behind the
// `crawl-scope-v1` version marker in CrawlJobWF: a history recorded before the
// marker takes the legacy path and never reaches this code.

// edgeScopeUnsupported ends a crawl whose edge did not apply the scope it was
// sent (an edge older than the orchestrator). The crawl fails closed: no child
// of the page is enqueued.
const edgeScopeUnsupported = "edge_scope_unsupported"

// crawlScopeRun carries one crawl's scope through its loop. It is nil for a
// legacy crawl, and every method is then skipped by the caller.
type crawlScopeRun struct {
	// base is the context every page of the crawl starts from. Depth and
	// seed hosts are set per page by contextFor.
	base activities.CrawlContext
	// seedHosts holds the seed hosts and the hosts the edge added on a depth-0
	// redirect. A later page is scoped to all of them.
	seedHosts   map[string]struct{}
	hostsAdded  map[string]struct{}
	linksDenied uint32
	denials     *denialAgg
}

// newCrawlScopeRun builds the scope of a crawl, or returns nil for a legacy one.
// An unknown scope mode is observe and an unknown robots value is respect: the
// safer reading of a value this build does not know.
func newCrawlScopeRun(in CrawlJobInput) *crawlScopeRun {
	if in.Scope == nil || in.Scope.Legacy {
		return nil
	}
	mode := activities.ScopeModeObserve
	if in.ScopeMode == activities.ScopeModeEnforce {
		mode = activities.ScopeModeEnforce
	}
	robots := activities.RobotsRespect
	if in.Robots == activities.RobotsIgnore {
		robots = activities.RobotsIgnore
	}
	seedHosts := map[string]struct{}{}
	seedURL := ""
	for _, seed := range in.Seeds {
		if strings.TrimSpace(seed) == "" {
			continue
		}
		if seedURL == "" {
			// The first seed is the crawl's seed URL: the edge uses it only when the
			// scope does not crawl the whole domain.
			seedURL = seed
		}
		if host := hostOf(seed); host != "" {
			seedHosts[host] = struct{}{}
		}
	}
	return &crawlScopeRun{
		base: activities.CrawlContext{
			V:         activities.CrawlContractVersion,
			Kind:      activities.CrawlKindCrawl,
			MaxDepth:  in.MaxDepth,
			SeedURL:   seedURL,
			Scope:     *in.Scope,
			ScopeMode: mode,
			Robots:    robots,
		},
		seedHosts:  seedHosts,
		hostsAdded: map[string]struct{}{},
		denials:    newDenialAgg(mode),
	}
}

// contextFor is the `crawl` object for a page at depth. Its seed hosts are the
// hosts known so far, sorted, so the context is the same on every replay.
func (s *crawlScopeRun) contextFor(depth uint32) *activities.CrawlContext {
	c := s.base
	c.Depth = depth
	c.SeedHosts = sortedKeys(s.seedHosts)
	return &c
}

// absorb applies one page's report: the counts, the hosts the edge added to the
// scope and the denial summary.
func (s *crawlScopeRun) absorb(report *activities.CrawlReport) {
	s.linksDenied += report.LinksDenied
	for _, host := range report.HostsAdded {
		if host == "" {
			continue
		}
		s.seedHosts[host] = struct{}{}
		s.hostsAdded[host] = struct{}{}
	}
	s.denials.absorb(report)
}

// appliedBy reports whether the edge's report proves the edge applied the scope.
func appliedBy(report *activities.CrawlReport) bool {
	return report != nil && report.ScopeApplied
}

// terminal is the scope part of a terminal event's payload (WS14.S3):
// pages_blocked, links_denied, hosts_added and denials.
func (s *crawlScopeRun) terminal(pagesBlocked uint32) map[string]any {
	return map[string]any{
		"pages_blocked": pagesBlocked,
		"links_denied":  s.linksDenied,
		"hosts_added":   sortedKeys(s.hostsAdded),
		"denials":       s.denials.payload(),
	}
}

// hostOf is the lower-cased host of a URL, or "" when it has none.
func hostOf(raw string) string {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return ""
	}
	return strings.ToLower(u.Hostname())
}

// sortedKeys returns the keys of a set in sorted order.
func sortedKeys(set map[string]struct{}) []string {
	keys := make([]string, 0, len(set))
	for k := range set {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// enqueueChildrenBatch enqueues the links of one page with a single activity call
// (WS14.S3). A link with a blank URL is skipped. The request id is the one the
// per-link path uses, so the queue refuses a link it already has.
func enqueueChildrenBatch(
	ctx workflow.Context,
	a *activities.Activities,
	queue, orgID, runID string,
	depth uint32,
	links []string,
) error {
	items := make([]activities.FrontierBatchItem, 0, len(links))
	for _, link := range links {
		if strings.TrimSpace(link) == "" {
			continue
		}
		items = append(items, activities.FrontierBatchItem{
			RequestID: durableFrontierRequestID(runID, link),
			URL:       link,
			Depth:     depth,
		})
	}
	if len(items) == 0 {
		return nil
	}
	return workflow.ExecuteActivity(ctx, a.FrontierEnqueueBatch, activities.FrontierEnqueueBatchInput{
		Queue: queue,
		OrgID: orgID,
		Items: items,
	}).Get(ctx, nil)
}
