package workflows

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// INNH-6: sitemap seeding and per-page scrape options in CrawlJobWF, on the durable
// frontier (the production path) and on the in-memory one.

const sitemapRoot = "https://site.test/"

// eventLog records every event the workflow emits.
type eventLog struct {
	mu     sync.Mutex
	events []quarrycontracts.Event
}

func (l *eventLog) emit(_ context.Context, _ string, event quarrycontracts.Event) error {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.events = append(l.events, event)
	return nil
}

func (l *eventLog) first(t *testing.T, kind quarrycontracts.EventType) map[string]any {
	t.Helper()
	l.mu.Lock()
	defer l.mu.Unlock()
	for _, e := range l.events {
		if e.Type == kind {
			return e.Payload
		}
	}
	t.Fatalf("no %s event was emitted", kind)
	return nil
}

func (l *eventLog) has(kind quarrycontracts.EventType) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	for _, e := range l.events {
		if e.Type == kind {
			return true
		}
	}
	return false
}

// addItem is the queue's enqueue for the SitemapSeeds double: like the real queue, a request
// id it already holds is dropped.
func (q *durableFakeQueue) addItem(requestID, url string, depth uint32) {
	q.enqueued++
	if _, seen := q.known[requestID]; seen {
		return
	}
	q.known[requestID] = struct{}{}
	q.pending = append(q.pending, activities.FrontierQueueItem{
		RequestID: requestID,
		URL:       url,
		Payload:   map[string]any{"depth": float64(depth)},
		Attempt:   1,
	})
}

// sitemapSite is a page graph with an orphan page that only the sitemap lists.
type sitemapSite struct {
	links   map[string][]string
	fetched map[string]uint32 // url -> depth of the context it was fetched with
	order   []string
	calls   []activities.RunPageInput
	mu      sync.Mutex
}

func newSitemapSite() *sitemapSite {
	return &sitemapSite{
		links: map[string][]string{
			sitemapRoot:                      {"https://site.test/a"},
			"https://site.test/a":            nil,
			"https://site.test/orphan":       {"https://site.test/orphan-child"},
			"https://site.test/orphan-child": nil,
			"https://site.test/p2":           nil,
		},
		fetched: map[string]uint32{},
	}
}

func (s *sitemapSite) runPage(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.order = append(s.order, in.URL)
	s.calls = append(s.calls, in)
	depth := uint32(0)
	res := activities.RunPageResult{Status: 200, Fingerprint: "blake3:" + in.URL, Links: s.links[in.URL]}
	if in.Crawl != nil {
		depth = in.Crawl.Depth
		res.Crawl = &activities.CrawlReport{ScopeApplied: true}
	}
	s.fetched[in.URL] = depth
	if len(in.Scrape) > 0 {
		applied := true
		res.ScrapeApplied = &applied
	}
	return res, nil
}

func sitemapInput(mode string) CrawlJobInput {
	return CrawlJobInput{
		RunID: "run_sitemap", JobID: "job_sitemap", OrgID: "org_s",
		Seeds: []string{sitemapRoot}, MaxDepth: 3, MaxPages: 50,
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true},
		ScopeMode: activities.ScopeModeEnforce,
		Sitemap:   mode,
	}
}

func baseMocks(env *testsuite.TestWorkflowEnvironment, a *activities.Activities, site *sitemapSite, log *eventLog) {
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(site.runPage)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(log.emit)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)
}

// durableMocks wires the fake queue behind the frontier activities and returns it.
func durableMocks(env *testsuite.TestWorkflowEnvironment, a *activities.Activities) *durableFakeQueue {
	q := newDurableFakeQueue()
	env.OnActivity(a.FrontierEnqueue, mock.Anything, mock.Anything).Return(q.enqueue)
	env.OnActivity(a.FrontierEnqueueBatch, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.FrontierEnqueueBatchInput) (uint32, error) {
			for _, item := range in.Items {
				q.addItem(item.RequestID, item.URL, item.Depth)
			}
			return uint32(len(in.Items)), nil
		})
	env.OnActivity(a.FrontierPop, mock.Anything, mock.Anything).Return(q.pop)
	env.OnActivity(a.FrontierAck, mock.Anything, mock.Anything).Return(q.ack)
	return q
}

func sitemapDouble(q *durableFakeQueue, seen *[]activities.SitemapSeedsInput, urls []string) func(context.Context, activities.SitemapSeedsInput) (activities.SitemapSeedsResult, error) {
	return func(_ context.Context, in activities.SitemapSeedsInput) (activities.SitemapSeedsResult, error) {
		*seen = append(*seen, in)
		res := activities.SitemapSeedsResult{Discovered: 7, SitemapsFetched: 2, NestedFetched: 1, Seeded: uint32(len(urls))}
		res.Skipped.OutOfScope = 3
		if in.Queue != "" {
			for _, u := range urls {
				q.addItem(activities.FrontierRequestID(in.RunID, u), u, in.Depth)
			}
		} else {
			res.URLs = urls
		}
		return res, nil
	}
}

func TestCrawlJobDurable_SitemapIncludeSeedsTheOrphanPagesAndStillFollowsLinks(t *testing.T) {
	env, a := newEnv(t)
	a.DurableFrontier = true
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	q := durableMocks(env, a)
	var asked []activities.SitemapSeedsInput
	env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(
		sitemapDouble(q, &asked, []string{"https://site.test/orphan", "https://site.test/p2", "https://site.test/a"}))

	env.ExecuteWorkflow(CrawlJobWF, sitemapInput("include"), a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	// The seed, its link, the two sitemap-only pages and the orphan's own link are all fetched, once.
	require.Len(t, site.fetched, 5, "%v", site.order)
	require.Len(t, site.order, 5, "no page is fetched twice: %v", site.order)
	require.Equal(t, uint32(1), site.fetched["https://site.test/orphan"], "sitemap URLs are one level below the seed")
	require.Equal(t, uint32(2), site.fetched["https://site.test/orphan-child"])
	require.Equal(t, uint32(0), site.fetched[sitemapRoot])

	// One SitemapSeeds call, into the run's queue, bounded by the page bound.
	require.Len(t, asked, 1)
	require.Equal(t, durableFrontierQueueName("run_sitemap"), asked[0].Queue)
	require.Equal(t, uint32(50), asked[0].MaxURLs)
	require.Equal(t, uint32(1), asked[0].Depth)
	require.Equal(t, sitemapRoot, asked[0].Seed)
	require.NotNil(t, asked[0].Crawl)
	require.Equal(t, uint32(0), asked[0].Crawl.Depth)

	started := log.first(t, quarrycontracts.EvtRunStarted)
	require.Equal(t, "include", started["sitemap"])
	completed := log.first(t, quarrycontracts.EvtRunCompleted)
	summary, _ := completed["sitemap"].(map[string]any)
	require.NotNil(t, summary, "the counts ride the existing run_completed event: %v", completed)
	require.Equal(t, "include", summary["mode"])
	require.Equal(t, float64(3), summary["seeded"])
	require.Equal(t, float64(7), summary["discovered"])
	skipped, _ := summary["skipped"].(map[string]any)
	require.Equal(t, float64(3), skipped["out_of_scope"])
}

func TestCrawlJobDurable_SitemapOnlyCrawlsTheListedPagesAndFollowsNoLink(t *testing.T) {
	env, a := newEnv(t)
	a.DurableFrontier = true
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	q := durableMocks(env, a)
	var asked []activities.SitemapSeedsInput
	env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(
		sitemapDouble(q, &asked, []string{"https://site.test/orphan", "https://site.test/p2"}))

	env.ExecuteWorkflow(CrawlJobWF, sitemapInput("only"), a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	require.ElementsMatch(t, []string{sitemapRoot, "https://site.test/orphan", "https://site.test/p2"}, site.order)
	_, followedSeedLink := site.fetched["https://site.test/a"]
	_, followedOrphanLink := site.fetched["https://site.test/orphan-child"]
	require.False(t, followedSeedLink, "`only` follows no link of the seed page")
	require.False(t, followedOrphanLink, "`only` follows no link of a sitemap page")
	require.Equal(t, "only", log.first(t, quarrycontracts.EvtRunStarted)["sitemap"])
}

func TestCrawlJobDurable_NoSitemapNeverCallsTheActivityOrNamesItInTheEvents(t *testing.T) {
	for _, mode := range []string{"", "skip", "bogus"} {
		env, a := newEnv(t)
		a.DurableFrontier = true
		site, log := newSitemapSite(), &eventLog{}
		baseMocks(env, a, site, log)
		durableMocks(env, a)
		env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(activities.SitemapSeedsResult{}, nil)

		env.ExecuteWorkflow(CrawlJobWF, sitemapInput(mode), a)
		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		env.AssertNumberOfCalls(t, "SitemapSeeds", 0)
		require.ElementsMatch(t, []string{sitemapRoot, "https://site.test/a"}, site.order, mode)
		_, named := log.first(t, quarrycontracts.EvtRunStarted)["sitemap"]
		require.False(t, named, "%q: run_started is unchanged", mode)
		_, summarised := log.first(t, quarrycontracts.EvtRunCompleted)["sitemap"]
		require.False(t, summarised, "%q: run_completed is unchanged", mode)
	}
}

func TestCrawlJobInMemory_SitemapURLsJoinTheFrontierBoundedForWorkflowState(t *testing.T) {
	env, a := newEnv(t)
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	var asked []activities.SitemapSeedsInput
	env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(
		sitemapDouble(nil, &asked, []string{"https://site.test/orphan", "https://site.test/p2", sitemapRoot}))

	in := sitemapInput("include")
	in.MaxPages = 2000
	env.ExecuteWorkflow(CrawlJobWF, in, a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	require.Len(t, asked, 1)
	require.Empty(t, asked[0].Queue, "the in-memory frontier takes the URLs back")
	require.Equal(t, sitemapInMemoryCap, asked[0].MaxURLs, "the in-memory frontier keeps URLs in workflow state, so it is capped")
	require.ElementsMatch(t, []string{
		sitemapRoot, "https://site.test/a", "https://site.test/orphan", "https://site.test/orphan-child", "https://site.test/p2",
	}, site.order, "the seed listed in the sitemap is not fetched twice")
	require.Equal(t, uint32(1), site.fetched["https://site.test/p2"])
}

func TestCrawlJobInMemory_SitemapOnlyFollowsNoLink(t *testing.T) {
	env, a := newEnv(t)
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	var asked []activities.SitemapSeedsInput
	env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(
		sitemapDouble(nil, &asked, []string{"https://site.test/orphan"}))

	env.ExecuteWorkflow(CrawlJobWF, sitemapInput("only"), a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.ElementsMatch(t, []string{sitemapRoot, "https://site.test/orphan"}, site.order)
}

func TestCrawlJob_SitemapFailureEndsAnOnlyCrawlAndLetsAnIncludeCrawlGoOn(t *testing.T) {
	failure := temporal.NewNonRetryableApplicationError("the edge does not serve sitemap discovery (501)", "validation", errors.New("501"))
	for _, durable := range []bool{true, false} {
		// only: nothing to crawl but the seed, so the run fails with its own code.
		env, a := newEnv(t)
		a.DurableFrontier = durable
		site, log := newSitemapSite(), &eventLog{}
		baseMocks(env, a, site, log)
		if durable {
			durableMocks(env, a)
		}
		env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(activities.SitemapSeedsResult{}, failure)
		env.ExecuteWorkflow(CrawlJobWF, sitemapInput("only"), a)
		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		failed := log.first(t, quarrycontracts.EvtRunFailed)
		require.Equal(t, sitemapUnavailable, failed["code"], "durable=%v", durable)
		require.False(t, log.has(quarrycontracts.EvtRunCompleted))
		require.Empty(t, site.order, "durable=%v: no page is fetched", durable)

		// include: the links are followed as usual and the completed event says what failed.
		env, a = newEnv(t)
		a.DurableFrontier = durable
		site, log = newSitemapSite(), &eventLog{}
		baseMocks(env, a, site, log)
		if durable {
			durableMocks(env, a)
		}
		env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(activities.SitemapSeedsResult{}, failure)
		env.ExecuteWorkflow(CrawlJobWF, sitemapInput("include"), a)
		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		require.ElementsMatch(t, []string{sitemapRoot, "https://site.test/a"}, site.order, "durable=%v", durable)
		summary, _ := log.first(t, quarrycontracts.EvtRunCompleted)["sitemap"].(map[string]any)
		require.NotNil(t, summary)
		require.NotEmpty(t, summary["error"], "durable=%v", durable)
	}
}

func TestCrawlJob_ALegacyCrawlHasNoScopeToSeedThrough(t *testing.T) {
	env, a := newEnv(t)
	a.DurableFrontier = true
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	durableMocks(env, a)
	env.OnActivity(a.SitemapSeeds, mock.Anything, mock.Anything).Return(activities.SitemapSeedsResult{}, nil)
	in := sitemapInput("only")
	in.Scope = &activities.CrawlScope{Legacy: true}
	env.ExecuteWorkflow(CrawlJobWF, in, a)
	require.True(t, env.IsWorkflowCompleted())
	require.Equal(t, sitemapUnavailable, log.first(t, quarrycontracts.EvtRunFailed)["code"])
	env.AssertNumberOfCalls(t, "SitemapSeeds", 0)
}

// ---- INNH-6b

const scrapeOptionsJSON = `{"exclude_tags":["nav"],"extractor":"readability","include_tags":[],"only_main_content":true,"remove_base64_images":false}`

func TestCrawlJobDurable_ScrapeOptionsRideEveryPageAndAnEchoIsRequired(t *testing.T) {
	env, a := newEnv(t)
	a.DurableFrontier = true
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	durableMocks(env, a)

	in := sitemapInput("")
	in.ScrapeOptions = json.RawMessage(scrapeOptionsJSON)
	env.ExecuteWorkflow(CrawlJobWF, in, a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, site.calls, 2)
	for _, page := range site.calls {
		require.JSONEq(t, scrapeOptionsJSON, string(page.Scrape), "every page carries the options")
	}
	require.Equal(t, true, log.first(t, quarrycontracts.EvtRunStarted)["scrape_options"])
	require.True(t, log.has(quarrycontracts.EvtRunCompleted))
}

func TestCrawlJob_ScrapeOptionsAnEdgeThatDoesNotEchoEndsTheRun(t *testing.T) {
	for _, durable := range []bool{true, false} {
		env, a := newEnv(t)
		a.DurableFrontier = durable
		log := &eventLog{}
		env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(log.emit)
		env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)
		if durable {
			durableMocks(env, a)
		}
		var pages []string
		env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
			func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
				pages = append(pages, in.URL)
				// An old edge: it applies the scope and ignores `scrape`, so there is no echo.
				return activities.RunPageResult{
					Status: 200, Fingerprint: "blake3:x", Links: []string{"https://site.test/a"},
					Crawl: &activities.CrawlReport{ScopeApplied: true},
				}, nil
			})

		in := sitemapInput("")
		in.ScrapeOptions = json.RawMessage(scrapeOptionsJSON)
		env.ExecuteWorkflow(CrawlJobWF, in, a)
		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		require.Equal(t, []string{sitemapRoot}, pages, "durable=%v: no link of the page is followed", durable)
		require.Equal(t, edgeScrapeUnsupported, log.first(t, quarrycontracts.EvtRunFailed)["code"])
		require.False(t, log.has(quarrycontracts.EvtRunCompleted))
	}
}

func TestCrawlJob_WithoutScrapeOptionsThePageCallIsUnchanged(t *testing.T) {
	env, a := newEnv(t)
	site, log := newSitemapSite(), &eventLog{}
	baseMocks(env, a, site, log)
	env.ExecuteWorkflow(CrawlJobWF, sitemapInput(""), a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.NotEmpty(t, site.calls)
	for _, page := range site.calls {
		require.Empty(t, page.Scrape)
	}
	_, named := log.first(t, quarrycontracts.EvtRunStarted)["scrape_options"]
	require.False(t, named)
}

// ---- the input builders

func TestCrawlInputFromParams_ReadsSitemapAndScrapeOptions(t *testing.T) {
	params := map[string]any{
		"url":     sitemapRoot,
		"sitemap": "only",
		"scrape_options": map[string]any{
			"exclude_tags": []any{"nav"}, "only_main_content": true, "extractor": "readability",
			"include_tags": []any{}, "remove_base64_images": false,
		},
	}
	in, err := CrawlInputFromParams(params, "run_1", "job_1", 3, 50)
	require.NoError(t, err)
	require.Equal(t, "only", in.Sitemap)
	require.JSONEq(t, scrapeOptionsJSON, string(in.ScrapeOptions))

	for _, value := range []any{"skip", "", "everything", true, nil} {
		in, err := CrawlInputFromParams(map[string]any{"url": sitemapRoot, "sitemap": value}, "run_1", "job_1", 3, 50)
		require.NoError(t, err)
		require.Empty(t, in.Sitemap, "%v is links only", value)
	}
	plain, err := CrawlInputFromParams(map[string]any{"url": sitemapRoot}, "run_1", "job_1", 3, 50)
	require.NoError(t, err)
	require.Empty(t, plain.Sitemap)
	require.Nil(t, plain.ScrapeOptions)
	for _, value := range []any{nil, map[string]any{}, "x"} {
		in, err := CrawlInputFromParams(map[string]any{"url": sitemapRoot, "scrape_options": value}, "run_1", "job_1", 3, 50)
		require.NoError(t, err)
		require.Nil(t, in.ScrapeOptions, "%v is the default cleaning", value)
	}
}

func TestCrawlJobInput_AnInputWithoutTheNewFieldsEncodesAsBefore(t *testing.T) {
	raw, err := json.Marshal(CrawlJobInput{RunID: "r", Seeds: []string{sitemapRoot}, MaxDepth: 1, MaxPages: 2})
	require.NoError(t, err)
	require.NotContains(t, string(raw), "sitemap")
	require.NotContains(t, string(raw), "scrape_options")
	var back CrawlJobInput
	require.NoError(t, json.Unmarshal([]byte(`{"run_id":"r","seeds":["`+sitemapRoot+`"],"max_depth":1,"max_pages":2}`), &back))
	require.Empty(t, back.Sitemap, "a workflow input recorded before the fields existed decodes with them empty")
	require.Nil(t, back.ScrapeOptions)
}

func TestFrontierRequestIDIsTheOneTheWorkflowAlwaysUsed(t *testing.T) {
	require.Equal(t, durableFrontierRequestID("run_1", "https://a.test/x"), activities.FrontierRequestID("run_1", "https://a.test/x"))
	require.Contains(t, activities.FrontierRequestID("run_1", "https://a.test/x"), "frontier-")
}
