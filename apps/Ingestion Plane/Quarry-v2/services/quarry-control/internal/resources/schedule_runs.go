package resources

// Executable scheduled jobs (WS15.S2). A scrape, crawl or batch schedule fires
// through the orchestrator's ScheduledJobWF only when the kill switch is on.
// Each fire registers itself here as a control job with origin "schedule", so
// the fire is listed under GET /v1/schedules/{id}/runs and read like any job.

import (
	"encoding/json"
	"net/http"
	"os"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

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
func registerScheduledRun(db store.DB) http.HandlerFunc {
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
			writeScheduleSkipped(db, id, job.ID, runID, in.RunKey, in.Reason, now)
		}
		httpx.WriteJSON(w, r, http.StatusCreated, registerRunResult{JobID: string(job.ID), Status: job.Status})
	}
}

// writeScheduleSkipped records the schedule_skipped event of a denied fire. A
// failed append is logged, not returned: the skipped job already exists and a
// retry would only find it again.
func writeScheduleSkipped(db store.DB, scheduleID, jobID, runID quarrycontracts.ID, runKey, reason string, now int64) {
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
	}
}
