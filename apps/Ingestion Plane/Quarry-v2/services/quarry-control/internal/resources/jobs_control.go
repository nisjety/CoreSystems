package resources

// Job control (WS15.L2): the org-scoped control request a caller writes, and the
// internal calls the orchestrator's control loop makes to read the pending
// requests, acknowledge them, list the running jobs and finalize the ones whose
// workflow ended without telling control. The rules are in store.PlanControl.
// Control never talks to Temporal; the orchestrator does.

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// controlRequest is the body of POST /v1/jobs/{id}/control, sent by the edge.
// The actor is what the edge verified from the caller; control records it and
// never reads an actor from anywhere else.
type controlRequest struct {
	Action    string `json:"action"`
	Actor     string `json:"actor"`
	ActorType string `json:"actor_type"`
}

// controlJob backs POST /v1/jobs/{id}/control (org-scoped by ?org_id). It
// answers the job record after the request was written. A cancel of an accepted
// job ends it at once and writes run_cancelled; every accepted request writes
// control_requested.
func controlJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		var in controlRequest
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		switch in.Action {
		case store.ControlPause, store.ControlResume, store.ControlCancel:
		default:
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "action must be pause, resume or cancel", nil)
			return
		}
		job, err := db.Jobs().SetControl(org, id, in.Action)
		if err != nil {
			var refused *store.ControlError
			switch {
			case errors.As(err, &refused):
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "the job cannot take this request",
					map[string]any{"id": id, "status": refused.Status, "reason": refused.Reason})
			case errors.Is(err, store.ErrNotFound):
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			default:
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			}
			return
		}
		requested := quarrycontracts.Event{
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
			Type:    quarrycontracts.EvtControlRequested,
			Payload: map[string]any{
				"action":     in.Action,
				"actor":      in.Actor,
				"actor_type": in.ActorType,
			},
		}
		_ = appendJobEvent(db, job, requested)
		if job.Status == "cancelled" && job.TerminalReason == "run_cancelled" {
			// Cancelled before any workflow started: the run ends here.
			ended := quarrycontracts.Event{
				EventID: deterministicJobEventID(job.ID, "run_cancelled:before_start"),
				Type:    quarrycontracts.EvtRunCancelled,
				Payload: map[string]any{"pages_visited": 0, "pages_failed": 0, "forced": false, "before_start": true},
			}
			_ = appendJobEvent(db, job, ended)
		}
		httpx.WriteJSON(w, r, http.StatusOK, job)
	}
}

// pendingControl backs GET /v1/internal/jobs/pending-control?limit=N: the jobs
// whose desired state is ahead of what the orchestrator applied.
func pendingControl(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		httpx.WriteJSON(w, r, http.StatusOK, db.Jobs().PendingControl(internalLimit(r, 100, 500)))
	}
}

// controlAck backs POST /v1/internal/jobs/{id}/control-ack with {applied_seq}.
// It raises applied_seq; a lower value is accepted and ignored.
func controlAck(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		var in struct {
			AppliedSeq int64 `json:"applied_seq"`
		}
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if err := db.Jobs().AckControl(id, in.AppliedSeq); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{"id": id, "applied_seq": in.AppliedSeq})
	}
}

// runningJobs backs GET /v1/internal/jobs/running?limit=N: running or paused
// jobs that have a workflow, for the orchestrator's sweep.
func runningJobs(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		httpx.WriteJSON(w, r, http.StatusOK, db.Jobs().ListRunning(internalLimit(r, 200, 500)))
	}
}

// finalizeJob backs POST /v1/internal/jobs/{id}/finalize. The orchestrator sends
// the terminal status it observed and the facts of it. Only a job that has not
// ended changes; the event is written once, with a deterministic id.
func finalizeJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		var in struct {
			Status      string `json:"status"`
			Forced      bool   `json:"forced"`
			TimeoutType string `json:"timeout_type"`
			LimitS      int64  `json:"limit_s"`
			FinishedAt  int64  `json:"finished_at"`
		}
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		var (
			reason  string
			evtType quarrycontracts.EventType
			payload map[string]any
		)
		switch in.Status {
		case "timed_out":
			reason, evtType = "run_timed_out", quarrycontracts.EvtRunTimedOut
			payload = map[string]any{"timeout_type": in.TimeoutType, "limit_s": in.LimitS}
		case "cancelled":
			reason, evtType = "run_cancelled", quarrycontracts.EvtRunCancelled
			payload = map[string]any{"forced": in.Forced}
		case "failed":
			reason, evtType = "run_failed", quarrycontracts.EvtRunFailed
			payload = map[string]any{"reason": "workflow_failed"}
		default:
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "status must be timed_out, cancelled or failed", nil)
			return
		}
		finishedAt := in.FinishedAt
		if finishedAt <= 0 {
			finishedAt = time.Now().UnixMilli()
		}
		job, changed, err := db.Jobs().Finalize(id, in.Status, reason, finishedAt)
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		if changed {
			ended := quarrycontracts.Event{
				EventID: deterministicJobEventID(id, reason+":finalize"),
				Type:    evtType,
				Payload: payload,
			}
			if err := appendJobEvent(db, job, ended); err != nil && !errors.Is(err, store.ErrConflict) {
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
				return
			}
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{"job": job, "changed": changed})
	}
}

// appendJobEvent writes an event of job j. A job with a run gets the run's next
// sequence number; a job that never started gets sequence 1 with no run.
func appendJobEvent(db store.DB, j store.Job, evt quarrycontracts.Event) error {
	jobID := j.ID
	evt.JobID = &jobID
	if evt.Timestamp.IsZero() {
		evt.Timestamp = time.Now().UTC()
	}
	if j.RunID != nil {
		rid := *j.RunID
		evt.RunID = &rid
		// The seq is assigned in the same transaction as the insert (D2-016).
		_, err := db.Events().AppendNext(evt)
		return err
	}
	evt.Seq = 1
	return db.Events().Append(evt)
}

// deterministicJobEventID is the event id for an event control writes once per
// job and reason, so a retried write is a conflict, not a second event.
func deterministicJobEventID(jobID quarrycontracts.ID, tag string) quarrycontracts.ID {
	sum := sha256.Sum256([]byte(string(jobID) + "|" + tag))
	return quarrycontracts.ID("evt_" + hex.EncodeToString(sum[:])[:26])
}

// internalLimit reads ?limit= for the internal lists: def when absent or not a
// positive number, capped at max.
func internalLimit(r *http.Request, def, max int) int {
	n, err := strconv.Atoi(r.URL.Query().Get("limit"))
	if err != nil || n <= 0 {
		return def
	}
	if n > max {
		return max
	}
	return n
}
