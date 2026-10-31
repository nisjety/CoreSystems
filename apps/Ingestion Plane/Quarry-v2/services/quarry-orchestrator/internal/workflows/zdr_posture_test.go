package workflows

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// WS4.H: the workflow passes the schedule's stamp to CheckChange untouched.
func TestChangeMonitorWFPassesZdr(t *testing.T) {
	yes, no := true, false
	for _, tc := range []struct {
		name string
		in   *bool
	}{{"zdr", &yes}, {"standard", &no}, {"unstamped", nil}} {
		t.Run(tc.name, func(t *testing.T) {
			env, a := newEnv(t)
			var seen activities.CheckChangeInput
			env.OnActivity(a.CheckChange, mock.Anything, mock.Anything).Return(
				func(_ context.Context, in activities.CheckChangeInput) (activities.CheckChangeResult, error) {
					seen = in
					return activities.CheckChangeResult{Status: "unchanged", Fingerprint: "blake3:x"}, nil
				}).Once()
			env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

			env.ExecuteWorkflow(ChangeMonitorWF, ChangeMonitorInput{
				RunID: "run_cm_p", OrgID: "org_a", URL: "https://example.com/p", Zdr: tc.in,
			}, a)
			require.True(t, env.IsWorkflowCompleted())
			require.NoError(t, env.GetWorkflowError())
			if tc.in == nil {
				require.Nil(t, seen.Zdr)
			} else {
				require.NotNil(t, seen.Zdr)
				require.Equal(t, *tc.in, *seen.Zdr)
			}
		})
	}
}

// The args quarry-control builds today (no zdr key) and the stamped args must
// both decode into ChangeMonitorInput: an old schedule keeps firing across a
// deploy, and a stamped one decodes the pointer.
func TestChangeMonitorInputDecodesLegacyArgs(t *testing.T) {
	var legacy ChangeMonitorInput
	require.NoError(t, json.Unmarshal([]byte(`{"org_id":"o","url":"https://x","created_by":"u"}`), &legacy))
	require.Nil(t, legacy.Zdr)
	require.Equal(t, "o", legacy.OrgID)

	var stamped ChangeMonitorInput
	require.NoError(t, json.Unmarshal([]byte(`{"org_id":"o","url":"https://x","zdr":true}`), &stamped))
	require.NotNil(t, stamped.Zdr)
	require.True(t, *stamped.Zdr)

	// An unknown future key must not break decoding either (rolling deploys).
	var future ChangeMonitorInput
	require.NoError(t, json.Unmarshal([]byte(`{"org_id":"o","url":"https://x","zdr":false,"other":1}`), &future))
	require.NotNil(t, future.Zdr)
	require.False(t, *future.Zdr)
}
