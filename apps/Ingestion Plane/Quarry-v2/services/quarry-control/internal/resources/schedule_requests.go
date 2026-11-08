package resources

// Trigger and backfill through pull-model request rows (WS15.S3, X-9 / D14
// option 2, migration 026).
//
// quarry-control holds no Temporal client. With QUARRY_CONTROL__SCHEDULE_REQUESTS
// on, POST /v1/schedules/{id}/trigger and /backfill validate the request, insert
// a schedule_requests row and answer 202 {request_id, status:"pending"}. The
// orchestrator claims rows every 2 s (POST /v1/internal/schedules/requests/claim),
// asks Temporal to trigger or backfill the schedule (so the fire runs the same
// ScheduledJobWF, with the same fire-time preflight, as a cron fire) and posts the
// outcome (POST /v1/internal/schedules/requests/{rid}/result). The tenant reads
// the outcome at GET /v1/schedules/{id}/requests/{rid}.
//
// The internal routes sit under /v1/internal/schedules/ so control's split HMAC
// verifies them with the orchestrator-only secret (httpx.SplitHMAC, D2-050).
//
// OFF BY DEFAULT. With the flag unset the trigger and backfill routes keep their
// previous answer (501 UNSUPPORTED while no Temporal client is wired) and none of
// the routes below is mounted. The gateway's SCHEDULE capabilities trigger and
// backfill stay false either way; turning them on is WS15.S5.

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/cron"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// EnvScheduleRequests turns the request-row transport on (truthy: 1, true, yes, on).
const EnvScheduleRequests = "QUARRY_CONTROL__SCHEDULE_REQUESTS"

// ScheduleRequestsEnabled reports whether EnvScheduleRequests is truthy. Default false.
func ScheduleRequestsEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv(EnvScheduleRequests))) {
	case "1", "true", "yes", "on":
		return true
	}
	return false
}

const (
	// backfillMaxWindows caps one backfill: N missed windows start N runs.
	backfillMaxWindows = 24
	// Claim bounds. The orchestrator asks for a small batch every 2 s.
	claimLimitDefault = 10
	claimLimitMax     = 50
	claimLeaseDefault = 30 * time.Second
	claimLeaseMin     = 5 * time.Second
	claimLeaseMax     = 5 * time.Minute
	workerNameMax     = 128
	idempotencyKeyMax = 128
)

// scheduleRequestsOf returns the request table of db, when it has one.
func scheduleRequestsOf(db store.DB) (store.ScheduleRequestsStore, bool) {
	p, ok := db.(store.ScheduleRequestsProvider)
	if !ok {
		return nil, false
	}
	s := p.ScheduleRequests()
	return s, s != nil
}

// mountScheduleRequests adds the tenant read and the two orchestrator routes.
func mountScheduleRequests(r chi.Router, db store.DB) {
	r.Get("/v1/schedules/{id}/requests/{rid}", getScheduleRequestHandler(db))
	r.Post("/v1/internal/schedules/requests/claim", claimScheduleRequestsHandler(db))
	r.Post("/v1/internal/schedules/requests/{rid}/result", scheduleRequestResultHandler(db))
}

// scheduleRequestView is the tenant view of a request: no worker, no lease.
type scheduleRequestView struct {
	RequestID  string         `json:"request_id"`
	ScheduleID string         `json:"schedule_id"`
	Action     string         `json:"action"`
	Status     string         `json:"status"`
	Attempts   int            `json:"attempts"`
	Params     map[string]any `json:"params,omitempty"`
	Result     map[string]any `json:"result,omitempty"`
	Error      string         `json:"error,omitempty"`
	CreatedAt  int64          `json:"created_at"`
	FinishedAt *int64         `json:"finished_at,omitempty"`
}

func toScheduleRequestView(v store.ScheduleRequest) scheduleRequestView {
	return scheduleRequestView{
		RequestID:  string(v.ID),
		ScheduleID: string(v.ScheduleID),
		Action:     v.Action,
		Status:     v.Status,
		Attempts:   v.Attempts,
		Params:     v.Params,
		Result:     v.Result,
		Error:      v.Error,
		CreatedAt:  v.CreatedAt,
		FinishedAt: v.FinishedAt,
	}
}

// claimedRequestView is what the orchestrator gets for a claimed row.
type claimedRequestView struct {
	ID         string         `json:"id"`
	ScheduleID string         `json:"schedule_id"`
	OrgID      string         `json:"org_id"`
	Action     string         `json:"action"`
	Params     map[string]any `json:"params,omitempty"`
	Attempts   int            `json:"attempts"`
	LeaseUntil int64          `json:"lease_until"`
}

// scheduleRequestAccepted is the 202 body of a trigger or backfill.
type scheduleRequestAccepted struct {
	RequestID  string `json:"request_id"`
	ScheduleID string `json:"schedule_id"`
	Action     string `json:"action"`
	Status     string `json:"status"`
}

func writeRefusal(w http.ResponseWriter, r *http.Request, code quarrycontracts.ErrorCode, reason, msg string, id quarrycontracts.ID) {
	httpx.WriteErr(w, r, code, msg, map[string]any{"reason": reason, "id": id})
}

// requestPreamble runs the checks trigger and backfill share. On false it has
// already written the answer.
func requestPreamble(w http.ResponseWriter, r *http.Request, db store.DB, action string) (store.Schedule, store.ScheduleRequestsStore, bool) {
	id := quarrycontracts.ID(chi.URLParam(r, "id"))
	if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
		return store.Schedule{}, nil, false
	}
	org := orgFromQuery(r)
	if org == "" {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
		return store.Schedule{}, nil, false
	}
	sched, ok := db.Schedules().GetByOrg(id, org)
	if !ok {
		// A schedule of another org is not found, never forbidden.
		httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
		return store.Schedule{}, nil, false
	}
	reqs, ok := scheduleRequestsOf(db)
	if !ok {
		httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported, action+" requests are not available on this store", nil)
		return store.Schedule{}, nil, false
	}
	if !sched.Enabled {
		writeRefusal(w, r, quarrycontracts.CodeConflict, "needs_activation",
			"the schedule is paused; activate it first", id)
		return store.Schedule{}, nil, false
	}
	// A scrape, crawl or batch schedule is materialised in Temporal only while
	// the kill switch is on; without it the orchestrator would reject the row.
	if store.AdmissionKind(sched.TargetKind) && !ScheduledJobsEnabled() {
		writeRefusal(w, r, quarrycontracts.CodeConflict, "scheduled_jobs_disabled",
			"scheduled jobs are switched off on this deployment", id)
		return store.Schedule{}, nil, false
	}
	if reqs.HasPending(id, "") {
		writeRefusal(w, r, quarrycontracts.CodeConflict, "request_pending",
			"this schedule already has a trigger or backfill waiting", id)
		return store.Schedule{}, nil, false
	}
	return sched, reqs, true
}

// insertRequest stores the row and answers 202.
func insertRequest(w http.ResponseWriter, r *http.Request, reqs store.ScheduleRequestsStore, sched store.Schedule, action string, params map[string]any) {
	key := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
	if len(key) > idempotencyKeyMax {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "Idempotency-Key is limited to 128 characters", nil)
		return
	}
	req := store.ScheduleRequest{
		ID:             quarrycontracts.NewID(quarrycontracts.KindRequest),
		ScheduleID:     sched.ID,
		OrgID:          sched.OrgID,
		Action:         action,
		Params:         params,
		Status:         store.ScheduleRequestPending,
		RequestedBy:    strings.TrimSpace(r.URL.Query().Get("user_id")),
		IdempotencyKey: key,
		CreatedAt:      time.Now().UnixMilli(),
	}
	if err := reqs.Create(req); err != nil {
		if errors.Is(err, store.ErrConflict) {
			// Two quick presses: the database's one-open-row index refused the second.
			writeRefusal(w, r, quarrycontracts.CodeConflict, "request_pending",
				"this schedule already has a trigger or backfill waiting", sched.ID)
			return
		}
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "schedule request: "+err.Error(), nil)
		return
	}
	httpx.WriteJSON(w, r, http.StatusAccepted, scheduleRequestAccepted{
		RequestID:  string(req.ID),
		ScheduleID: string(sched.ID),
		Action:     action,
		Status:     store.ScheduleRequestPending,
	})
}

// requestScheduleTrigger answers POST /v1/schedules/{id}/trigger in request mode.
func requestScheduleTrigger(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sched, reqs, ok := requestPreamble(w, r, db, store.ScheduleRequestTrigger)
		if !ok {
			return
		}
		insertRequest(w, r, reqs, sched, store.ScheduleRequestTrigger, nil)
	}
}

var validBackfillOverlap = map[string]bool{"": true, "skip": true, "allow": true, "cancel": true}

// backfillWindows estimates how many fires of sched fall in [start, end]: the
// span over the cron's smallest gap, rounded up. For a regular cron this is the
// count; for an irregular one it is an upper bound, so the cap never lets more
// runs through than it says.
func backfillWindows(sched store.Schedule, start, end time.Time) (int64, error) {
	tz := sched.Timezone
	if tz == "" {
		tz = "UTC"
	}
	loc, err := time.LoadLocation(tz)
	if err != nil {
		return 0, err
	}
	gap, err := cron.MinInterval(sched.Cron, loc)
	if err != nil {
		return 0, err
	}
	if gap <= 0 {
		return 0, errors.New("cron has no interval")
	}
	span := end.Sub(start)
	n := int64(span / gap)
	if span%gap != 0 {
		n++
	}
	if n < 1 {
		n = 1
	}
	return n, nil
}

// requestScheduleBackfill answers POST /v1/schedules/{id}/backfill in request
// mode. Only scrape schedules backfill (a crawl or batch backfill would start one
// crawl per missed window), at most 24 windows, entirely in the past.
func requestScheduleBackfill(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var body backfillBody
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if !body.EndAt.After(body.StartAt) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "end_at must be strictly after start_at", nil)
			return
		}
		if body.EndAt.After(time.Now().Add(time.Minute)) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "end_at must not be in the future",
				map[string]any{"reason": "backfill_window_in_future"})
			return
		}
		body.OverlapPolicy = strings.ToLower(strings.TrimSpace(body.OverlapPolicy))
		if !validBackfillOverlap[body.OverlapPolicy] {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "overlap_policy must be skip, allow or cancel", nil)
			return
		}
		sched, reqs, ok := requestPreamble(w, r, db, store.ScheduleRequestBackfill)
		if !ok {
			return
		}
		if sched.TargetKind != "scrape" {
			writeRefusal(w, r, quarrycontracts.CodeConflict, "backfill_kind_refused",
				"only scrape schedules can be backfilled; a "+sched.TargetKind+" backfill would start one run per missed window", sched.ID)
			return
		}
		n, err := backfillWindows(sched, body.StartAt, body.EndAt)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "backfill: "+err.Error(), nil)
			return
		}
		if n > backfillMaxWindows {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "a backfill covers at most 24 windows",
				map[string]any{"reason": "backfill_too_many_windows", "windows": n, "max": backfillMaxWindows})
			return
		}
		params := map[string]any{
			"start_at": body.StartAt.UTC().Format(time.RFC3339),
			"end_at":   body.EndAt.UTC().Format(time.RFC3339),
			"windows":  n,
		}
		if body.OverlapPolicy != "" {
			params["overlap_policy"] = body.OverlapPolicy
		}
		insertRequest(w, r, reqs, sched, store.ScheduleRequestBackfill, params)
	}
}

// getScheduleRequestHandler answers GET /v1/schedules/{id}/requests/{rid}. A
// request of another org, or of another schedule, is not found.
func getScheduleRequestHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		rid := quarrycontracts.ID(chi.URLParam(r, "rid"))
		if err := rid.MustKind(quarrycontracts.KindRequest); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		reqs, ok := scheduleRequestsOf(db)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported, "schedule requests are not available on this store", nil)
			return
		}
		v, ok := reqs.GetByOrg(rid, org)
		if !ok || v.ScheduleID != id {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": rid})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, toScheduleRequestView(v))
	}
}

type claimRequestsBody struct {
	Worker string `json:"worker"`
	Limit  int    `json:"limit"`
	LeaseS int    `json:"lease_s"`
}

type claimRequestsResult struct {
	Requests []claimedRequestView `json:"requests"`
}

// claimScheduleRequestsHandler answers POST /v1/internal/schedules/requests/claim
// (orchestrator only, HMAC signed with the control-internal secret).
func claimScheduleRequestsHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var in claimRequestsBody
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		in.Worker = strings.TrimSpace(in.Worker)
		if in.Worker == "" || len(in.Worker) > workerNameMax {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "worker is required (max 128 characters)", nil)
			return
		}
		limit := in.Limit
		if limit <= 0 {
			limit = claimLimitDefault
		}
		if limit > claimLimitMax {
			limit = claimLimitMax
		}
		lease := time.Duration(in.LeaseS) * time.Second
		if in.LeaseS <= 0 {
			lease = claimLeaseDefault
		}
		if lease < claimLeaseMin {
			lease = claimLeaseMin
		}
		if lease > claimLeaseMax {
			lease = claimLeaseMax
		}
		reqs, ok := scheduleRequestsOf(db)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported, "schedule requests are not available on this store", nil)
			return
		}
		claimed, err := reqs.Claim(in.Worker, limit, lease, time.Now())
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "claim: "+err.Error(), nil)
			return
		}
		out := claimRequestsResult{Requests: make([]claimedRequestView, 0, len(claimed))}
		for _, c := range claimed {
			out.Requests = append(out.Requests, claimedRequestView{
				ID:         string(c.ID),
				ScheduleID: string(c.ScheduleID),
				OrgID:      c.OrgID,
				Action:     c.Action,
				Params:     c.Params,
				Attempts:   c.Attempts,
				LeaseUntil: c.LeaseUntil,
			})
		}
		httpx.WriteJSON(w, r, http.StatusOK, out)
	}
}

// Result statuses the orchestrator may post. retry gives the row back to the
// queue (a transient Temporal error); the others end it.
const scheduleRequestRetry = "retry"

type requestResultBody struct {
	Worker string         `json:"worker"`
	Status string         `json:"status"`
	Result map[string]any `json:"result,omitempty"`
	Error  string         `json:"error,omitempty"`
}

// scheduleRequestResultHandler answers POST /v1/internal/schedules/requests/{rid}/result.
// Only the worker that holds the claim can post; a late post after the lease was
// reclaimed by another worker is 409 and changes nothing.
func scheduleRequestResultHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		rid := quarrycontracts.ID(chi.URLParam(r, "rid"))
		if err := rid.MustKind(quarrycontracts.KindRequest); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		var in requestResultBody
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		in.Worker = strings.TrimSpace(in.Worker)
		if in.Worker == "" || len(in.Worker) > workerNameMax {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "worker is required (max 128 characters)", nil)
			return
		}
		reqs, ok := scheduleRequestsOf(db)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported, "schedule requests are not available on this store", nil)
			return
		}
		now := time.Now()
		var err error
		switch in.Status {
		case store.ScheduleRequestDone:
			err = reqs.Complete(rid, in.Worker, in.Result, now)
		case store.ScheduleRequestFailed, store.ScheduleRequestRejected:
			err = reqs.Fail(rid, in.Worker, in.Status, in.Error, now)
		case scheduleRequestRetry:
			err = reqs.Release(rid, in.Worker, in.Error)
		default:
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "status must be done, failed, rejected or retry", nil)
			return
		}
		switch {
		case err == nil:
			w.WriteHeader(http.StatusNoContent)
		case errors.Is(err, store.ErrNotFound):
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": rid})
		case errors.Is(err, store.ErrConflict):
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "the request is not held by this worker", map[string]any{"id": rid})
		default:
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "result: "+err.Error(), nil)
		}
	}
}
