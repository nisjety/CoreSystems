package workflows

import (
	"net/url"
	"sort"
	"strings"

	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
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

// crawlScopeTolerantVersion is the GetVersion id of the tolerant applied-by check (W2-U64).
const crawlScopeTolerantVersion = "crawl-scope-applied-tolerant-v1"

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
	// tolerateUnapplied is set on executions that carry the crawl-scope-applied-tolerant-v1
	// marker (W2-U64). An edge that does not report scope_applied (an edge older than the
	// orchestrator during a rolling deploy) then lets an observe-mode crawl go on as a
	// legacy-unscoped one. Enforce still refuses it.
	tolerateUnapplied bool
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

// scopeVerdict is what a crawl does with the report of one page (W2-U64, D2-073).
type scopeVerdict int

const (
	// verdictApplied: the edge proved it applied the scope; the report is absorbed.
	verdictApplied scopeVerdict = iota
	// verdictLegacyUnscoped: the edge did not report scope_applied (version skew), the crawl
	// is in observe mode, so the page counts as a legacy-unscoped one. Nothing is absorbed.
	verdictLegacyUnscoped
	// verdictRefused: the crawl ends with edge_scope_unsupported (fail closed).
	verdictRefused
)

// verdict decides one page. A report that proves the scope is applied always passes. A
// missing or false scope_applied is the version-skew case: tolerated in observe mode only
// (the scope was never going to change what the page returned), refused in enforce mode
// (links the scope would drop must not be followed), and refused on an execution recorded
// before the tolerant marker, which keeps the strict behaviour it replays with.
func (s *crawlScopeRun) verdict(report *activities.CrawlReport) scopeVerdict {
	if appliedBy(report) {
		return verdictApplied
	}
	if s.tolerateUnapplied && s.base.ScopeMode != activities.ScopeModeEnforce {
		return verdictLegacyUnscoped
	}
	return verdictRefused
}

// The two codes of the robots.txt refusals (WS14.S4), as the edge names them in
// `details.denial.code`.
const (
	robotsDisallowedCode  = "robots_disallowed"
	robotsUnreachableCode = "robots_unreachable"
)

// terminal is the scope part of a terminal event's payload (WS14.S3):
// pages_blocked, pages_blocked_robots, links_denied, hosts_added and denials.
// pages_blocked_robots is the part of pages_blocked the edge refused under
// robots.txt (WS14.S4), so a crawl can say how many pages the robots file kept out.
func (s *crawlScopeRun) terminal(pagesBlocked, pagesBlockedRobots uint32) map[string]any {
	return map[string]any{
		"pages_blocked":        pagesBlocked,
		"pages_blocked_robots": pagesBlockedRobots,
		"links_denied":         s.linksDenied,
		"hosts_added":          sortedKeys(s.hostsAdded),
		"denials":              s.denials.payload(),
	}
}

// robotsRefused reports whether the edge refused a page under robots.txt: a
// `robots_disallowed` or `robots_unreachable` denial. Both are blocklist refusals
// and so count in pages_blocked; a scope or web-policy refusal does not.
func robotsRefused(err error) bool {
	denial, ok := errs.DenialOf(err)
	if !ok {
		return false
	}
	code, _ := denial["code"].(string)
	return code == robotsDisallowedCode || code == robotsUnreachableCode
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
// per-link path uses, so the queue refuses a link it already has. With canonical on
// (W3 E3.7.6) a link goes in under its canonical form, as on the per-link path.
func enqueueChildrenBatch(
	ctx workflow.Context,
	a *activities.Activities,
	queue, orgID, runID string,
	depth uint32,
	links []string,
	canonical bool,
) error {
	items := make([]activities.FrontierBatchItem, 0, len(links))
	for _, link := range links {
		if strings.TrimSpace(link) == "" {
			continue
		}
		requestID, target := frontierTarget(runID, link, canonical)
		items = append(items, activities.FrontierBatchItem{
			RequestID: requestID,
			URL:       target,
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
