package activities_test

import (
	"context"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
)

// TestExecuteScheduledStepActivityStatesTheExtSkillsWordOnTheRequest pins P2-ec-01-rel1: the
// ext.skills word the step intent carries reaches execution-core on ExecuteScheduledStepRequest,
// and an intent with no word sends the empty word, which execution-core reads as not enforced.
func TestExecuteScheduledStepActivityStatesTheExtSkillsWordOnTheRequest(t *testing.T) {
	for _, word := range []string{"", "none", "approved_only", "any"} {
		fakeExecution := &fakeExecutionCore{
			executeScheduledStepResp: &mpv1.ExecuteScheduledStepResponse{
				Status:    "completed",
				ReceiptId: "receipt-step-1",
				Output:    "scheduled result",
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
			ExtSkills:      word,
		}

		_, err := a.ExecuteScheduledStepActivity(context.Background(), intent)
		require.NoError(t, err, "word %q", word)
		require.NotNil(t, fakeExecution.executeScheduledStepReq, "word %q", word)
		assert.Equal(t, word, fakeExecution.executeScheduledStepReq.GetExtSkills(), "word %q", word)
		assert.Equal(t, "control-step-decision", fakeExecution.executeScheduledStepReq.GetControlDecisionToken(),
			"the word is not part of the Control decision, which is forwarded unchanged")
	}
}
