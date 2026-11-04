package schedules

// The schedule request loop (WS15.S3, X-9 / D14 option 2). quarry-control holds
// no Temporal client: a tenant's trigger or backfill is a schedule_requests row.
// Every 2 s this loop claims open rows (FOR UPDATE SKIP LOCKED on control's side,
// so two orchestrators never take the same row), asks Temporal to trigger or
// backfill the schedule and posts the outcome back.
//
// A trigger or backfill goes through the Temporal schedule, so the fire is the
// schedule's own action: a scrape, crawl or batch fire runs ScheduledJobWF with
// the same fire-time preflight (PreflightFire) as a cron fire. A change-monitor
// fire gets its preflight in WS15.S4 (monitor-preflight-v1).
//
// Outcomes: Temporal accepted the call -> done; the schedule is not in Temporal
// (never materialised, or reaped) -> rejected; any other error -> retry (the row
// goes back to pending) until the third attempt, then failed. A row whose worker
// died is claimed again when its lease runs out, so a restart loses nothing; the
// cost is at-least-once: a worker that dies after Temporal accepted the call but
// before it posted the result fires that request again.
//
// OFF BY DEFAULT: QUARRY_SCHEDULE_REQUESTS=1 starts the loop (cmd/orchestrator).
// It only finds rows when control's QUARRY_CONTROL__SCHEDULE_REQUESTS is on too.

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

	"github.com/rs/zerolog"
	"go.temporal.io/api/serviceerror"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
)

// EnvScheduleRequests starts the request loop when truthy (1, true, yes, on).
const EnvScheduleRequests = "QUARRY_SCHEDULE_REQUESTS"

// RequestsEnabled reports whether EnvScheduleRequests is on. Default off.
func RequestsEnabled() bool {
	return requestsEnabled(os.Getenv)
}

func requestsEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv(EnvScheduleRequests))) {
	case "1", "true", "yes", "on":
		return true
	}
	return false
}

const (
	requestInterval    = 2 * time.Second
	requestBatch       = 10
	requestLeaseS      = 60
	requestMaxAttempts = 3
	requestCallTimeout = 15 * time.Second

	requestClaimPath  = "/v1/internal/schedules/requests/claim"
	requestResultPath = "/v1/internal/schedules/requests/%s/result"
)

// requestTemporal is the part of the Manager the loop drives; tests replace it.
type requestTemporal interface {
	Trigger(ctx context.Context, scheduleID string) error
	Backfill(ctx context.Context, scheduleID string, start, end time.Time) error
}

// claimedRequest mirrors control's claimedRequestView.
type claimedRequest struct {
	ID         string         `json:"id"`
	ScheduleID string         `json:"schedule_id"`
	OrgID      string         `json:"org_id"`
	Action     string         `json:"action"`
	Params     map[string]any `json:"params,omitempty"`
	Attempts   int            `json:"attempts"`
	LeaseUntil int64          `json:"lease_until"`
}

// requestOutcome is the body posted to the result route.
type requestOutcome struct {
	Worker string         `json:"worker"`
	Status string         `json:"status"`
	Result map[string]any `json:"result,omitempty"`
	Error  string         `json:"error,omitempty"`
}

type requestRunner struct {
	cfg      Config
	temporal requestTemporal
	worker   string
	interval time.Duration
	getenv   func(string) string
	now      func() time.Time
	log      zerolog.Logger
}

// RunRequests runs the claim loop until ctx ends. Start it only when
// RequestsEnabled() is true.
func (m *Manager) RunRequests(ctx context.Context) error {
	return m.newRequestRunner(m).run(ctx)
}

func (m *Manager) newRequestRunner(t requestTemporal) *requestRunner {
	return &requestRunner{
		cfg:      m.cfg,
		temporal: t,
		worker:   requestWorkerName(),
		interval: requestInterval,
		getenv:   os.Getenv,
		now:      time.Now,
		log:      m.log.With().Str("loop", "schedule-requests").Logger(),
	}
}

// requestWorkerName names this process in claimed_by: host and pid.
func requestWorkerName() string {
	host, err := os.Hostname()
	if err != nil || host == "" {
		host = "orchestrator"
	}
	name := host + "-" + strconv.Itoa(os.Getpid())
	if len(name) > 128 {
		name = name[:128]
	}
	return name
}

func (r *requestRunner) run(ctx context.Context) error {
	t := time.NewTicker(r.interval)
	defer t.Stop()
	for {
		r.tick(ctx)
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-t.C:
		}
	}
}

// tick claims one batch and applies it.
func (r *requestRunner) tick(ctx context.Context) {
	claimed, err := r.claim(ctx)
	if err != nil {
		if ctx.Err() == nil {
			r.log.Warn().Err(err).Msg("schedule request claim failed")
		}
		return
	}
	for _, req := range claimed {
		if ctx.Err() != nil {
			// The lease runs out and another pass claims the rest.
			return
		}
		out := r.apply(ctx, req)
		if err := r.postResult(ctx, req.ID, out); err != nil {
			r.log.Warn().Err(err).Str("request_id", req.ID).Str("status", out.Status).
				Msg("schedule request result not recorded; the lease will expire and the row is claimed again")
		}
	}
}

// apply runs one request against Temporal and decides its outcome.
func (r *requestRunner) apply(ctx context.Context, req claimedRequest) requestOutcome {
	out := requestOutcome{Worker: r.worker}
	if req.ScheduleID == "" {
		out.Status, out.Error = "rejected", "request has no schedule id"
		return out
	}
	callCtx, cancel := context.WithTimeout(ctx, requestCallTimeout)
	defer cancel()

	var err error
	switch req.Action {
	case "trigger":
		err = r.temporal.Trigger(callCtx, req.ScheduleID)
	case "backfill":
		start, end, perr := backfillWindow(req.Params)
		if perr != nil {
			out.Status, out.Error = "rejected", perr.Error()
			return out
		}
		err = r.temporal.Backfill(callCtx, req.ScheduleID, start, end)
	default:
		out.Status, out.Error = "rejected", fmt.Sprintf("unknown action %q", req.Action)
		return out
	}

	switch {
	case err == nil:
		out.Status = "done"
		out.Result = map[string]any{
			"schedule_id":  req.ScheduleID,
			"action":       req.Action,
			"submitted_at": r.now().UTC().Format(time.RFC3339),
		}
	case isScheduleNotFound(err):
		out.Status, out.Error = "rejected", "schedule not materialised yet"
	case req.Attempts >= requestMaxAttempts:
		out.Status, out.Error = "failed", "temporal: "+err.Error()
	default:
		out.Status, out.Error = "retry", "temporal: "+err.Error()
	}
	r.log.Info().Str("request_id", req.ID).Str("schedule_id", req.ScheduleID).Str("org_id", req.OrgID).
		Str("action", req.Action).Int("attempt", req.Attempts).Str("status", out.Status).Msg("schedule request applied")
	return out
}

func isScheduleNotFound(err error) bool {
	var nf *serviceerror.NotFound
	return errors.As(err, &nf)
}

// backfillWindow reads the window control stored (RFC 3339, UTC).
func backfillWindow(params map[string]any) (time.Time, time.Time, error) {
	read := func(k string) (time.Time, error) {
		s, _ := params[k].(string)
		if s == "" {
			return time.Time{}, fmt.Errorf("backfill: %s missing", k)
		}
		v, err := time.Parse(time.RFC3339, s)
		if err != nil {
			return time.Time{}, fmt.Errorf("backfill: %s: %v", k, err)
		}
		return v, nil
	}
	start, err := read("start_at")
	if err != nil {
		return time.Time{}, time.Time{}, err
	}
	end, err := read("end_at")
	if err != nil {
		return time.Time{}, time.Time{}, err
	}
	if !end.After(start) {
		return time.Time{}, time.Time{}, errors.New("backfill: end_at must be after start_at")
	}
	return start, end, nil
}

// signingSecret is the control-internal secret (D2-050): control verifies every
// /v1/internal/schedules/ path with it. controlauth.IsControlInternalPath does
// not list the request routes yet, so Sign would otherwise pick the shared
// secret; passing the control secret as the one to use keeps both sides equal.
// Unset, it falls back to the shared secret, as controlauth does.
func (r *requestRunner) signingSecret() string {
	if s := strings.TrimSpace(r.getenv(controlauth.ControlInternalSecretEnv)); s != "" {
		return s
	}
	return r.cfg.ControlHMACSecret
}

func (r *requestRunner) post(ctx context.Context, path string, payload any) (*http.Response, []byte, error) {
	body, err := json.Marshal(payload)
	if err != nil {
		return nil, nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, r.cfg.ControlBaseURL+path, bytes.NewReader(body))
	if err != nil {
		return nil, nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	if r.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+r.cfg.ControlAuthToken)
	}
	if err := controlauth.Sign(req, body, r.signingSecret()); err != nil {
		return nil, nil, err
	}
	resp, err := r.cfg.HTTPClient.Do(req)
	if err != nil {
		return nil, nil, err
	}
	defer resp.Body.Close()
	respBody, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, nil, err
	}
	return resp, respBody, nil
}

func (r *requestRunner) claim(ctx context.Context) ([]claimedRequest, error) {
	resp, body, err := r.post(ctx, requestClaimPath, map[string]any{
		"worker":  r.worker,
		"limit":   requestBatch,
		"lease_s": requestLeaseS,
	})
	if err != nil {
		return nil, err
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("claim: control returned %d: %s", resp.StatusCode, truncate(body, 256))
	}
	var env struct {
		Data struct {
			Requests []claimedRequest `json:"requests"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &env); err != nil {
		return nil, fmt.Errorf("claim: decode: %w", err)
	}
	return env.Data.Requests, nil
}

func (r *requestRunner) postResult(ctx context.Context, id string, out requestOutcome) error {
	if id == "" || strings.ContainsAny(id, "/?#") {
		return fmt.Errorf("result: bad request id %q", id)
	}
	resp, body, err := r.post(ctx, fmt.Sprintf(requestResultPath, id), out)
	if err != nil {
		return err
	}
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return fmt.Errorf("result: control returned %d: %s", resp.StatusCode, truncate(body, 256))
	}
	return nil
}

func truncate(b []byte, n int) string {
	if len(b) > n {
		return string(b[:n])
	}
	return string(b)
}
