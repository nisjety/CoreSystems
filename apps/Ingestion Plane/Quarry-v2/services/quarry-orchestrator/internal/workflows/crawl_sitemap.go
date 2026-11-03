package workflows

import (
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// Sitemap seeding and per-page scrape options of a crawl (INNH-6).
//
// A crawl whose job params carry `sitemap: include|only` asks the edge for the
// sitemap's URLs once, before its loop, through the SitemapSeeds activity. The
// call sits behind the `crawl-sitemap-seeds-v1` version marker, and a crawl
// without the param never reaches it, so a history recorded before this version
// replays unchanged.
//
// A crawl whose params carry `scrape_options` sends them with every page. The edge
// echoes `scrape_applied` when it honoured them; a page without the echo ends the
// run (an edge that ignores the field would crawl with the default cleaning).

const (
	// sitemapInclude seeds the sitemap's URLs as well as following links.
	sitemapInclude = "include"
	// sitemapOnly crawls the seed and the sitemap's URLs and follows no page link.
	sitemapOnly = "only"

	// sitemapSeedDepth is the depth sitemap URLs are queued at: one below the seed, so a
	// redirect off the host cannot add a host to the crawl's scope (that is a depth-0
	// privilege) and the depth bound still counts the links of a sitemap page.
	sitemapSeedDepth uint32 = 1

	// sitemapInMemoryCap bounds the sitemap URLs the in-memory frontier takes: it keeps
	// them in workflow state. The durable frontier enqueues from the activity and has no cap
	// beyond the crawl's page bound.
	sitemapInMemoryCap uint32 = 500

	// sitemapUnavailable is the terminal code of a `sitemap: only` crawl whose discovery failed.
	sitemapUnavailable = "sitemap_unavailable"

	// edgeScrapeUnsupported ends a crawl whose edge did not apply the scrape options it was sent.
	edgeScrapeUnsupported = "edge_scrape_options_unsupported"
)

// sitemapRun is one crawl's sitemap discovery.
type sitemapRun struct {
	mode   string
	result *activities.SitemapSeedsResult
	// failure is the error text when the discovery did not complete. Empty on success.
	failure string
}

// followLinks reports whether the pages' links are followed: always, except for `sitemap: only`.
func (s *sitemapRun) followLinks() bool {
	return s == nil || s.mode != sitemapOnly
}

// failedHard reports a `sitemap: only` crawl whose discovery failed: with nothing to crawl but
// the seed it ends instead of reporting a success it did not earn. An `include` crawl goes on.
func (s *sitemapRun) failedHard() bool {
	return s != nil && s.mode == sitemapOnly && s.failure != ""
}

// payload is the `sitemap` object of the terminal event. Counts only.
func (s *sitemapRun) payload() map[string]any {
	out := map[string]any{"mode": s.mode}
	if s.failure != "" {
		out["error"] = s.failure
		return out
	}
	r := s.result
	out["discovered"] = r.Discovered
	out["sitemaps_fetched"] = r.SitemapsFetched
	out["nested_fetched"] = r.NestedFetched
	out["seeded"] = r.Seeded
	out["skipped"] = map[string]any{
		"out_of_scope":         r.Skipped.OutOfScope,
		"robots_disallowed":    r.Skipped.RobotsDisallowed,
		"duplicate":            r.Skipped.Duplicate,
		"cap":                  r.Skipped.Cap,
		"invalid":              r.Skipped.Invalid,
		"sitemaps_refused":     r.Skipped.SitemapsRefused,
		"sitemaps_unavailable": r.Skipped.SitemapsUnavailable,
	}
	if r.Truncated {
		out["truncated"] = true
	}
	return out
}

func sitemapActivityOpts() workflow.ActivityOptions {
	return workflow.ActivityOptions{
		StartToCloseTimeout: 3 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumInterval:    30 * time.Second,
			MaximumAttempts:    3,
		},
	}
}

// seedSitemap discovers the sitemap URLs of a crawl whose input asks for them. It returns nil
// when the crawl does not ask (or the history predates the marker). queue is the durable
// frontier's queue: the activity then enqueues the URLs itself and the result carries counts
// only; empty, the URLs come back and the caller enqueues them.
func seedSitemap(
	ctx workflow.Context,
	in CrawlJobInput,
	a *activities.Activities,
	scope *crawlScopeRun,
	queue string,
) *sitemapRun {
	if in.Sitemap != sitemapInclude && in.Sitemap != sitemapOnly {
		return nil
	}
	if workflow.GetVersion(ctx, "crawl-sitemap-seeds-v1", workflow.DefaultVersion, 1) < 1 {
		return nil
	}
	run := &sitemapRun{mode: in.Sitemap}
	if scope == nil {
		run.failure = "the crawl has no scope to check the sitemap's URLs against"
		return run
	}
	seed := scope.base.SeedURL
	if seed == "" {
		run.failure = "the crawl has no seed URL"
		return run
	}
	maxURLs := in.MaxPages
	if queue == "" && maxURLs > sitemapInMemoryCap {
		maxURLs = sitemapInMemoryCap
	}
	var res activities.SitemapSeedsResult
	actx := workflow.WithActivityOptions(ctx, sitemapActivityOpts())
	err := workflow.ExecuteActivity(actx, a.SitemapSeeds, activities.SitemapSeedsInput{
		RunID:   in.RunID,
		OrgID:   in.OrgID,
		Seed:    seed,
		Crawl:   scope.contextFor(0),
		MaxURLs: maxURLs,
		Queue:   queue,
		Depth:   sitemapSeedDepth,
	}).Get(ctx, &res)
	if err != nil {
		run.failure = err.Error()
		return run
	}
	run.result = &res
	return run
}

// sitemapFailedEvent ends a `sitemap: only` crawl whose discovery failed.
func sitemapFailedEvent(s *sitemapRun) map[string]any {
	return map[string]any{
		"code":          sitemapUnavailable,
		"message":       "sitemap discovery failed, so a crawl that only follows the sitemap had nothing to crawl",
		"pages_visited": 0,
		"pages_failed":  0,
		"sitemap":       s.payload(),
	}
}

// scrapeAppliedBy reports whether the edge's answer proves it applied the scrape options. A
// crawl without options needs no proof.
func scrapeAppliedBy(in CrawlJobInput, res activities.RunPageResult) bool {
	if len(in.ScrapeOptions) == 0 {
		return true
	}
	return res.ScrapeApplied != nil && *res.ScrapeApplied
}

// scrapeFailedEvent ends a crawl whose edge did not apply the scrape options.
func scrapeFailedEvent(pagesVisited, pagesFailed, pagesBlocked uint32) map[string]any {
	return map[string]any{
		"code":          edgeScrapeUnsupported,
		"message":       "the edge did not apply the crawl's scrape options; no link of the page was followed",
		"pages_visited": pagesVisited,
		"pages_failed":  pagesFailed,
		"pages_blocked": pagesBlocked,
	}
}
