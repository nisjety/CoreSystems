package schedules

// Schedule runtime sync (WS15.S4c). Temporal keeps a schedule's runtime: the
// scheduled times of its latest and next actions, and how many fires it skipped
// because an earlier run was still going (overlap SKIP). Control keeps the same
// facts for the tenant view (PUT /v1/internal/schedules/{id}/runtime, WS15.S4b)
// and records a skipped fire as a skipped run. Before this file, nothing wrote
// either.
//
// With QUARRY_SCHEDULE_RUNTIME_SYNC on, each reconcile pass:
//   - reads last_run_at and next_run_at from the schedule list it already fetched;
//   - every five minutes per schedule, describes the schedule for its skip count
//     and PUTs the three runtime fields to control;
//   - records each overlap skip that no row covers yet as a skipped run
//     (POST /v1/internal/schedules/{id}/runs, reason skipped_overlap), at most ten
//     rows per pass across all schedules. Only ScheduledJobWF schedules get rows:
//     control refuses a change_monitor run (AdmissionKind), so a monitor's skips
//     reach the runtime count only.
//
// Only a desired schedule whose memo org is in the desired set is synced. A failure
// is logged with the schedule id and a status, never a response body, and is
// retried on a later pass; it never fails the reconcile. The sync sends ids, counts
// and times, and no run content. OFF BY DEFAULT.

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strings"
	"time"

	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
)

// EnvScheduleRuntimeSync turns the runtime sync on when truthy (1, true, yes, on).
const EnvScheduleRuntimeSync = "QUARRY_SCHEDULE_RUNTIME_SYNC"

// RuntimeSyncEnabled reports whether EnvScheduleRuntimeSync is on. Default off.
// cmd/orchestrator reads it once into Config.RuntimeSync.
func RuntimeSyncEnabled() bool {
	return runtimeSyncEnabled(os.Getenv)
}

func runtimeSyncEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv(EnvScheduleRuntimeSync))) {
	case "1", "true", "yes", "on":
		return true
	}
	return false
}

// runtimeDescribeEvery is how often one schedule is described for its skip count.
const runtimeDescribeEvery = 5 * time.Minute

// runtimeRowsPerPass caps the skipped-run rows one reconcile pass posts, across all
// schedules.
const runtimeRowsPerPass = 10

// runtimeCallTimeout bounds one Temporal describe and one control call.
const runtimeCallTimeout = 15 * time.Second

// runtimeSkipReason is the reason of a skipped-run row for an overlap skip. It is
// one of the closed skip reasons quarry-edge's schedule preflight already names.
const runtimeSkipReason = "skipped_overlap"

// runtimePathFmt is PUT /v1/internal/schedules/{id}/runtime (WS15.S4b).
const runtimePathFmt = "/v1/internal/schedules/%s/runtime"

// runsPathFmt is POST /v1/internal/schedules/{id}/runs (the registration route).
const runsPathFmt = "/v1/internal/schedules/%s/runs"

// runtimeTrack is what the runtime sync keeps for one schedule between passes. It
// is in memory only: a restarted orchestrator describes every schedule again and
// takes the first skip count it reads as the baseline.
type runtimeTrack struct {
	// describedAt is when the schedule was last pushed to control. It stays zero
	// until a push succeeds.
	describedAt time.Time
	// seeded is set once the first skip count has been read.
	seeded bool
	// skips is the latest skip count Temporal reported.
	skips int64
	// rowsPushed is the sequence number of the last skipped run control accepted.
	// Every overlap skip up to it is recorded.
	rowsPushed int64
}

// runtimeBody is the body of PUT /v1/internal/schedules/{id}/runtime. Control reads
// a nil timestamp as "clear the field" and refuses any other key, so exactly these
// three keys are sent.
type runtimeBody struct {
	LastRunAt           *int64 `json:"last_run_at"`
	NextRunAt           *int64 `json:"next_run_at"`
	SkippedOverlapCount int64  `json:"skipped_overlap_count"`
}

// skipRowBody is the body of POST /v1/internal/schedules/{id}/runs for one overlap
// skip. Control refuses a row with no run id, so each row carries a fresh one. The
// run key is the idempotency key, so a retry returns the row already recorded.
type skipRowBody struct {
	RunKey string `json:"run_key"`
	RunID  string `json:"run_id"`
	Status string `json:"status"`
	Reason string `json:"reason"`
}

// syncRuntime runs the runtime sync for one reconcile pass. desired is the pass's
// desired set, desiredOrgs the orgs it represents, and existing the schedules
// Temporal listed for the pass. It returns nothing: each failure is logged and
// retried on a later pass.
func (m *Manager) syncRuntime(ctx context.Context, sc client.ScheduleClient, existing map[string]existingSchedule, desired map[string]ScheduleSpec, desiredOrgs map[string]struct{}) {
	if m.runtime == nil {
		m.runtime = make(map[string]*runtimeTrack)
	}
	// The schedules this pass syncs are desired, present in Temporal, and owned by an
	// org in the desired set. A schedule with no memo org is never synced (fail-closed).
	ids := make([]string, 0, len(desired))
	for id := range desired {
		ex, ok := existing[id]
		if !ok || ex.OrgID == "" {
			continue
		}
		if _, owned := desiredOrgs[ex.OrgID]; !owned {
			continue
		}
		ids = append(ids, id)
	}
	sort.Strings(ids)

	// Forget a schedule that is no longer synced, so one that comes back starts from
	// a fresh baseline.
	keep := make(map[string]struct{}, len(ids))
	for _, id := range ids {
		keep[id] = struct{}{}
	}
	for id := range m.runtime {
		if _, ok := keep[id]; !ok {
			delete(m.runtime, id)
		}
	}

	now := time.Now()
	budget := runtimeRowsPerPass
	for _, id := range ids {
		if ctx.Err() != nil {
			return
		}
		t, ok := m.runtime[id]
		if !ok {
			t = &runtimeTrack{}
			m.runtime[id] = t
		}
		if t.describedAt.IsZero() || now.Sub(t.describedAt) >= runtimeDescribeEvery {
			m.describeAndPush(ctx, sc, id, existing[id], t, now)
		}
		if desired[id].Workflow == ScheduledJobWorkflow {
			budget = m.pushSkipRows(ctx, id, t, budget)
		}
	}
}

// describeAndPush reads one schedule's skip count from Temporal and pushes the
// schedule's runtime to control. The first count read is the baseline: skips that
// happened before this process first saw the schedule are counted on the runtime
// row but are not recorded as runs. A failed read or push leaves the track as it
// was, and the next pass tries again.
func (m *Manager) describeAndPush(ctx context.Context, sc client.ScheduleClient, id string, ex existingSchedule, t *runtimeTrack, now time.Time) {
	callCtx, cancel := context.WithTimeout(ctx, runtimeCallTimeout)
	desc, err := sc.GetHandle(callCtx, id).Describe(callCtx)
	cancel()
	if err != nil || desc == nil {
		m.log.Warn().Str("id", id).Str("status", "describe_failed").Msg("schedule runtime not read; retried next pass")
		return
	}
	total := int64(desc.Info.NumActionsSkippedOverlap)
	if !t.seeded {
		t.seeded = true
		t.rowsPushed = total
	}
	if total < t.rowsPushed {
		// The count went back, so the schedule was recreated under the same id.
		t.rowsPushed = total
	}
	t.skips = total

	body := runtimeBody{
		LastRunAt:           optionalUnixMillis(ex.LastRunAt),
		NextRunAt:           optionalUnixMillis(ex.NextRunAt),
		SkippedOverlapCount: total,
	}
	status, err := m.controlCall(ctx, http.MethodPut, fmt.Sprintf(runtimePathFmt, url.PathEscape(id)), body)
	if err != nil || status < 200 || status > 299 {
		m.log.Warn().Str("id", id).Int("http_status", status).Str("status", "runtime_push_failed").Msg("schedule runtime not written; retried next pass")
		return
	}
	t.describedAt = now
}

// pushSkipRows records the overlap skips of one schedule that no row covers yet,
// oldest first, while the pass has budget left. It stops at the first failure,
// which the next pass retries, and returns the budget left.
func (m *Manager) pushSkipRows(ctx context.Context, id string, t *runtimeTrack, budget int) int {
	for t.rowsPushed < t.skips && budget > 0 && ctx.Err() == nil {
		n := t.rowsPushed + 1
		body := skipRowBody{
			RunKey: fmt.Sprintf("sched:%s:skip:%d", id, n),
			RunID:  string(quarrycontracts.NewID(quarrycontracts.KindRun)),
			Status: "skipped",
			Reason: runtimeSkipReason,
		}
		status, err := m.controlCall(ctx, http.MethodPost, fmt.Sprintf(runsPathFmt, url.PathEscape(id)), body)
		budget--
		if err != nil || status < 200 || status > 299 {
			m.log.Warn().Str("id", id).Int("http_status", status).Str("status", "skip_row_failed").Msg("skipped run not recorded; retried next pass")
			break
		}
		t.rowsPushed = n
		m.log.Info().Str("id", id).Int64("skip", n).Msg("overlap skip recorded as a skipped run")
	}
	return budget
}

// controlCall sends one HMAC-signed request to control and returns the HTTP status,
// or 0 when no response came back. The response body is drained and never read, so
// nothing control returns reaches a log.
func (m *Manager) controlCall(ctx context.Context, method, path string, payload any) (int, error) {
	raw, err := json.Marshal(payload)
	if err != nil {
		return 0, err
	}
	callCtx, cancel := context.WithTimeout(ctx, runtimeCallTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(callCtx, method, m.cfg.ControlBaseURL+path, bytes.NewReader(raw))
	if err != nil {
		return 0, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	if err := controlauth.Sign(req, raw, runtimeSigningSecret(m.cfg.ControlHMACSecret)); err != nil {
		return 0, err
	}
	resp, err := m.cfg.HTTPClient.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 64<<10))
	return resp.StatusCode, nil
}

// runtimeSigningSecret is the secret the runtime calls are signed with: the
// control-internal secret when it is set, otherwise the shared secret. It is the
// rule of requestRunner.signingSecret (requests.go). controlauth does not list the
// runtime route among control's internal paths, so Sign uses the secret it is given
// for it; passing the control secret keeps the orchestrator and control equal.
func runtimeSigningSecret(shared string) string {
	if s := strings.TrimSpace(os.Getenv(controlauth.ControlInternalSecretEnv)); s != "" {
		return s
	}
	return shared
}

// optionalUnixMillis is t in unix milliseconds, or nil for the zero time. Control
// reads nil as "clear the field".
func optionalUnixMillis(t time.Time) *int64 {
	if t.IsZero() {
		return nil
	}
	ms := t.UnixMilli()
	return &ms
}

// latestScheduleTime is the scheduled time of the latest of the recent actions, or
// the zero time when there are none (WS15.S4c).
func latestScheduleTime(recent []client.ScheduleActionResult) time.Time {
	var latest time.Time
	for _, a := range recent {
		if a.ScheduleTime.After(latest) {
			latest = a.ScheduleTime
		}
	}
	return latest
}

// nextScheduleTime is the first of the next scheduled action times, or the zero time
// when Temporal holds none (WS15.S4c).
func nextScheduleTime(next []time.Time) time.Time {
	if len(next) == 0 {
		return time.Time{}
	}
	return next[0]
}
