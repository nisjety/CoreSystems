package workflows

import (
	"context"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/converter"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// ---------------------------------------------------------------------------
// W2-U60 (D2-139): the workflow-level scope tests of CrawlJobWF (WS14.S3). The unit tests of
// crawlScopeRun are in crawl_scope_test.go; these run the whole loop, on the in-memory
// frontier and on the durable one (the production path), and read what the workflow sends
// the edge and what it reports.
// ---------------------------------------------------------------------------

// scopePage answers one RunPage of a scope test.
type scopePage func(in activities.RunPageInput) (activities.RunPageResult, error)

// scopeRefusal is the edge refusing a page: a blocklist denial with the given code.
func scopeRefusal(code string) error {
	body := []byte(`{"error":{"code":"SECURITY_BLOCKED","message":"refused","details":{"denial":{"code":"` + code + `","message":"m","remedy":"r"}}}}`)
	return errs.FromHTTPResponse("run_page", 403, body).Temporal()
}

// scopeApplied is a page the edge fetched and scoped: its links, and a report that says the
// scope was applied.
func scopeApplied(links []string, report activities.CrawlReport) activities.RunPageResult {
	report.ScopeApplied = true
	return activities.RunPageResult{Status: 200, Fingerprint: "blake3:scope", Links: links, Crawl: &report}
}

// scopeRun runs a CrawlJobWF with page standing in for the edge, on the in-memory frontier
// or the durable one. It returns what the workflow emitted, the RunPage inputs in the order
// the workflow sent them, the names of the activities it scheduled and its error.
func scopeRun(t *testing.T, durable bool, in CrawlJobInput, page scopePage) (*eventLog, []activities.RunPageInput, []string, error) {
	t.Helper()
	env, a := newEnv(t)
	a.DurableFrontier = durable
	log := &eventLog{}
	var sent []activities.RunPageInput
	var scheduled []string
	env.SetOnActivityStartedListener(func(info *activity.Info, _ context.Context, _ converter.EncodedValues) {
		scheduled = append(scheduled, info.ActivityType.Name)
	})
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, rp activities.RunPageInput) (activities.RunPageResult, error) {
			sent = append(sent, rp)
			return page(rp)
		})
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(log.emit)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)
	if durable {
		durableMocks(env, a)
	}
	env.ExecuteWorkflow(CrawlJobWF, in, a)
	require.True(t, env.IsWorkflowCompleted())
	return log, sent, scheduled, env.GetWorkflowError()
}

func scopeInput(runID string) CrawlJobInput {
	return CrawlJobInput{
		RunID: runID, JobID: "job_" + runID, OrgID: "org_scope",
		Seeds: []string{"https://scope.test/"}, MaxDepth: 3, MaxPages: 20,
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true},
		ScopeMode: activities.ScopeModeEnforce,
	}
}

// forEachFrontier runs body on the in-memory frontier and on the durable one.
func forEachFrontier(t *testing.T, body func(t *testing.T, durable bool)) {
	t.Helper()
	t.Run("in_memory", func(t *testing.T) { body(t, false) })
	t.Run("durable", func(t *testing.T) { body(t, true) })
}

func TestCrawlJobScope_EveryPageCarriesTheCrawlContext(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_scope_ctx")
		in.Robots = activities.RobotsIgnore
		_, sent, _, err := scopeRun(t, durable, in, func(rp activities.RunPageInput) (activities.RunPageResult, error) {
			if rp.URL == "https://scope.test/" {
				return scopeApplied([]string{"https://scope.test/a"}, activities.CrawlReport{}), nil
			}
			return scopeApplied(nil, activities.CrawlReport{}), nil
		})
		require.NoError(t, err)
		require.Len(t, sent, 2)
		root, child := sent[0].Crawl, sent[1].Crawl
		require.NotNil(t, root)
		require.NotNil(t, child)
		require.Equal(t, uint32(0), root.Depth)
		require.Equal(t, uint32(1), child.Depth, "the child is one level below the seed")
		for _, c := range []*activities.CrawlContext{root, child} {
			require.Equal(t, activities.CrawlContractVersion, c.V)
			require.Equal(t, activities.CrawlKindCrawl, c.Kind)
			require.Equal(t, uint32(3), c.MaxDepth)
			require.Equal(t, "https://scope.test/", c.SeedURL)
			require.Equal(t, []string{"scope.test"}, c.SeedHosts)
			require.Equal(t, activities.ScopeModeEnforce, c.ScopeMode)
			require.Equal(t, activities.RobotsIgnore, c.Robots)
			require.True(t, c.Scope.CrawlEntireDomain)
		}
	})
}

func TestCrawlJobScope_ObserveIsTheDefaultModeAndRobotsDefaultToRespect(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_scope_defaults")
		in.ScopeMode = ""
		in.Robots = ""
		_, sent, _, err := scopeRun(t, durable, in, func(activities.RunPageInput) (activities.RunPageResult, error) {
			return scopeApplied(nil, activities.CrawlReport{Mode: activities.ScopeModeObserve}), nil
		})
		require.NoError(t, err)
		require.Len(t, sent, 1)
		require.Equal(t, activities.ScopeModeObserve, sent[0].Crawl.ScopeMode)
		require.Equal(t, activities.RobotsRespect, sent[0].Crawl.Robots)
	})
}

// A legacy scope is a crawl accepted with the scope switch off: the workflow sends no context
// and its terminal event names none of the scope fields.
func TestCrawlJobScope_ALegacyScopeSendsNoContextAndReportsNoScopeFields(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_scope_legacy")
		in.Scope = &activities.CrawlScope{Legacy: true}
		log, sent, _, err := scopeRun(t, durable, in, func(rp activities.RunPageInput) (activities.RunPageResult, error) {
			return activities.RunPageResult{Status: 200, Fingerprint: "blake3:legacy"}, nil
		})
		require.NoError(t, err)
		require.Len(t, sent, 1)
		require.Nil(t, sent[0].Crawl, "a legacy crawl carries no scope to the edge")
		completed := log.first(t, quarrycontracts.EvtRunCompleted)
		for _, key := range []string{"pages_blocked", "pages_blocked_robots", "links_denied", "hosts_added", "denials"} {
			require.NotContains(t, completed, key)
		}
	})
}

// An edge that predates the scope answers without the proof that it applied it. The run
// ends there with a failure and follows none of the page's links.
func TestCrawlJobScope_AnEdgeThatDoesNotApplyTheScopeFailsTheRun(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		log, sent, _, err := scopeRun(t, durable, scopeInput("run_scope_unsupported"),
			func(rp activities.RunPageInput) (activities.RunPageResult, error) {
				return activities.RunPageResult{
					Status: 200, Fingerprint: "blake3:old-edge",
					Links: []string{"https://scope.test/a", "https://elsewhere.test/"},
				}, nil
			})
		require.NoError(t, err, "the run ends with an event, not with a workflow error")
		require.Len(t, sent, 1, "no link of the page the edge did not scope is followed")
		failed := log.first(t, quarrycontracts.EvtRunFailed)
		require.Equal(t, edgeScopeUnsupported, failed["code"])
		require.EqualValues(t, 0, failed["pages_visited"])
		require.False(t, log.has(quarrycontracts.EvtRunCompleted))
	})
}

// A page the edge refuses (scope or robots) is blocked, not failed: it does not count toward
// the failure bound, robots refusals are counted apart, and the crawl goes on.
func TestCrawlJobScope_RefusedPagesCountAsBlockedAndNotAsFailed(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		log, sent, _, err := scopeRun(t, durable, scopeInput("run_scope_blocked"),
			func(rp activities.RunPageInput) (activities.RunPageResult, error) {
				switch rp.URL {
				case "https://scope.test/":
					return scopeApplied([]string{
						"https://scope.test/outside", "https://scope.test/private", "https://scope.test/ok",
					}, activities.CrawlReport{}), nil
				case "https://scope.test/outside":
					return activities.RunPageResult{}, scopeRefusal("external_host_disabled")
				case "https://scope.test/private":
					return activities.RunPageResult{}, scopeRefusal("robots_disallowed")
				}
				return scopeApplied(nil, activities.CrawlReport{}), nil
			})
		require.NoError(t, err)
		require.Len(t, sent, 4)
		completed := log.first(t, quarrycontracts.EvtRunCompleted)
		require.EqualValues(t, 2, completed["pages_visited"], "the seed and the page that was allowed")
		require.EqualValues(t, 0, completed["pages_failed"])
		require.EqualValues(t, 2, completed["pages_blocked"])
		require.EqualValues(t, 1, completed["pages_blocked_robots"], "only the robots refusal is a robots one")
		require.False(t, log.has(quarrycontracts.EvtRunFailed))
	})
}

// The reports of the pages fold into the terminal event: the denied links, the hosts the edge
// added to the scope on a redirect and the summary of the denials. Later pages are scoped to
// the added hosts too.
func TestCrawlJobScope_ReportsFoldIntoTheTerminalEventAndWidenTheSeedHosts(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		log, sent, _, err := scopeRun(t, durable, scopeInput("run_scope_reports"),
			func(rp activities.RunPageInput) (activities.RunPageResult, error) {
				if rp.URL != "https://scope.test/" {
					return scopeApplied(nil, activities.CrawlReport{LinksDenied: 1}), nil
				}
				remedy := "allow external links"
				return scopeApplied([]string{"https://www.scope.test/a"}, activities.CrawlReport{
					LinksDenied: 2,
					HostsAdded:  []string{"www.scope.test"},
					Denials: activities.DenialReport{
						Counts: map[string]uint32{"external_host_disabled": 2},
						Samples: []activities.DenialSample{
							{URL: "https://other.test/x", Code: "external_host_disabled", Message: "external", Remedy: &remedy, Depth: 1},
						},
					},
				}), nil
			})
		require.NoError(t, err)
		require.Len(t, sent, 2)
		require.Equal(t, []string{"scope.test"}, sent[0].Crawl.SeedHosts)
		require.Equal(t, []string{"scope.test", "www.scope.test"}, sent[1].Crawl.SeedHosts,
			"a host the edge added is part of the scope of the pages after it")
		completed := log.first(t, quarrycontracts.EvtRunCompleted)
		require.EqualValues(t, 3, completed["links_denied"])
		require.ElementsMatch(t, []any{"www.scope.test"}, completed["hosts_added"])
		denials, ok := completed["denials"].(map[string]any)
		require.True(t, ok, "denials is an object: %T", completed["denials"])
		counts, _ := denials["counts"].(map[string]any)
		require.EqualValues(t, 2, counts["external_host_disabled"])
	})
}

// The depth bound stops the links of a page at the last level: nothing past it is fetched.
func TestCrawlJobScope_TheDepthBoundStopsLinkFollowing(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_scope_depth")
		in.MaxDepth = 1
		_, sent, _, err := scopeRun(t, durable, in, func(rp activities.RunPageInput) (activities.RunPageResult, error) {
			return scopeApplied([]string{rp.URL + "n"}, activities.CrawlReport{}), nil
		})
		require.NoError(t, err)
		require.Len(t, sent, 2, "the seed (depth 0) and its child (depth 1); the child's own link is past the bound")
		require.Equal(t, uint32(1), sent[1].Crawl.Depth)
	})
}

// The children of a page under scope go to the durable queue in one batch activity, not one
// activity per link (WS14.S3). The seed itself is still enqueued one at a time.
func TestCrawlJobScope_DurableChildrenGoInOneBatchCallPerPage(t *testing.T) {
	_, _, scheduled, err := scopeRun(t, true, scopeInput("run_scope_batch"),
		func(rp activities.RunPageInput) (activities.RunPageResult, error) {
			if rp.URL == "https://scope.test/" {
				return scopeApplied([]string{
					"https://scope.test/a", "https://scope.test/b", "https://scope.test/c",
				}, activities.CrawlReport{}), nil
			}
			return scopeApplied(nil, activities.CrawlReport{}), nil
		})
	require.NoError(t, err)
	count := map[string]int{}
	for _, name := range scheduled {
		count[name]++
	}
	require.Equal(t, 1, count["FrontierEnqueue"], "only the seed goes in alone")
	require.Equal(t, 1, count["FrontierEnqueueBatch"], "the three children of the seed go in together")
	require.Equal(t, 4, count["RunPage"])
}
