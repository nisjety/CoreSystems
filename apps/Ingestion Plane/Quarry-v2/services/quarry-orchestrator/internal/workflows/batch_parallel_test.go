package workflows

import (
	"context"
	"errors"
	"sync"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// setBatchParallelFlag sets QUARRY_BATCH_PARALLEL for one test without touching the
// process environment. The tests that use it do not run in parallel.
func setBatchParallelFlag(t *testing.T, value string) {
	t.Helper()
	prev := batchParallelEnv
	batchParallelEnv = func() string { return value }
	t.Cleanup(func() { batchParallelEnv = prev })
}

// concurrencyProbe counts the RunPage activities running at once.
type concurrencyProbe struct {
	mu      sync.Mutex
	running int
	max     int
	urls    []string
}

func (p *concurrencyProbe) enter(url string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.running++
	if p.running > p.max {
		p.max = p.running
	}
	p.urls = append(p.urls, url)
}

func (p *concurrencyProbe) leave() {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.running--
}

func TestParseBatchParallel(t *testing.T) {
	cases := map[string]int{
		"":      0,
		"0":     0,
		"off":   0,
		"false": 0,
		"junk":  0,
		"-3":    0,
		"on":    batchParallelDefault,
		"TRUE":  batchParallelDefault,
		"1":     1,
		" 6 ":   6,
		"999":   batchParallelHardMax,
	}
	for raw, want := range cases {
		require.Equal(t, want, parseBatchParallel(raw), "flag %q", raw)
	}
}

func TestEffectiveBatchParallel(t *testing.T) {
	require.Equal(t, 4, effectiveBatchParallel(4, 0, 10), "no org cap keeps the flag")
	require.Equal(t, 2, effectiveBatchParallel(4, 2, 10), "the org cap lowers it")
	require.Equal(t, 4, effectiveBatchParallel(4, 50, 10), "an org cap above the flag does not raise it")
	require.Equal(t, 3, effectiveBatchParallel(8, 0, 3), "never more than the URLs")
	require.Equal(t, 0, effectiveBatchParallel(0, 8, 10), "flag off stays off")
}

func TestBatchInputFromParams_MaxParallel(t *testing.T) {
	in, err := BatchInputFromParams(map[string]any{
		"urls":         []any{"https://a.example"},
		"max_parallel": float64(3),
	}, "run_1", "job_1")
	require.NoError(t, err)
	require.EqualValues(t, 3, in.MaxParallel)

	in, err = BatchInputFromParams(map[string]any{"urls": []any{"https://a.example"}}, "run_1", "job_1")
	require.NoError(t, err)
	require.EqualValues(t, 0, in.MaxParallel, "absent is no org cap")
}

// TestBatchJobWF_ParallelRunsEveryURLAndKeepsCounts (W3 E3.7.2): with the flag on the
// batch visits every URL once, never runs more pages at a time than its limit, and ends
// with the same terminal counts as the sequential batch.
func TestBatchJobWF_ParallelRunsEveryURLAndKeepsCounts(t *testing.T) {
	setBatchParallelFlag(t, "3")
	env, a := newEnv(t)
	env.OnGetVersion(batchCheckVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))
	env.OnGetVersion(batchParallelVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))

	probe := &concurrencyProbe{}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			probe.enter(in.URL)
			defer probe.leave()
			if in.URL == "https://batch.example/p4" {
				return activities.RunPageResult{},
					errs.New(errs.CategoryBlocklist, "test", errors.New("denied")).Temporal()
			}
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	var completed map[string]any
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ string, evt quarrycontracts.Event) error {
			if evt.Type == quarrycontracts.EvtRunCompleted {
				completed = evt.Payload
			}
			return nil
		},
	)

	urls := batchURLs(7)
	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_parallel", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.ElementsMatch(t, urls, probe.urls, "every URL runs exactly once")
	require.LessOrEqual(t, probe.max, 3, "never more pages at once than the flag allows")
	require.EqualValues(t, 6, completed["pages_visited"])
	require.EqualValues(t, 1, completed["pages_failed"])
	require.EqualValues(t, 1, completed["pages_blocked"])
}

// TestBatchJobWF_ParallelOrgCapBoundsConcurrency (W3 E3.7.2): the org's MaxParallel caps
// the flag.
func TestBatchJobWF_ParallelOrgCapBoundsConcurrency(t *testing.T) {
	setBatchParallelFlag(t, "8")
	env, a := newEnv(t)
	env.OnGetVersion(batchParallelVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))

	probe := &concurrencyProbe{}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			probe.enter(in.URL)
			defer probe.leave()
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	urls := batchURLs(6)
	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_cap", URLs: urls, MaxParallel: 2}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, probe.urls, len(urls))
	require.LessOrEqual(t, probe.max, 2, "the org cap bounds the pool")
}

// TestBatchJobWF_ParallelOldHistoryStaysSequential (W3 E3.7.2): a history recorded
// before batch-parallel-v1 never reads the flag and runs one page at a time.
func TestBatchJobWF_ParallelOldHistoryStaysSequential(t *testing.T) {
	setBatchParallelFlag(t, "8")
	env, a := newEnv(t)
	env.OnGetVersion(batchParallelVersion, workflow.DefaultVersion, 1).Return(workflow.DefaultVersion)

	probe := &concurrencyProbe{}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			probe.enter(in.URL)
			defer probe.leave()
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	urls := batchURLs(4)
	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_seq", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Equal(t, urls, probe.urls, "the sequential batch keeps URL order")
	require.Equal(t, 1, probe.max)
}

// TestBatchJobWF_ParallelFlagOffIsSequential (W3 E3.7.2): with the marker but the flag
// off (the default) the batch runs the sequential loop.
func TestBatchJobWF_ParallelFlagOffIsSequential(t *testing.T) {
	setBatchParallelFlag(t, "")
	env, a := newEnv(t)
	env.OnGetVersion(batchParallelVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))

	probe := &concurrencyProbe{}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			probe.enter(in.URL)
			defer probe.leave()
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	urls := batchURLs(4)
	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_off", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Equal(t, urls, probe.urls)
	require.Equal(t, 1, probe.max)
}

// TestBatchJobWF_ParallelCancelEndsWithCancelled (W3 E3.7.2): the control handlers are
// reused, so a cancel before the first page launches no page and ends with run_cancelled.
func TestBatchJobWF_ParallelCancelEndsWithCancelled(t *testing.T) {
	setBatchParallelFlag(t, "4")
	env, a := newEnv(t)
	env.OnGetVersion(batchParallelVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))
	events := &controlEvents{}
	events.record(env, a)

	var mu sync.Mutex
	pages := 0
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ activities.RunPageInput) (activities.RunPageResult, error) {
			mu.Lock()
			pages++
			mu.Unlock()
			return activities.RunPageResult{Status: 200}, nil
		})
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalCancel, nil) }, 0)

	urls := batchURLs(10)
	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_parallel_cancel", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Less(t, pages, len(urls), "cancel launches no more pages")
	require.Equal(t, quarrycontracts.EvtRunCancelled, events.last())
	require.Equal(t, 0, events.count(quarrycontracts.EvtRunCompleted))
}
