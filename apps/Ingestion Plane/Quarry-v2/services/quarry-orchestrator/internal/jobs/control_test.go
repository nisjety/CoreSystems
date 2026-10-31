package jobs

// Tests of the job control loop and the dispatcher's compare-and-set (WS15.L2).
// Written in the build phase and compile-checked only; the wave's test phase runs
// them. Control is a small httptest fake; Temporal is the SDK's client mock.

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/api/serviceerror"
	workflowpb "go.temporal.io/api/workflow/v1"
	"go.temporal.io/api/workflowservice/v1"
	"go.temporal.io/sdk/mocks"
)

// fakeControlAPI serves the internal job routes the loop calls and records what it
// was sent. A finalize removes the job from the running list, as control does.
type fakeControlAPI struct {
	mu       sync.Mutex
	pending  []map[string]any
	running  []map[string]any
	acks     []map[string]any
	finals   []map[string]any
	finalIDs []string
	conflict bool
}

func (f *fakeControlAPI) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/jobs/pending-control", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		writeFakeJSON(w, f.pending)
	})
	mux.HandleFunc("/v1/internal/jobs/running", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		writeFakeJSON(w, f.running)
	})
	mux.HandleFunc("/v1/internal/jobs/", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		raw, _ := io.ReadAll(r.Body)
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		switch {
		case strings.HasSuffix(r.URL.Path, "/control-ack"):
			f.acks = append(f.acks, body)
		case strings.HasSuffix(r.URL.Path, "/finalize"):
			id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/v1/internal/jobs/"), "/finalize")
			f.finals = append(f.finals, body)
			f.finalIDs = append(f.finalIDs, id)
			kept := f.running[:0]
			for _, j := range f.running {
				if j["id"] != id {
					kept = append(kept, j)
				}
			}
			f.running = kept
		}
		writeFakeJSON(w, map[string]any{})
	})
	mux.HandleFunc("/v1/jobs/", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		if f.conflict {
			w.WriteHeader(http.StatusConflict)
			_, _ = w.Write([]byte(`{"error":{"code":"CONFLICT","message":"the job changed since it was read"}}`))
			return
		}
		writeFakeJSON(w, map[string]any{})
	})
	return mux
}

func writeFakeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(v)
}

func newControlManager(t *testing.T, api *fakeControlAPI) (*Manager, *mocks.Client) {
	t.Helper()
	server := httptest.NewServer(api.handler())
	t.Cleanup(server.Close)
	temporal := &mocks.Client{}
	m := New(temporal, Config{
		ControlBaseURL:    server.URL,
		ControlHMACSecret: "test-secret",
		ControlLoop:       true,
	})
	return m, temporal
}

func pendingJob(id, desired string, seq int64, workflowID string) map[string]any {
	return map[string]any{
		"id": id, "kind": "crawl", "status": "running", "org_id": "org_a",
		"workflow_id": workflowID, "desired_state": desired, "desired_seq": seq, "applied_seq": 0,
	}
}

// TestApplyPending_SignalsAndAcks: a pending pause is sent as the pause signal and
// acknowledged at its sequence number.
func TestApplyPending_SignalsAndAcks(t *testing.T) {
	api := &fakeControlAPI{pending: []map[string]any{pendingJob("job_a", "paused", 1, "quarry-job-job_a")}}
	m, temporal := newControlManager(t, api)
	temporal.On("SignalWorkflow", mock.Anything, "quarry-job-job_a", "", "pause", mock.Anything).Return(nil)

	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: time.Minute}
	loop.applyPending(context.Background())

	temporal.AssertExpectations(t)
	require.Len(t, api.acks, 1)
	require.EqualValues(t, 1, api.acks[0]["applied_seq"])
}

// TestApplyPending_NotFoundCountsAsApplied: a workflow that already closed is not an
// error; the request is acknowledged and the sweep corrects the status.
func TestApplyPending_NotFoundCountsAsApplied(t *testing.T) {
	api := &fakeControlAPI{pending: []map[string]any{pendingJob("job_b", "cancelled", 2, "quarry-job-job_b")}}
	m, temporal := newControlManager(t, api)
	temporal.On("SignalWorkflow", mock.Anything, "quarry-job-job_b", "", "cancel", mock.Anything).
		Return(serviceerror.NewNotFound("workflow execution already completed"))

	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: time.Minute}
	loop.applyPending(context.Background())

	require.Len(t, api.acks, 1, "a closed workflow counts as applied")
	require.Empty(t, api.finals, "no forced finalize for a closed workflow")
}

// TestEscalate_TerminatesAfterGrace: a cancel the workflow keeps refusing is
// terminated and finalized as cancelled and forced once the grace has passed.
func TestEscalate_TerminatesAfterGrace(t *testing.T) {
	api := &fakeControlAPI{
		pending: []map[string]any{pendingJob("job_c", "cancelled", 3, "quarry-job-job_c")},
		running: []map[string]any{pendingJob("job_c", "cancelled", 3, "quarry-job-job_c")},
	}
	m, temporal := newControlManager(t, api)
	temporal.On("SignalWorkflow", mock.Anything, "quarry-job-job_c", "", "cancel", mock.Anything).
		Return(errors.New("transient signal failure"))
	temporal.On("TerminateWorkflow", mock.Anything, "quarry-job-job_c", "", "cancel grace elapsed").Return(nil)

	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: 0}
	loop.applyPending(context.Background())

	temporal.AssertExpectations(t)
	require.Len(t, api.finals, 1)
	require.Equal(t, "cancelled", api.finals[0]["status"])
	require.Equal(t, true, api.finals[0]["forced"])
}

// TestSweep_TimedOutBecomesTypedTerminal: a workflow that closed TIMED_OUT ends the
// job as timed_out with the execution timeout named.
func TestSweep_TimedOutBecomesTypedTerminal(t *testing.T) {
	api := &fakeControlAPI{running: []map[string]any{{
		"id": "job_d", "kind": "scrape", "status": "running", "org_id": "org_a",
		"workflow_id": "quarry-job-job_d", "params": map[string]any{"url": "https://d.example"},
	}}}
	m, temporal := newControlManager(t, api)
	temporal.On("DescribeWorkflowExecution", mock.Anything, "quarry-job-job_d", "").Return(
		&workflowservice.DescribeWorkflowExecutionResponse{
			WorkflowExecutionInfo: &workflowpb.WorkflowExecutionInfo{
				Status: enumspb.WORKFLOW_EXECUTION_STATUS_TIMED_OUT,
			},
		}, nil)

	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: time.Minute}
	loop.sweep(context.Background())

	temporal.AssertExpectations(t)
	require.Equal(t, []string{"job_d"}, api.finalIDs)
	require.Equal(t, "timed_out", api.finals[0]["status"])
	require.Equal(t, "execution", api.finals[0]["timeout_type"])
}

// TestSweep_RepeatedPassIsNoop: once finalized the job leaves the running list, so
// a second pass finalizes nothing.
func TestSweep_RepeatedPassIsNoop(t *testing.T) {
	api := &fakeControlAPI{running: []map[string]any{{
		"id": "job_e", "kind": "crawl", "status": "running", "org_id": "org_a",
		"workflow_id": "quarry-job-job_e",
	}}}
	m, temporal := newControlManager(t, api)
	temporal.On("DescribeWorkflowExecution", mock.Anything, "quarry-job-job_e", "").Return(
		&workflowservice.DescribeWorkflowExecutionResponse{
			WorkflowExecutionInfo: &workflowpb.WorkflowExecutionInfo{
				Status: enumspb.WORKFLOW_EXECUTION_STATUS_FAILED,
			},
		}, nil)

	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: time.Minute}
	loop.sweep(context.Background())
	loop.sweep(context.Background())

	require.Len(t, api.finals, 1, "the second pass sees no running job")
	require.Equal(t, "failed", api.finals[0]["status"])
}

// TestMarkRunning_ConflictTerminatesStartedWorkflow: when control answers 409 to
// the running mark, the dispatcher reports the job as changed, and the workflow it
// started is terminated.
func TestMarkRunning_ConflictTerminatesStartedWorkflow(t *testing.T) {
	api := &fakeControlAPI{conflict: true}
	m, temporal := newControlManager(t, api)

	err := m.markRunning(context.Background(), job{ID: "job_f"}, "run_f", "quarry-job-job_f", time.Time{})
	require.ErrorIs(t, err, errJobChanged)

	temporal.On("TerminateWorkflow", mock.Anything, "quarry-job-job_f", "", "job cancelled before it started").Return(nil)
	m.stopStarted(context.Background(), "quarry-job-job_f")
	temporal.AssertExpectations(t)
}

// TestExecutionTimeoutFollowsControlLoop: the legacy 30-minute constant stays while
// the loop is off; with it on, the per-kind timeout is used.
func TestExecutionTimeoutFollowsControlLoop(t *testing.T) {
	off := New(&mocks.Client{}, Config{ControlLoop: false})
	require.Equal(t, legacyExecutionTimeout, off.executionTimeout(job{Kind: "scrape"}))

	on := New(&mocks.Client{}, Config{ControlLoop: true})
	require.Equal(t, 300*time.Second, on.executionTimeout(job{Kind: "scrape"}))
}
