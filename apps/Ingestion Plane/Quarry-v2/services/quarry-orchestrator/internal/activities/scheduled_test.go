package activities

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"
)

// The workflow passes RegisterRunInput to the activity through the SDK's JSON
// data converter, so every field the activity reads must survive a JSON round
// trip. ScheduleID once carried json:"-" and arrived empty on every fire.
func TestRegisterRunInput_SurvivesTheDataConverter(t *testing.T) {
	raw, err := json.Marshal(RegisterRunInput{ScheduleID: "sch_1", RunKey: "run_k", RunID: "run_r", Status: "running", WorkflowID: "wf"})
	require.NoError(t, err)
	var back RegisterRunInput
	require.NoError(t, json.Unmarshal(raw, &back))
	require.Equal(t, "sch_1", back.ScheduleID)
	require.Equal(t, "run_k", back.RunKey)
}

// RegisterScheduledRun posts to the schedule's path, sends no schedule id in the
// body, and reads the job from control's {request_id, data} envelope as well as
// from a bare object.
func TestRegisterScheduledRun_PathBodyAndEnvelope(t *testing.T) {
	for _, tc := range []struct {
		name   string
		answer string
	}{
		{"enveloped", `{"request_id":"req_1","data":{"job_id":"job_1","status":"running"}}`},
		{"bare", `{"job_id":"job_1","status":"running"}`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var mu sync.Mutex
			var gotPath string
			var gotBody map[string]any
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				raw, _ := io.ReadAll(r.Body)
				mu.Lock()
				gotPath = r.URL.Path
				_ = json.Unmarshal(raw, &gotBody)
				mu.Unlock()
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(http.StatusCreated)
				_, _ = w.Write([]byte(tc.answer))
			}))
			t.Cleanup(srv.Close)

			a := New(Config{ControlBaseURL: srv.URL, ControlHMACSecret: "secret"})
			out, err := a.RegisterScheduledRun(context.Background(), RegisterRunInput{
				ScheduleID: "sch_1", RunKey: "run_k", RunID: "run_r", Status: "running", WorkflowID: "wf-1",
			})
			require.NoError(t, err)
			require.Equal(t, "job_1", out.JobID)
			require.Equal(t, "running", out.Status)

			mu.Lock()
			defer mu.Unlock()
			require.Equal(t, "/v1/internal/schedules/sch_1/runs", gotPath)
			require.Equal(t, "run_k", gotBody["run_key"])
			require.Equal(t, "wf-1", gotBody["workflow_id"])
			_, hasSchedule := gotBody["schedule_id"]
			require.False(t, hasSchedule, "the schedule id is in the path, not the body")
		})
	}
}

func TestRegisterScheduledRun_RefusesAnEmptyScheduleID(t *testing.T) {
	a := New(Config{ControlBaseURL: "http://127.0.0.1:1"})
	_, err := a.RegisterScheduledRun(context.Background(), RegisterRunInput{RunKey: "k", RunID: "r", Status: "running"})
	require.Error(t, err)
}
