package resources

// Executable scheduled jobs (WS15.S2). A scrape, crawl or batch schedule fires
// through the orchestrator's ScheduledJobWF only when the kill switch is on.
// Each fire registers itself here as a control job with origin "schedule", so
// the fire is listed under GET /v1/schedules/{id}/runs and read like any job.

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// EnvScheduleNotices is the switch for schedule failure and skip notices
// (WS15.S4d). Unset or anything but a truthy value (1, true, yes, on) keeps them
// off: no notice is sent for a failed, timed-out or skipped fire.
const EnvScheduleNotices = "QUARRY_CONTROL__SCHEDULE_NOTICES"

// ScheduleNoticesEnabled reports whether QUARRY_CONTROL__SCHEDULE_NOTICES is truthy.
// Default false.
func ScheduleNoticesEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv(EnvScheduleNotices))) {
	case "true", "1", "yes", "on":
		return true
	}
	return false
}

// NoticeOption customises MountSchedules and MountEvents with the schedule notice
// sink. Without it no schedule notice is sent.
type NoticeOption func(*noticeConfig)

// noticeConfig is what the NoticeOptions set. The zero value sends nothing.
type noticeConfig struct {
	scheduleSink notify.Sink
}

// WithScheduleNotices sends quarry.schedule_run_failed and
// quarry.schedule_run_skipped notices to a schedule's creator through sink. The
// caller passes it only when QUARRY_CONTROL__SCHEDULE_NOTICES is on; a nil sink
// sends nothing.
func WithScheduleNotices(sink notify.Sink) NoticeOption {
	return func(into *noticeConfig) { into.scheduleSink = sink }
}

func noticeConfigOf(opts []NoticeOption) noticeConfig {
	var cfg noticeConfig
	for _, opt := range opts {
		opt(&cfg)
	}
	return cfg
}

// scheduleNoticeReasons is the closed vocabulary of the reason a schedule notice
// carries: the orchestrator's ScheduledJobWF reasons and the two failure event
// types. A reason outside it is replaced by the notice's fallback.
var scheduleNoticeReasons = map[string]struct{}{
	"preflight_denied": {},
	"invalid_params":   {},
	"child_failed":     {},
	"child_timed_out":  {},
	"run_failed":       {},
	"run_timed_out":    {},
}

// closedNoticeReason returns reason when it is in the closed vocabulary, else fallback.
func closedNoticeReason(reason, fallback string) string {
	if _, ok := scheduleNoticeReasons[strings.TrimSpace(reason)]; ok {
		return strings.TrimSpace(reason)
	}
	return fallback
}

// scheduleZdr reports the schedule's create-time ZDR stamp; an unstamped legacy row is standard.
func scheduleZdr(s store.Schedule) bool {
	return s.Zdr != nil && *s.Zdr
}

// sendScheduleNotice delivers one schedule notice to the schedule's creator.
// Best-effort with a 5 s budget: a failure is logged and never returned, so a
// notification-core outage cannot fail an event append or a fire registration.
// A schedule without a creator is an honest skip. The payload holds ids, the kind
// and a closed reason only, never the URL or target.
func sendScheduleNotice(sink notify.Sink, sched store.Schedule, noticeType, idempotencyKey, runID, reason string) {
	if sink == nil {
		return
	}
	recipient := strings.TrimSpace(sched.CreatedBy)
	if recipient == "" || strings.TrimSpace(sched.OrgID) == "" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	payload := map[string]string{
		"schedule_id":   string(sched.ID),
		"schedule_kind": sched.TargetKind,
		"reason":        reason,
		"run_id":        runID,
	}
	if err := sink.NotifyScheduleRun(ctx, sched.OrgID, recipient, idempotencyKey, noticeType, scheduleZdr(sched), payload); err != nil {
		log.Warn().Err(err).Str("schedule_id", string(sched.ID)).Str("type", noticeType).Msg("schedule notice delivery failed")
	}
}

// notifyOnScheduleRun sends the quarry.schedule_run_failed notice for a run_failed
// or run_timed_out event of a job a schedule fired (Job.ScheduleID set). The
// schedule is loaded within the job's org and the notice goes to its creator. The
// idempotency key is per job, so the wrapper's and the child's failure events of
// one fire raise one notice.
func notifyOnScheduleRun(db store.DB, sink notify.Sink, ev quarrycontracts.Event) {
	if sink == nil {
		return
	}
	if ev.Type != quarrycontracts.EvtRunFailed && ev.Type != quarrycontracts.EvtRunTimedOut {
		return
	}
	var (
		job   store.Job
		found bool
	)
	switch {
	case ev.JobID != nil:
		job, found = db.Jobs().Get(*ev.JobID)
	case ev.RunID != nil:
		job, found = db.Jobs().GetByRunID(*ev.RunID)
	}
	if !found || job.ScheduleID == nil {
		return
	}
	sched, ok := db.Schedules().GetByOrg(*job.ScheduleID, job.OrgID)
	if !ok {
		return
	}
	reason, _ := ev.Payload["reason"].(string)
	runID := ""
	if ev.RunID != nil {
		runID = string(*ev.RunID)
	}
	sendScheduleNotice(sink, sched, notify.ScheduleRunFailedType, "sched-fail:"+string(job.ID), runID,
		closedNoticeReason(reason, string(ev.Type)))
}

// EnvScheduledJobsEnabled is the kill switch for executable scrape, crawl and
// batch schedules. Unset or any value other than "1" keeps them unmapped: the
// reconciler pauses or skips them and creates nothing. It stays 0 until WS14
// scope is deployed and WS6.A is live.
const EnvScheduledJobsEnabled = "QUARRY_SCHEDULED_JOBS_ENABLED"

// ScheduledJobsEnabled reports whether scrape, crawl and batch schedules map to
// the ScheduledJobWF workflow. It is read at serialize time, so turning the
// switch off takes effect on the orchestrator's next reconcile pass.
func ScheduledJobsEnabled() bool {
	return os.Getenv(EnvScheduledJobsEnabled) == "1"
}

// scheduledJobArgs is the [0] element of the ScheduledJobWF args. The
// orchestrator's ScheduledJobInput decodes it; the reconciler adds the overlap
// policy, the execution timeout and the resolved crawl bounds.
type scheduledJobArgs struct {
	ScheduleID string         `json:"schedule_id"`
	OrgID      string         `json:"org_id"`
	CreatedBy  string         `json:"created_by,omitempty"`
	Kind       string         `json:"kind"`
	Params     map[string]any `json:"params"`
}

// scheduledParams is the params of a fire's job: the schedule's per-kind config
// with the tenant and the creator stamped from the schedule row. The org and the
// user are never read from a request body.
func scheduledParams(s store.Schedule) map[string]any {
	out := make(map[string]any, len(s.Config)+2)
	for k, v := range s.Config {
		out[k] = v
	}
	// A fired job carries no per-job webhook: a hook in the schedule config never went through
	// the create-time vet (W3-E3.5-3b), so it is not copied onto the job.
	delete(out, JobWebhookParamKey)
	out["org_id"] = s.OrgID
	out["user_id"] = s.CreatedBy
	return out
}

// scheduleFireKey is the idempotency key of one fire of one schedule. A retried
// registration returns the job the first attempt created.
func scheduleFireKey(id quarrycontracts.ID, runKey string) string {
	return "sched:" + string(id) + ":" + runKey
}

// registerRunBody is what the orchestrator sends for one fire. Org, kind and
// params are deliberately absent: they come from the schedule row.
type registerRunBody struct {
	RunKey     string `json:"run_key"`
	RunID      string `json:"run_id"`
	Status     string `json:"status"`
	Reason     string `json:"reason,omitempty"`
	WorkflowID string `json:"workflow_id,omitempty"`
}

// registerRunResult answers a registration with the job the fire is recorded as.
type registerRunResult struct {
	JobID  string `json:"job_id"`
	Status string `json:"status"`
}

// registerScheduledRun answers POST /v1/internal/schedules/{id}/runs, called by
// the orchestrator's RegisterScheduledRun activity (HMAC signed). It creates the
// fire's job: running (a workflow starts it) or terminal skipped (the edge
// denied the fire, and a schedule_skipped event records why). The schedule must
// exist and be enabled; a disabled or unknown schedule is refused.
func registerScheduledRun(db store.DB, sink notify.Sink) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		var in registerRunBody
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if in.RunKey == "" || len(in.RunKey) > 128 || in.RunID == "" || len(in.RunID) > 128 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "run_key and run_id are required (max 128 characters)", nil)
			return
		}
		if in.Status != "running" && in.Status != "skipped" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "status must be running or skipped", nil)
			return
		}
		if len(in.Reason) > 64 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "reason is limited to 64 characters", nil)
			return
		}

		sched, ok := db.Schedules().Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		if !sched.Enabled {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "schedule is disabled", map[string]any{"id": id})
			return
		}
		if !store.AdmissionKind(sched.TargetKind) {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "schedule kind does not run as a job", map[string]any{"id": id})
			return
		}

		key := scheduleFireKey(id, in.RunKey)
		if existing, found := db.Jobs().FindByIdempotencyKey(sched.OrgID, key); found {
			httpx.WriteJSON(w, r, http.StatusOK, registerRunResult{JobID: string(existing.ID), Status: existing.Status})
			return
		}

		now := time.Now().UnixMilli()
		sid := id
		k := key
		job := store.Job{
			ID:             quarrycontracts.NewID(quarrycontracts.KindJob),
			OrgID:          sched.OrgID,
			Kind:           sched.TargetKind,
			Status:         "running",
			Policy:         quarrycontracts.DefaultRunPolicy(),
			Params:         scheduledParams(sched),
			ScheduleID:     &sid,
			CreatedAt:      now,
			IdempotencyKey: &k,
			WorkflowID:     in.WorkflowID,
			Origin:         store.JobOrigin{Type: "schedule", Actor: string(id), CreatedBy: sched.CreatedBy},
		}
		runID := quarrycontracts.ID(in.RunID)
		if in.Status == "skipped" {
			job.Status = "skipped"
			job.TerminalReason = "schedule_skipped"
			job.FinishedAt = &now
		} else {
			job.RunID = &runID
		}

		if err := db.Jobs().Create(job); err != nil {
			if existing, found := db.Jobs().FindByIdempotencyKey(sched.OrgID, key); found {
				httpx.WriteJSON(w, r, http.StatusOK, registerRunResult{JobID: string(existing.ID), Status: existing.Status})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}

		if in.Status == "skipped" {
			writeScheduleSkipped(db, sink, sched, job.ID, runID, in.RunKey, in.Reason, now)
		}
		httpx.WriteJSON(w, r, http.StatusCreated, registerRunResult{JobID: string(job.ID), Status: job.Status})
	}
}

// writeScheduleSkipped records the schedule_skipped event of a denied fire. A
// failed append is logged, not returned: the skipped job already exists and a
// retry would only find it again. After a successful append the schedule's
// creator gets a quarry.schedule_run_skipped notice when sink is set (WS15.S4d);
// the notice is best-effort and never fails the registration.
func writeScheduleSkipped(db store.DB, sink notify.Sink, sched store.Schedule, jobID, runID quarrycontracts.ID, runKey, reason string, now int64) {
	scheduleID := sched.ID
	evt := quarrycontracts.Event{
		EventID:        quarrycontracts.NewID(quarrycontracts.KindEvent),
		RunID:          &runID,
		JobID:          &jobID,
		Type:           quarrycontracts.EvtScheduleSkipped,
		Timestamp:      time.UnixMilli(now).UTC(),
		Payload:        map[string]any{"schedule_id": string(scheduleID), "reason": reason},
		IdempotencyKey: "sched-skip:" + string(scheduleID) + ":" + runKey,
	}
	// The seq is assigned in the same transaction as the insert (D2-016).
	if _, err := db.Events().AppendNext(evt); err != nil {
		log.Warn().Str("schedule_id", string(scheduleID)).Msg("schedule_skipped event append failed")
		return
	}
	sendScheduleNotice(sink, sched, notify.ScheduleRunSkippedType, evt.IdempotencyKey, string(runID),
		closedNoticeReason(reason, "preflight_denied"))
}

// maxScheduleRuntimeBodyBytes caps the runtime write. The body holds three fields.
const maxScheduleRuntimeBodyBytes = 4 << 10

// minRuntimeYear and maxRuntimeYear bound the UTC year of a runtime timestamp
// (WS15.S4b). A value outside them is a unit or clock mistake, not a run time.
const (
	minRuntimeYear = 2000
	maxRuntimeYear = 2200
)

// scheduleRuntimeBody is the body of PUT /v1/internal/schedules/{id}/runtime. The
// write replaces the three runtime fields of the row, so an absent or null
// timestamp clears it. Org and params are not fields here: an unknown key is refused.
type scheduleRuntimeBody struct {
	LastRunAt           *int64 `json:"last_run_at"`
	NextRunAt           *int64 `json:"next_run_at"`
	SkippedOverlapCount *int64 `json:"skipped_overlap_count"`
}

// updateScheduleRuntime answers PUT /v1/internal/schedules/{id}/runtime, called by
// the orchestrator's reconciler (WS15.S4c). The route is under /v1/internal/schedules/,
// so the control-internal secret verifies it (D2-050). It writes the executor's
// bookkeeping of one schedule: the latest and the next run, and the number of fires
// Temporal skipped because a run was in flight. The timestamps arrive in unix
// milliseconds and are stored in unix seconds, the unit of the row's created_at and
// updated_at, which the tenant view renders. A missing or negative count, and a
// timestamp outside the years 2000 to 2200, are a 400. A schedule that does not
// exist is a 404. Org and params are never read from the body.
func updateScheduleRuntime(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		r.Body = http.MaxBytesReader(w, r.Body, maxScheduleRuntimeBodyBytes)
		dec := json.NewDecoder(r.Body)
		dec.DisallowUnknownFields()
		var in scheduleRuntimeBody
		if err := dec.Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "decode body: "+err.Error(), nil)
			return
		}
		if in.SkippedOverlapCount == nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "skipped_overlap_count is required",
				map[string]any{"field": "skipped_overlap_count", "reason": "count_required"})
			return
		}
		if *in.SkippedOverlapCount < 0 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "skipped_overlap_count must be 0 or more",
				map[string]any{"field": "skipped_overlap_count", "reason": "negative_count"})
			return
		}
		lastRunAt, ok := runtimeSeconds(w, r, "last_run_at", in.LastRunAt)
		if !ok {
			return
		}
		nextRunAt, ok := runtimeSeconds(w, r, "next_run_at", in.NextRunAt)
		if !ok {
			return
		}
		if err := db.Schedules().UpdateRuntime(id, lastRunAt, nextRunAt, *in.SkippedOverlapCount); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}
}

// runtimeSeconds reads one optional runtime timestamp of the body. An absent or null
// value is nil, which clears the stored field. Any other value must be unix
// milliseconds in a UTC year from 2000 to 2200, and is returned in unix seconds. A
// value that fails writes the 400 and reports false.
func runtimeSeconds(w http.ResponseWriter, r *http.Request, field string, ms *int64) (*int64, bool) {
	if ms == nil {
		return nil, true
	}
	year := time.UnixMilli(*ms).UTC().Year()
	if year < minRuntimeYear || year > maxRuntimeYear {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, field+" must be unix milliseconds in the years 2000 to 2200",
			map[string]any{"field": field, "reason": "timestamp_out_of_range"})
		return nil, false
	}
	secs := *ms / 1000
	return &secs, true
}
