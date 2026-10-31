package activities

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// PreflightInput is the fire-time check a scheduled job runs before it starts.
type PreflightInput struct {
	ScheduleID string `json:"schedule_id"`
	OrgID      string `json:"org_id"`
}

// PreflightResult is the edge's answer. S2 always answers allow with the
// posture "unchecked" and the entitlement "floor"; S4 fills both in.
type PreflightResult struct {
	Allow       bool   `json:"allow"`
	Reason      string `json:"reason,omitempty"`
	Posture     string `json:"posture,omitempty"`
	Entitlement string `json:"entitlement,omitempty"`
}

// PreflightFire asks the edge whether a scheduled fire may run. The call uses
// the runtime bearer token and the run-binding signature over
// org_id + "\n" + "schedule:<id>", the same binding run_page uses, so a leaked
// token cannot ask about another tenant's schedule.
func (a *Activities) PreflightFire(ctx context.Context, in PreflightInput) (PreflightResult, error) {
	const op = "activities.PreflightFire"
	body, err := json.Marshal(in)
	if err != nil {
		return PreflightResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := strings.TrimRight(a.cfg.RuntimeBaseURL, "/") + "/v1/internal/schedules/preflight"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return PreflightResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	setAuth(req, a.cfg.RuntimeAuthToken)
	if a.cfg.EdgeRunSecret != "" {
		req.Header.Set("X-Quarry-Run-Sig", runBindingSig(a.cfg.EdgeRunSecret, in.OrgID, "schedule:"+in.ScheduleID))
	}

	resp, err := a.http.Do(req)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return PreflightResult{}, err
		}
		return PreflightResult{}, errs.New(errs.CategoryNetwork, op, err).Temporal()
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return PreflightResult{}, errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	var out PreflightResult
	if err := json.Unmarshal(respBody, &out); err != nil {
		return PreflightResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode response: %w", err)).Temporal()
	}
	return out, nil
}

// RegisterRunInput records one fire with control. RunKey is the fire's
// idempotency key within its schedule; RunID is the run timeline the fire's
// events use.
//
// ScheduleID must survive the JSON the SDK's data converter makes between the
// workflow and the activity, so it has a tag of its own. (It was once json:"-" to
// keep it out of the request body, and the activity then saw an empty id on
// every fire.) The request body is registerRunBody, which leaves it out: the
// schedule id is in the URL path.
type RegisterRunInput struct {
	ScheduleID string `json:"schedule_id"`
	RunKey     string `json:"run_key"`
	RunID      string `json:"run_id"`
	Status     string `json:"status"`
	Reason     string `json:"reason,omitempty"`
	WorkflowID string `json:"workflow_id,omitempty"`
}

// registerRunBody is the request body of POST /v1/internal/schedules/{id}/runs.
// Org, kind and params are not sent: control takes them from the schedule row.
type registerRunBody struct {
	RunKey     string `json:"run_key"`
	RunID      string `json:"run_id"`
	Status     string `json:"status"`
	Reason     string `json:"reason,omitempty"`
	WorkflowID string `json:"workflow_id,omitempty"`
}

// RegisterRunResult is the job control created or found for the fire.
type RegisterRunResult struct {
	JobID  string `json:"job_id"`
	Status string `json:"status"`
}

// RegisterScheduledRun records a fire as a control job. The call is HMAC signed
// with the control secret. Control answers the existing job when the same run
// key comes back, so a retried activity does not create a second job.
func (a *Activities) RegisterScheduledRun(ctx context.Context, in RegisterRunInput) (RegisterRunResult, error) {
	const op = "activities.RegisterScheduledRun"
	if in.ScheduleID == "" {
		return RegisterRunResult{}, errs.New(errs.CategoryValidation, op, errors.New("schedule id is required")).Temporal()
	}
	body, err := json.Marshal(registerRunBody{
		RunKey: in.RunKey, RunID: in.RunID, Status: in.Status, Reason: in.Reason, WorkflowID: in.WorkflowID,
	})
	if err != nil {
		return RegisterRunResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := fmt.Sprintf("%s/v1/internal/schedules/%s/runs", strings.TrimRight(a.cfg.ControlBaseURL, "/"), in.ScheduleID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return RegisterRunResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", "sched:"+in.ScheduleID+":"+in.RunKey)
	setAuth(req, a.cfg.ControlAuthToken)
	if err := controlauth.Sign(req, body, a.cfg.ControlHMACSecret); err != nil {
		return RegisterRunResult{}, errs.New(errs.CategoryNetwork, op, err).Temporal()
	}

	resp, err := a.http.Do(req)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return RegisterRunResult{}, err
		}
		return RegisterRunResult{}, errs.New(errs.CategoryNetwork, op, err).Temporal()
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return RegisterRunResult{}, errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	// Control answers every route in the {request_id, data} envelope. A bare
	// object is read too, so a stub or an older control keeps working.
	var out RegisterRunResult
	var wrapped struct {
		Data json.RawMessage `json:"data"`
	}
	payload := respBody
	if err := json.Unmarshal(respBody, &wrapped); err == nil && len(wrapped.Data) > 0 {
		payload = wrapped.Data
	}
	if err := json.Unmarshal(payload, &out); err != nil {
		return RegisterRunResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode response: %w", err)).Temporal()
	}
	if out.JobID == "" {
		return RegisterRunResult{}, errs.New(errs.CategoryValidation, op, errors.New("control returned no job id")).Temporal()
	}
	return out, nil
}
