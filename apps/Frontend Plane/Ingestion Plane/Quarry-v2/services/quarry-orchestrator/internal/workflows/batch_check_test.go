package workflows

import (
	"context"
	"errors"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// TestBatchJobWF_CountsBlockedAndSendsBatchContext (WS14.S5): with the batch-check-v1
// marker, every page carries a batch crawl context with the respect default, and a
// page the edge refused (a blocklist denial) is counted as blocked and does not fail
// the run.
func TestBatchJobWF_CountsBlockedAndSendsBatchContext(t *testing.T) {
	env, a := newEnv(t)
	env.OnGetVersion(batchCheckVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))

	var contexts []*activities.CrawlContext
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			contexts = append(contexts, in.Crawl)
			if in.URL == "https://blocked.example/page" {
				return activities.RunPageResult{},
					errs.New(errs.CategoryBlocklist, "test", errors.New("denied")).Temporal()
			}
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{
		RunID: "run_batch_check",
		URLs:  []string{"https://ok.example/a", "https://blocked.example/page"},
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError(), "a blocked page does not fail the batch")
	require.Len(t, contexts, 2)
	for _, c := range contexts {
		require.NotNil(t, c, "every page of a checked batch carries the batch context")
		require.Equal(t, activities.CrawlKindBatch, c.Kind)
		require.Equal(t, activities.RobotsRespect, c.Robots)
	}
}

// TestBatchJobWF_OldVersionHistoryHasNoBatchContext (WS14.S5): a history recorded
// before the marker sends no context and counts the refused page as failed, as it did.
func TestBatchJobWF_OldVersionHistoryHasNoBatchContext(t *testing.T) {
	env, a := newEnv(t)
	env.OnGetVersion(batchCheckVersion, workflow.DefaultVersion, 1).Return(workflow.DefaultVersion)

	var contexts []*activities.CrawlContext
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			contexts = append(contexts, in.Crawl)
			return activities.RunPageResult{}, errs.New(errs.CategoryBlocklist, "test", errors.New("denied")).Temporal()
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{
		RunID: "run_batch_old",
		URLs:  []string{"https://blocked.example/one"},
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, contexts, 1)
	require.Nil(t, contexts[0], "a pre-marker history sends no crawl context")
}

// TestBatchJobWF_TerminalCountsRobotsRefusalsApartFromOtherBlocks (WS14.S4): a page the
// edge refused under robots.txt is counted in pages_blocked and in pages_blocked_robots;
// a scope refusal is counted in pages_blocked alone, and neither fails the batch.
func TestBatchJobWF_TerminalCountsRobotsRefusalsApartFromOtherBlocks(t *testing.T) {
	env, a := newEnv(t)
	env.OnGetVersion(batchCheckVersion, workflow.DefaultVersion, 1).Return(workflow.Version(1))
	refusal := func(code string) error {
		body := []byte(`{"error":{"code":"SECURITY_BLOCKED","message":"refused","details":{"denial":{"code":"` + code + `","message":"m","remedy":"r"}}}}`)
		return errs.FromHTTPResponse("run_page", 403, body).Temporal()
	}
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			switch in.URL {
			case "https://robots.example/private":
				return activities.RunPageResult{}, refusal("robots_disallowed")
			case "https://scope.example/x":
				return activities.RunPageResult{}, refusal("external_host_disabled")
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

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{
		RunID: "run_batch_robots",
		URLs:  []string{"https://ok.example/a", "https://robots.example/private", "https://scope.example/x"},
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError(), "a refused page does not fail the batch")
	require.EqualValues(t, 2, completed["pages_blocked"])
	require.EqualValues(t, 1, completed["pages_blocked_robots"])
}

// TestBatchRobotsDefaultsToRespect (WS14.S5): only `ignore` is ignore; anything else,
// including absent, is respect.
func TestBatchRobotsDefaultsToRespect(t *testing.T) {
	require.Equal(t, activities.RobotsRespect, batchRobots(""))
	require.Equal(t, activities.RobotsRespect, batchRobots("respect"))
	require.Equal(t, activities.RobotsRespect, batchRobots("bogus"))
	require.Equal(t, activities.RobotsIgnore, batchRobots("ignore"))
}
