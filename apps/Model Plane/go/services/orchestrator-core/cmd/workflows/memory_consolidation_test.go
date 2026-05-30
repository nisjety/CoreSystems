package workflows

import (
	"errors"
	"io"
	"log/slog"
	"testing"
	"time"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

func newMaintenanceEnv() (*testsuite.TestWorkflowEnvironment, *activities.Activities) {
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivity(a.QueryMemoryEntriesActivity)
	env.RegisterActivity(a.SummarizeMemoryActivity)
	env.RegisterActivity(a.WriteConsolidatedMemoryActivity)
	return env, a
}

func TestMemoryConsolidationWorkflow_HappyPath(t *testing.T) {
	env, _ := newMaintenanceEnv()

	entries := []activities.MemoryEntry{{
		ID:        "m1",
		OrgID:     "org-1",
		ThreadID:  "thread-1",
		Content:   "first memory",
		CreatedAt: time.Now().UTC(),
	}}
	consolidated := activities.ConsolidationOutput{
		ConsolidatedEntries: []activities.MemoryEntry{{
			ID:        "thread-1-consolidated",
			OrgID:     "org-1",
			ThreadID:  "thread-1",
			Content:   "first memory",
			CreatedAt: time.Now().UTC(),
		}},
		Summary: "consolidated 1 entries into 1 summaries",
	}

	env.OnActivity("QueryMemoryEntriesActivity", mock.Anything, mock.Anything).
		Return(entries, nil).Once()
	env.OnActivity("SummarizeMemoryActivity", mock.Anything, mock.Anything).
		Return(consolidated, nil).Once()
	env.OnActivity("WriteConsolidatedMemoryActivity", mock.Anything, mock.Anything).
		Return(nil).Once()

	env.ExecuteWorkflow(MemoryConsolidationWorkflow, MemoryConsolidationInput{OrgID: "org-1", MaxItems: 10})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	var output MemoryConsolidationOutput
	require.NoError(t, env.GetWorkflowResult(&output))
	require.Equal(t, 1, output.EntriesProcessed)
	require.Equal(t, 1, output.EntriesConsolidated)
	require.Contains(t, output.Summary, "org-1")
}

func TestMemoryConsolidationWorkflow_QueryFailure(t *testing.T) {
	env, _ := newMaintenanceEnv()

	env.OnActivity("QueryMemoryEntriesActivity", mock.Anything, mock.Anything).
		Return([]activities.MemoryEntry(nil), errors.New("search failed")).Once()

	env.ExecuteWorkflow(MemoryConsolidationWorkflow, MemoryConsolidationInput{OrgID: "org-1", MaxItems: 10})

	require.True(t, env.IsWorkflowCompleted())
	require.Error(t, env.GetWorkflowError())
	require.Contains(t, env.GetWorkflowError().Error(), "query memory entries")
}
