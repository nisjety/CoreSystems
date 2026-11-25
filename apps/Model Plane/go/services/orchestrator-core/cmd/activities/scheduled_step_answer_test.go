package activities_test

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
)

// TestExecuteScheduledStepActivityNeverReturnsTheAnswerAsItsResult pins F-f03-05: the activity's result is
// recorded in Temporal history, so a scheduled step's answer, even when execution-core still sends it in its
// response, never becomes the result's output or metadata. The receipt id and the counts still come through,
// and the step still completes.
func TestExecuteScheduledStepActivityNeverReturnsTheAnswerAsItsResult(t *testing.T) {
	const answer = "the model's answer to the scheduled step"
	fakeExecution := &fakeExecutionCore{
		executeScheduledStepResp: &mpv1.ExecuteScheduledStepResponse{
			Status:          "completed",
			ReceiptId:       "receipt-step-1",
			Output:          answer,
			RoundsExecuted:  2,
			WithheldEffects: 1,
		},
	}
	conn := newExecutionCoreConn(t, fakeExecution)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{ExecutionCore: conn})
	a.SetScheduledStepDecisionAuthorizer(&fakeScheduledStepDecisionAuthorizer{token: "control-step-decision"})
	intent := activities.ScheduledStepExecutionIntent{
		OrgID:          "org-1",
		SpaceRef:       "space-1",
		SubjectID:      "user-1",
		RunID:          "run-1",
		ThreadID:       "thread-1",
		ScheduleID:     "schedule-1",
		FireKey:        "fire-1",
		TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		StepID:         "run-1:step:0",
		StepIndex:      0,
		PolicyDigest:   "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey: "fire-1:step:0",
	}

	step, err := a.ExecuteScheduledStepActivity(context.Background(), intent)
	require.NoError(t, err)
	assert.Empty(t, step.Output, "the answer is never the activity result")
	assert.NotContains(t, fmt.Sprint(step.Metadata), answer, "the answer is never copied into metadata")
	assert.True(t, step.Completed, "the step still completes")
	assert.Equal(t, "receipt-step-1", step.Metadata["receipt_id"])
	assert.Equal(t, 2, step.RoundsExecuted)
	assert.Equal(t, 1, step.WithheldEffects)
}
