package workflows

// Tests of the job control signals (WS15.L2): batch cancel and pause, the
// Await-based pause of a crawl, and the old-history timer path. Written in the
// build phase and compile-checked only; the wave's test phase runs them.

import (
	"context"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// controlEvents records the type and id of every event a workflow emits.
type controlEvents struct {
	types []quarrycontracts.EventType
	ids   []quarrycontracts.ID
}

func (r *controlEvents) record(env *testsuite.TestWorkflowEnvironment, a *activities.Activities) {
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ string, evt quarrycontracts.Event) error {
			r.types = append(r.types, evt.Type)
			r.ids = append(r.ids, evt.EventID)
			return nil
		})
}

func (r *controlEvents) count(t quarrycontracts.EventType) int {
	n := 0
	for _, got := range r.types {
		if got == t {
			n++
		}
	}
	return n
}

func (r *controlEvents) distinctIDs(t quarrycontracts.EventType) int {
	seen := map[quarrycontracts.ID]struct{}{}
	for i, got := range r.types {
		if got == t {
			seen[r.ids[i]] = struct{}{}
		}
	}
	return len(seen)
}

func (r *controlEvents) last() quarrycontracts.EventType {
	if len(r.types) == 0 {
		return ""
	}
	return r.types[len(r.types)-1]
}

func batchURLs(n int) []string {
	out := make([]string, 0, n)
	for i := 0; i < n; i++ {
		out = append(out, "https://batch.example/p"+intStr(i))
	}
	return out
}

// TestBatchJobWF_CancelSignalEndsBatchEarly: a cancel before the first URL ends the
// batch before it visits them all, with a terminal run_cancelled.
func TestBatchJobWF_CancelSignalEndsBatchEarly(t *testing.T) {
	env, a := newEnv(t)
	events := &controlEvents{}
	events.record(env, a)

	urls := batchURLs(5)
	pages := 0
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ activities.RunPageInput) (activities.RunPageResult, error) {
			pages++
			return activities.RunPageResult{Status: 200}, nil
		})
	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(SignalCancel, nil)
	}, 0)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_cancel", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Less(t, pages, len(urls), "cancel should end the batch before every URL")
	require.Equal(t, quarrycontracts.EvtRunCancelled, events.last())
	require.Equal(t, 0, events.count(quarrycontracts.EvtRunCompleted))
}

// TestBatchJobWF_PauseThenResumeCompletesAll: a pause holds the batch, a resume
// lets it finish every URL and end with run_completed.
func TestBatchJobWF_PauseThenResumeCompletesAll(t *testing.T) {
	env, a := newEnv(t)
	events := &controlEvents{}
	events.record(env, a)

	urls := batchURLs(3)
	pages := 0
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ activities.RunPageInput) (activities.RunPageResult, error) {
			pages++
			return activities.RunPageResult{Status: 200}, nil
		})
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalPause, nil) }, 0)
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalResume, nil) }, 5*time.Second)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_pause", URLs: urls}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Equal(t, len(urls), pages)
	require.Equal(t, 1, events.count(quarrycontracts.EvtRunPaused))
	require.Equal(t, 1, events.count(quarrycontracts.EvtRunResumed))
	require.Equal(t, quarrycontracts.EvtRunCompleted, events.last())
}

// TestBatchJobWF_PauseTwiceEmitsTwoDistinctEvents: two pauses in one run are two
// run_paused events with two different ids (the pause count is in the id), and
// each pause is released by its own resume.
//
// The order is fixed by the workflow, not by timing. Pause one is delivered
// before the first URL and resume one at 2 s of workflow time. The second pause
// is signalled from inside the first RunPage activity, so the workflow receives
// it before that activity's result and reaches the gate before the next URL
// already paused. Resume two is a delayed callback at 60 s: the test clock only
// moves on while the workflow is blocked, so it fires in the second wait and
// never earlier. A first version signalled the second pause from the
// run_resumed event and never resumed it, so the run waited for ever.
func TestBatchJobWF_PauseTwiceEmitsTwoDistinctEvents(t *testing.T) {
	env, a := newEnv(t)
	events := &controlEvents{}
	events.record(env, a)

	var calls atomic.Int32
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ activities.RunPageInput) (activities.RunPageResult, error) {
			if calls.Add(1) == 1 {
				env.SignalWorkflow(SignalPause, nil)
			}
			return activities.RunPageResult{Status: 200}, nil
		})
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalPause, nil) }, 0)
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalResume, nil) }, 2*time.Second)
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalResume, nil) }, 60*time.Second)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{RunID: "run_batch_twice", URLs: batchURLs(4)}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Equal(t, int32(4), calls.Load(), "every URL is visited")
	require.Equal(t, 2, events.count(quarrycontracts.EvtRunPaused), "one run_paused for each pause")
	require.Equal(t, 2, events.count(quarrycontracts.EvtRunResumed), "one run_resumed for each pause")
	require.Equal(t, 2, events.distinctIDs(quarrycontracts.EvtRunPaused),
		"every run_paused of one run has its own event id")
	require.Equal(t, quarrycontracts.EvtRunCompleted, events.last())
}

// TestCrawlJobWF_PauseUsesAwaitNotTimers: under lifecycle-v1 a paused crawl waits on
// the resume signal with Await; no five-second poll timer is scheduled while paused.
func TestCrawlJobWF_PauseUsesAwaitNotTimers(t *testing.T) {
	env, a := newEnv(t)
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(activities.RunPageResult{Status: 200}, nil)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)

	pollTimers := 0
	env.SetOnTimerScheduledListener(func(_ string, d time.Duration) {
		if d == sleepDuringPause {
			pollTimers++
		}
	})
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalPause, nil) }, 0)
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalResume, nil) }, 12*time.Second)

	env.ExecuteWorkflow(CrawlJobWF, CrawlJobInput{
		RunID:    "run_crawl_await",
		Seeds:    []string{"https://await.example/seed"},
		MaxDepth: 1,
		MaxPages: 10,
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Equal(t, 0, pollTimers, "lifecycle-v1 waits with Await, not the five-second poll")
}

// TestCrawlJobWF_OldHistoryKeepsSleepPolling: an execution that predates
// lifecycle-v1 (the version reads as DefaultVersion) keeps the timer poll it was
// started with, so its replay is unchanged.
func TestCrawlJobWF_OldHistoryKeepsSleepPolling(t *testing.T) {
	env, a := newEnv(t)
	env.OnGetVersion("lifecycle-v1", workflow.DefaultVersion, 1).Return(workflow.DefaultVersion)
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(activities.RunPageResult{Status: 200}, nil)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)

	pollTimers := 0
	env.SetOnTimerScheduledListener(func(_ string, d time.Duration) {
		if d == sleepDuringPause {
			pollTimers++
		}
	})
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalPause, nil) }, 0)
	env.RegisterDelayedCallback(func() { env.SignalWorkflow(SignalResume, nil) }, 12*time.Second)

	env.ExecuteWorkflow(CrawlJobWF, CrawlJobInput{
		RunID:    "run_crawl_old",
		Seeds:    []string{"https://old.example/seed"},
		MaxDepth: 1,
		MaxPages: 10,
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Greater(t, pollTimers, 0, "the pre-version history keeps the timer poll")
}
