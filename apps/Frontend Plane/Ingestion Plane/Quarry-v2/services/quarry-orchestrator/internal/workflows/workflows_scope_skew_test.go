package workflows

import (
	"context"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// ---------------------------------------------------------------------------
// W2-U64 (D2-029, D2-073): an edge older than the orchestrator answers a scoped page without
// scope_applied during a rolling deploy. An observe-mode crawl goes on as a legacy-unscoped
// one, an enforce-mode crawl is refused, and an execution recorded before the tolerant
// marker keeps the strict behaviour it replays with.
// ---------------------------------------------------------------------------

func TestScopeVerdict(t *testing.T) {
	scoped := func(mode string, tolerant bool) *crawlScopeRun {
		in := scopeInput("run_verdict")
		in.ScopeMode = mode
		s := newCrawlScopeRun(in)
		s.tolerateUnapplied = tolerant
		return s
	}
	applied := &activities.CrawlReport{ScopeApplied: true}
	missing := &activities.CrawlReport{}
	cases := []struct {
		name     string
		mode     string
		tolerant bool
		report   *activities.CrawlReport
		want     scopeVerdict
	}{
		{"applied observe", activities.ScopeModeObserve, true, applied, verdictApplied},
		{"applied enforce", activities.ScopeModeEnforce, true, applied, verdictApplied},
		{"applied on the strict path", activities.ScopeModeObserve, false, applied, verdictApplied},
		{"false observe tolerant", activities.ScopeModeObserve, true, missing, verdictLegacyUnscoped},
		{"absent report observe tolerant", activities.ScopeModeObserve, true, nil, verdictLegacyUnscoped},
		{"empty mode is observe", "", true, missing, verdictLegacyUnscoped},
		{"unknown mode is observe", "strict-ish", true, nil, verdictLegacyUnscoped},
		{"false enforce tolerant", activities.ScopeModeEnforce, true, missing, verdictRefused},
		{"absent report enforce tolerant", activities.ScopeModeEnforce, true, nil, verdictRefused},
		{"false observe, pre-marker execution", activities.ScopeModeObserve, false, missing, verdictRefused},
		{"false enforce, pre-marker execution", activities.ScopeModeEnforce, false, missing, verdictRefused},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			require.Equal(t, c.want, scoped(c.mode, c.tolerant).verdict(c.report))
		})
	}
}

// skewRun runs a crawl whose edge never reports scope_applied. tolerant pins the GetVersion
// answer for the marker: false is an execution recorded before it.
func skewRun(t *testing.T, durable, tolerant bool, in CrawlJobInput) (*eventLog, []activities.RunPageInput, error) {
	t.Helper()
	env, a := newEnv(t)
	a.DurableFrontier = durable
	log := &eventLog{}
	var sent []activities.RunPageInput
	if tolerant {
		env.OnGetVersion(crawlScopeTolerantVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))
	} else {
		env.OnGetVersion(crawlScopeTolerantVersion, workflow.DefaultVersion, 1).Return(workflow.DefaultVersion)
	}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, rp activities.RunPageInput) (activities.RunPageResult, error) {
			sent = append(sent, rp)
			links := []string{}
			if rp.URL == "https://scope.test/" {
				links = []string{"https://scope.test/a"}
			}
			return activities.RunPageResult{Status: 200, Fingerprint: "blake3:old-edge", Links: links}, nil
		})
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(log.emit)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)
	if durable {
		durableMocks(env, a)
	}
	env.ExecuteWorkflow(CrawlJobWF, in, a)
	require.True(t, env.IsWorkflowCompleted())
	return log, sent, env.GetWorkflowError()
}

func TestCrawlJobScope_ObserveCrawlGoesOnWhenTheEdgeDoesNotReportScopeApplied(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_skew_observe")
		in.ScopeMode = activities.ScopeModeObserve
		log, sent, err := skewRun(t, durable, true, in)
		require.NoError(t, err)
		require.Len(t, sent, 2, "the seed and the link it carried: the crawl is not cut after page one")
		completed := log.first(t, quarrycontracts.EvtRunCompleted)
		require.EqualValues(t, 2, completed["pages_visited"])
		require.False(t, log.has(quarrycontracts.EvtRunFailed))
	})
}

func TestCrawlJobScope_EnforceCrawlIsRefusedWhenTheEdgeDoesNotReportScopeApplied(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		log, sent, err := skewRun(t, durable, true, scopeInput("run_skew_enforce"))
		require.NoError(t, err)
		require.Len(t, sent, 1, "no link of an unscoped page is followed under enforce")
		failed := log.first(t, quarrycontracts.EvtRunFailed)
		require.Equal(t, edgeScopeUnsupported, failed["code"])
	})
}

func TestCrawlJobScope_AnExecutionRecordedBeforeTheMarkerKeepsTheStrictCheck(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_skew_old")
		in.ScopeMode = activities.ScopeModeObserve
		log, sent, err := skewRun(t, durable, false, in)
		require.NoError(t, err)
		require.Len(t, sent, 1)
		require.Equal(t, edgeScopeUnsupported, log.first(t, quarrycontracts.EvtRunFailed)["code"])
	})
}
