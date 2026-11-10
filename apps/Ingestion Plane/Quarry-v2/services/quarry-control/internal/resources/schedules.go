package resources

import (
	"encoding/json"
	"errors"
	"net/http"
	"strconv"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// EnvSchedulesRequireOrg is retired (D2-042). GET /v1/schedules used to answer
// with every org's rows when no org_id was given, unless this variable was 1.
// That branch is deleted: a request without org_id is always refused, so the
// variable changes nothing. The name stays so compose can keep it at 1, which
// also fences an older control image that still honours it. The orchestrator
// reads GET /v1/internal/schedules/desired for the all-orgs set.
const EnvSchedulesRequireOrg = "QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG"

// MountSchedules mounts the schedule routes (WS15.S1). Every tenant route takes
// org_id from the edge-verified query and answers 404 for a row that belongs to
// another org, exactly as it answers for a missing row. The pause, unpause,
// trigger and backfill aliases are mounted by MountScheduleAliases (cycle23.go).
// The monitor tag (E3.5-6) rides create, PATCH /{id} and the ?tag= list filter.
func MountSchedules(r chi.Router, db store.DB) {
	s := db.Schedules()
	r.Route("/v1/schedules", func(r chi.Router) {
		r.Get("/", listSchedulesHandler(s))
		r.Post("/", createScheduleHandler(s))
		r.Get("/{id}", getScheduleHandler(s))
		r.Patch("/{id}", patchScheduleHandler(s))
		r.Delete("/{id}", deleteScheduleHandler(s))
	})
	r.Post("/v1/schedules/{id}/enable", scheduleSetEnabled(s, true))
	r.Post("/v1/schedules/{id}/disable", scheduleSetEnabled(s, false))
	r.Get("/v1/schedules/{id}/runs", scheduleRuns(db))
	// The orchestrator's all-orgs read. Internal path, wire shape.
	r.Get("/v1/internal/schedules/desired", desiredSchedulesHandler(s))
	// The orchestrator's fire registration (WS15.S2). HMAC signed, like the rest
	// of the internal surface.
	r.Post("/v1/internal/schedules/{id}/runs", registerScheduledRun(db))
}

// listSchedulesHandler answers GET /v1/schedules: a keyset page of the caller's
// own rows, in the view shape. org_id comes from the edge-verified query; a
// request without it is refused and never falls back to an all-orgs list.
func listSchedulesHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		cursor := r.URL.Query().Get("cursor")
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		tag, filtered, err := tagFilter(r)
		if err != nil {
			writeTagBadRequest(w, r, err)
			return
		}
		var items []store.Schedule
		var next string
		if filtered {
			items, next = s.ListByOrgTag(org, tag, limit, cursor)
		} else {
			items, next = s.ListByOrg(org, limit, cursor)
		}
		out := make([]scheduleView, 0, len(items))
		for _, it := range items {
			out = append(out, toScheduleView(it))
		}
		var nextCursor any
		if next != "" {
			nextCursor = next
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"items":       out,
			"next_cursor": nextCursor,
		})
	}
}

// desiredSchedulesHandler answers the orchestrator's reconcile read: every
// org's schedules in the wire shape (workflow, args, paused).
func desiredSchedulesHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		items, _ := s.ListDesired(limit, r.URL.Query().Get("cursor"))
		out := make([]scheduleWire, 0, len(items))
		for _, it := range items {
			out = append(out, toScheduleWire(it))
		}
		httpx.WriteJSON(w, r, http.StatusOK, out)
	}
}

// getScheduleHandler answers GET /v1/schedules/{id} for the caller's org.
func getScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		v, ok := s.GetByOrg(id, org)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, toScheduleView(v))
	}
}

// patchScheduleHandler answers PATCH /v1/schedules/{id} for the caller's org (E3.5-6). The
// body's tag replaces the schedule's tag ("" clears it); the answer is the updated view. The
// row's updated_at is not moved (it stays the create or enable/disable stamp). A row of
// another org is a 404, as a missing one is.
func patchScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org, ok := orgOr400(w, r)
		if !ok {
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		tag, ok := readTagPatch(w, r)
		if !ok {
			return
		}
		if err := s.UpdateTagByOrg(id, org, tag); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		v, ok := s.GetByOrg(id, org)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, toScheduleView(v))
	}
}

// deleteScheduleHandler soft-removes the caller's schedule. The orchestrator
// reconciler reaps the matching Temporal schedule on its next pass (the deleted
// row drops out of the desired set). A row of another org is a 404.
func deleteScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := s.DeleteByOrg(id, org); err != nil {
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

// createScheduleHandler creates a schedule for the caller's org (WS15.S1).
//
//   - org_id is read from the edge-verified query and never from the body. A
//     body org_id is ignored.
//   - A new schedule starts disabled. The body's activate:true starts it. The
//     body's enabled flag is not read. The edge sets activate for user tokens.
//   - The definition is validated (store.Schedule.Validate). A rejection is a
//     400 whose error.details.reason is the stable token, for example
//     unsupported_schedule_kind, invalid_timezone or cadence_too_frequent.
func createScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		var v store.Schedule
		if err := json.NewDecoder(r.Body).Decode(&v); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		v.OrgID = org
		v.Enabled = v.Activate
		// The optional monitor tag (E3.5-6) is normalised before the definition is
		// validated, so a bad tag is a 400 naming the field, not a store conflict.
		tag, err := store.NormalizeTag(v.Tag)
		if err != nil {
			writeTagBadRequest(w, r, err)
			return
		}
		v.Tag = tag
		if err := v.Validate(); err != nil {
			writeScheduleValidation(w, r, err)
			return
		}
		if err := s.Create(v); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusCreated, toScheduleView(v))
	}
}

// writeScheduleValidation answers a definition the store refuses: a 400 with the
// reason in details when the error carries one.
func writeScheduleValidation(w http.ResponseWriter, r *http.Request, err error) {
	var details map[string]any
	var ve *store.ValidationError
	if errors.As(err, &ve) {
		details = map[string]any{"reason": ve.Reason}
	}
	httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), details)
}

// scheduleRuns returns the run history (jobs) triggered by a schedule.
// Contract: GET /v1/schedules/{id}/runs?org_id=&limit=&cursor=. Scoped to the
// edge-verified org: a schedule of another org is reported exactly as a
// genuinely missing one.
func scheduleRuns(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		sched, ok := db.Schedules().GetByOrg(id, org)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, next := db.Jobs().ListBySchedule(sched.ID, limit, r.URL.Query().Get("cursor"))
		if items == nil {
			items = []store.Job{}
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"items":       items,
			"next_cursor": next,
		})
	}
}

// scheduleSetEnabled answers enable, disable, pause and unpause for the caller's
// org. It stamps updated_at, so a re-enabled row is never disabled again by the
// migration guard (migration 017).
func scheduleSetEnabled(s store.SchedulesStore, enabled bool) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := s.UpdateEnabledByOrg(id, org, enabled); err != nil {
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
