package jobs

// The job control loop (WS15.L2). Control records a pause, resume or cancel as a
// desired state on the job record and never talks to Temporal. This loop, which
// runs in the orchestrator, does the rest:
//
//   - every 2 s it reads the pending requests from control, signals the workflow
//     and acknowledges each one (a workflow that no longer exists counts as applied);
//   - a cancel the workflow has not applied within QUARRY_CANCEL_GRACE_S (120 s)
//     terminates the workflow and finalizes the job as cancelled, forced;
//   - every 30 s it lists the running jobs and finalizes the ones whose workflow
//     ended in a state the workflow itself could not report: timed out (the
//     execution timeout), failed, terminated, or canceled.
//
// The loop runs only when QUARRY_JOB_CONTROL_LOOP is on. Off, nothing signals and
// the dispatcher keeps its 30-minute execution timeout.

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/rs/zerolog/log"
	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/api/serviceerror"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/timeouts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/workflows"
)

// Control loop environment and cadence.
const (
	// EnvControlLoop turns the loop on (1, true, yes). It also turns on the
	// per-kind execution timeouts in the dispatcher, which go with it.
	EnvControlLoop = "QUARRY_JOB_CONTROL_LOOP"
	// EnvCancelGraceS is how long an unapplied cancel waits before it is forced.
	EnvCancelGraceS = "QUARRY_CANCEL_GRACE_S"

	defaultCancelGrace = 120 * time.Second
	controlEvery       = 2 * time.Second
	sweepEvery         = 30 * time.Second
	pendingPageLimit   = 100
	runningPageLimit   = 200
)

// ControlLoopEnabled reports whether QUARRY_JOB_CONTROL_LOOP is on.
func ControlLoopEnabled() bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv(EnvControlLoop))) {
	case "1", "true", "yes":
		return true
	}
	return false
}

// cancelGrace reads QUARRY_CANCEL_GRACE_S; an unset or bad value is the default.
func cancelGrace() time.Duration {
	n, err := strconv.Atoi(strings.TrimSpace(os.Getenv(EnvCancelGraceS)))
	if err != nil || n <= 0 {
		return defaultCancelGrace
	}
	return time.Duration(n) * time.Second
}

// controlJob is the part of control's job record the loop reads.
type controlJob struct {
	ID           string         `json:"id"`
	Kind         string         `json:"kind"`
	Status       string         `json:"status"`
	OrgID        string         `json:"org_id"`
	WorkflowID   string         `json:"workflow_id"`
	DesiredState string         `json:"desired_state"`
	DesiredSeq   int64          `json:"desired_seq"`
	AppliedSeq   int64          `json:"applied_seq"`
	Params       map[string]any `json:"params"`
}

// pendingMark is when the loop first saw a request at its sequence number, so the
// grace period of a cancel is measured from the first attempt to apply it.
type pendingMark struct {
	seq   int64
	since time.Time
}

// controlLoop holds the loop's own state. It is created by RunControl and used
// from one goroutine only.
type controlLoop struct {
	m       *Manager
	pending map[string]pendingMark
	grace   time.Duration
}

// RunControl blocks until ctx is cancelled, applying pending control requests and
// sweeping running jobs. Errors are logged; the loop keeps going.
func (m *Manager) RunControl(ctx context.Context) {
	loop := &controlLoop{m: m, pending: map[string]pendingMark{}, grace: cancelGrace()}
	log.Info().Dur("grace", loop.grace).Msg("job control loop started")
	tick := time.NewTicker(controlEvery)
	defer tick.Stop()
	lastSweep := time.Time{}
	for {
		select {
		case <-ctx.Done():
			log.Info().Msg("job control loop stopped")
			return
		case <-tick.C:
			loop.applyPending(ctx)
			if time.Since(lastSweep) >= sweepEvery {
				loop.sweep(ctx)
				lastSweep = time.Now()
			}
		}
	}
}

// signalFor maps a desired state to the workflow signal that carries it.
func signalFor(desired string) (string, bool) {
	switch desired {
	case "paused":
		return workflows.SignalPause, true
	case "running":
		return workflows.SignalResume, true
	case "cancelled":
		return workflows.SignalCancel, true
	}
	return "", false
}

// applyPending signals and acknowledges every request control has not seen applied.
func (l *controlLoop) applyPending(ctx context.Context) {
	var pending []controlJob
	if err := l.m.controlCall(ctx, http.MethodGet,
		fmt.Sprintf("/v1/internal/jobs/pending-control?limit=%d", pendingPageLimit), nil, &pending); err != nil {
		log.Warn().Err(err).Msg("job control: pending read failed")
		return
	}
	now := time.Now()
	next := make(map[string]pendingMark, len(pending))
	for _, j := range pending {
		mark, seen := l.pending[j.ID]
		if !seen || mark.seq != j.DesiredSeq {
			mark = pendingMark{seq: j.DesiredSeq, since: now}
		}
		next[j.ID] = mark

		signal, ok := signalFor(j.DesiredState)
		if !ok {
			continue
		}
		applied, err := l.signal(ctx, j, signal)
		if err != nil {
			log.Warn().Err(err).Str("job_id", j.ID).Str("signal", signal).Msg("job control: signal failed")
			if j.DesiredState == "cancelled" && now.Sub(mark.since) >= l.grace {
				l.escalate(ctx, j)
				delete(next, j.ID)
			}
			continue
		}
		if !applied {
			continue
		}
		if err := l.m.controlCall(ctx, http.MethodPost,
			fmt.Sprintf("/v1/internal/jobs/%s/control-ack", j.ID),
			map[string]any{"applied_seq": j.DesiredSeq}, nil); err != nil {
			log.Warn().Err(err).Str("job_id", j.ID).Msg("job control: ack failed")
			continue
		}
		delete(next, j.ID)
	}
	l.pending = next
}

// signal sends one signal. A job with no workflow has nothing to signal, and a
// workflow that no longer exists (it closed) counts as applied: the sweep corrects
// the job's status if the close was not recorded.
func (l *controlLoop) signal(ctx context.Context, j controlJob, signal string) (bool, error) {
	if j.WorkflowID == "" {
		return true, nil
	}
	err := l.m.temporal.SignalWorkflow(ctx, j.WorkflowID, "", signal, nil)
	if err == nil {
		return true, nil
	}
	var notFound *serviceerror.NotFound
	if errors.As(err, &notFound) {
		return true, nil
	}
	return false, err
}

// escalate ends a cancel the workflow would not apply: it terminates the workflow
// and finalizes the job as cancelled and forced. The terminate is best effort; the
// finalize is what control records.
func (l *controlLoop) escalate(ctx context.Context, j controlJob) {
	if j.WorkflowID != "" {
		if err := l.m.temporal.TerminateWorkflow(ctx, j.WorkflowID, "", "cancel grace elapsed"); err != nil {
			var notFound *serviceerror.NotFound
			if !errors.As(err, &notFound) {
				log.Warn().Err(err).Str("job_id", j.ID).Msg("job control: terminate failed")
			}
		}
	}
	// org_id: control refuses a finalize that names an org other than the job's (D2-050).
	body := map[string]any{"status": "cancelled", "forced": true, "finished_at": time.Now().UnixMilli(), "org_id": j.OrgID}
	if err := l.m.controlCall(ctx, http.MethodPost,
		fmt.Sprintf("/v1/internal/jobs/%s/finalize", j.ID), body, nil); err != nil {
		log.Warn().Err(err).Str("job_id", j.ID).Msg("job control: forced finalize failed")
		return
	}
	// The desired state is applied by termination: acknowledge it so it is not read again.
	if err := l.m.controlCall(ctx, http.MethodPost,
		fmt.Sprintf("/v1/internal/jobs/%s/control-ack", j.ID),
		map[string]any{"applied_seq": j.DesiredSeq}, nil); err != nil {
		log.Warn().Err(err).Str("job_id", j.ID).Msg("job control: ack after forced cancel failed")
	}
	log.Info().Str("job_id", j.ID).Msg("job control: cancel forced after grace")
}

// sweep finalizes the running jobs whose workflow has closed in a state the
// workflow itself did not report. COMPLETED is left to the workflow's own
// run_completed event.
func (l *controlLoop) sweep(ctx context.Context) {
	var running []controlJob
	if err := l.m.controlCall(ctx, http.MethodGet,
		fmt.Sprintf("/v1/internal/jobs/running?limit=%d", runningPageLimit), nil, &running); err != nil {
		log.Warn().Err(err).Msg("job control: running read failed")
		return
	}
	for _, j := range running {
		if j.WorkflowID == "" {
			continue
		}
		desc, err := l.m.temporal.DescribeWorkflowExecution(ctx, j.WorkflowID, "")
		if err != nil {
			continue
		}
		info := desc.GetWorkflowExecutionInfo()
		body := map[string]any{"finished_at": time.Now().UnixMilli(), "org_id": j.OrgID}
		if ct := info.GetCloseTime(); ct != nil {
			body["finished_at"] = ct.AsTime().UnixMilli()
		}
		switch info.GetStatus() {
		case enumspb.WORKFLOW_EXECUTION_STATUS_TIMED_OUT:
			body["status"] = "timed_out"
			body["timeout_type"] = "execution"
			body["limit_s"] = int64(timeouts.ExecutionTimeoutFor(j.Kind, j.Params).Seconds())
		case enumspb.WORKFLOW_EXECUTION_STATUS_FAILED:
			body["status"] = "failed"
		case enumspb.WORKFLOW_EXECUTION_STATUS_TERMINATED:
			body["status"] = "cancelled"
			body["forced"] = true
		case enumspb.WORKFLOW_EXECUTION_STATUS_CANCELED:
			body["status"] = "cancelled"
			body["forced"] = false
		default:
			continue
		}
		if err := l.m.controlCall(ctx, http.MethodPost,
			fmt.Sprintf("/v1/internal/jobs/%s/finalize", j.ID), body, nil); err != nil {
			log.Warn().Err(err).Str("job_id", j.ID).Msg("job control: sweep finalize failed")
		}
	}
}

// controlCall sends one signed request to control's internal job routes. A body
// is encoded as JSON; out, when set, receives the decoded JSON answer. Any non-2xx
// answer is an error that names the status.
func (m *Manager) controlCall(ctx context.Context, method, path string, body any, out any) error {
	var raw []byte
	if body != nil {
		var err error
		raw, err = json.Marshal(body)
		if err != nil {
			return err
		}
	}
	req, err := http.NewRequestWithContext(ctx, method,
		strings.TrimRight(m.cfg.ControlBaseURL, "/")+path, bytes.NewReader(raw))
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	if err := controlauth.Sign(req, raw, m.cfg.ControlHMACSecret); err != nil {
		return err
	}
	resp, err := m.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("control %d: %s", resp.StatusCode, truncate(string(data), 200))
	}
	if out == nil || len(data) == 0 {
		return nil
	}
	var enveloped struct {
		Data json.RawMessage `json:"data"`
	}
	if err := json.Unmarshal(data, out); err == nil {
		return nil
	}
	if err := json.Unmarshal(data, &enveloped); err != nil || len(enveloped.Data) == 0 {
		return fmt.Errorf("decode control answer: %w", err)
	}
	return json.Unmarshal(enveloped.Data, out)
}
