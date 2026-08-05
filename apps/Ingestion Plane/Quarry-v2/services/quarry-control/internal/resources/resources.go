// Package resources mounts REST routes for control-plane resources.
// Each resource gets list / get / create / history (where applicable).
package resources

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func MountJobs(r chi.Router, db store.DB) {
	r.Route("/v1/jobs", func(r chi.Router) {
		r.Get("/", listJobs(db))
		r.Post("/", createJob(db))
		r.Get("/{id}", getJob(db))
		r.Put("/{id}", updateJob(db))
		r.Get("/{id}/events", jobEvents(db))
		r.Get("/{id}/history", jobHistory(db))
	})
}

func MountStores(r chi.Router, db store.DB) {
	mountSimple[store.NamedStore](r, "/v1/stores", db.Stores())
}
func MountSnapshots(r chi.Router, db store.DB) {
	mountSimple[store.Snapshot](r, "/v1/snapshots", db.Snapshots())
}
func MountArtifacts(r chi.Router, db store.DB) {
	mountSimple[store.Artifact](r, "/v1/artifacts", db.Artifacts())
}
func MountProfiles(r chi.Router, db store.DB) {
	mountSimple[store.BrowserProfile](r, "/v1/profiles", db.Profiles())
}
func MountSchedules(r chi.Router, db store.DB) {
	s := db.Schedules()
	// Schedules use explicit handlers (not mountSimple) because the
	// list/get responses must carry the DERIVED orchestrator contract
	// fields (workflow/args/paused) via scheduleWire — the reconciler
	// reads GET /v1/schedules to learn which Temporal workflow to run.
	r.Route("/v1/schedules", func(r chi.Router) {
		r.Get("/", listSchedulesHandler(s))
		r.Post("/", createScheduleHandler(s))
		r.Get("/{id}", getScheduleHandler(s))
		r.Delete("/{id}", deleteScheduleHandler(s))
	})
	r.Post("/v1/schedules/{id}/enable", scheduleSetEnabled(s, true))
	r.Post("/v1/schedules/{id}/disable", scheduleSetEnabled(s, false))
	r.Get("/v1/schedules/{id}/runs", scheduleRuns(db))
}

// listSchedulesHandler returns the schedule list projected through
// scheduleWire so the orchestrator reconciler sees workflow/args/paused.
func listSchedulesHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, _ := s.List(limit, r.URL.Query().Get("cursor"))
		out := make([]scheduleWire, 0, len(items))
		for _, it := range items {
			out = append(out, toScheduleWire(it))
		}
		httpx.WriteJSON(w, r, http.StatusOK, out)
	}
}

func getScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		v, ok := s.Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, toScheduleWire(v))
	}
}

// deleteScheduleHandler soft-removes a schedule. The orchestrator reconciler
// reaps the matching Temporal schedule on its next pass (the deleted row drops
// out of desiredSet). Restores the DELETE the generic mountSimple used to
// provide before MountSchedules moved to explicit wire-shaped handlers.
func deleteScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := s.Delete(id); err != nil {
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

// createScheduleHandler decodes + validates a schedule (mapping the
// change_monitor preset to a 5-field cron and requiring org_id) and returns
// the created record in wire shape. org_id is expected to already be stamped
// from the edge's verified JWT — Validate rejects an empty org_id.
func createScheduleHandler(s store.SchedulesStore) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var v store.Schedule
		if err := json.NewDecoder(r.Body).Decode(&v); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if err := v.Validate(); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if err := s.Create(v); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusCreated, toScheduleWire(v))
	}
}

// scheduleRuns returns the run history (jobs) triggered by a schedule.
// Contract: GET /v1/schedules/{id}/runs?limit=&cursor=. Scoped to the
// edge-verified `?org_id`: SchedulesStore has no dedicated GetByOrg (Get +
// an OrgID equality check is the same guard), and a schedule ID mismatch
// is reported identically to a genuinely missing one — mirrors the
// Source/Job GetByOrg pattern.
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
		sched, ok := db.Schedules().Get(id)
		if !ok || sched.OrgID != org {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, next := db.Jobs().ListBySchedule(id, limit, r.URL.Query().Get("cursor"))
		if items == nil {
			items = []store.Job{}
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"items":       items,
			"next_cursor": next,
		})
	}
}

func scheduleSetEnabled(s store.SchedulesStore, enabled bool) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := s.UpdateEnabled(id, enabled); err != nil {
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

func MountEvents(r chi.Router, db store.DB, apiKey string, sink notify.Sink) {
	// GET /v1/runs/{id}/events backs quarry-edge's list_run_events fallback
	// (crates/quarry-edge/src/resource_routes.rs), which deserializes every
	// item strictly as quarry_core::job_history::JobHistoryEvent — org_id
	// and kind are required fields that a bare quarrycontracts.Event has no
	// equivalent for. Resolve the owning job via GetByRunID once (every
	// event in a run shares one org_id/kind) and translate each row through
	// toJobHistoryEvent (event_wire.go) rather than serializing the legacy
	// Event shape directly.
	r.Get("/v1/runs/{id}/events", func(w http.ResponseWriter, r *http.Request) {
		runID := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := runID.MustKind(quarrycontracts.KindRun); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		owner, ok := db.Jobs().GetByRunID(runID)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "no job dispatched this run_id", nil)
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 100
		}
		evts := db.Events().ForRun(runID, after, limit)
		wire := make([]jobHistoryEventWire, 0, len(evts))
		for _, evt := range evts {
			wire = append(wire, toJobHistoryEvent(evt, owner))
		}
		httpx.WriteJSON(w, r, http.StatusOK, wire)
	})

	// Append-only event log (batch). Runtime + orchestrator post here.
	// Protected by bearer auth when apiKey is non-empty.
	r.With(httpx.BearerAuth(apiKey)).Post("/v1/runs/{id}/events", func(w http.ResponseWriter, r *http.Request) {
		runID := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := runID.MustKind(quarrycontracts.KindRun); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		// Cap the body at 1 MB. Without this a malicious caller can
		// stream an arbitrarily large JSON array and pin a goroutine
		// in `Decode`. Real event batches are tens of KB at most.
		const maxEventBatchBytes = 1 << 20
		r.Body = http.MaxBytesReader(w, r.Body, maxEventBatchBytes)
		var batch []quarrycontracts.Event
		if err := json.NewDecoder(r.Body).Decode(&batch); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if len(batch) == 0 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "empty batch", nil)
			return
		}
		// Pre-fetch the next per-run seq once and assign monotonically
		// within the batch. Callers do not share one sequence contract:
		// workflow activities usually emit seq=0, while quarry-edge's
		// runtime sink emits a process-wide counter. The control plane
		// owns durable per-run ordering, so never trust caller seq here.
		nextSeq := db.Events().NextSeq(runID)
		for i := range batch {
			if batch[i].Type == "" {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "event.type required", map[string]any{"index": i})
				return
			}
			if batch[i].EventID == "" {
				batch[i].EventID = quarrycontracts.NewID(quarrycontracts.KindEvent)
			}
			if batch[i].Timestamp.IsZero() {
				batch[i].Timestamp = time.Now().UTC()
			}
			batch[i].Seq = nextSeq
			nextSeq++
			rid := runID
			batch[i].RunID = &rid
			if err := db.Events().Append(batch[i]); err != nil {
				if errors.Is(err, store.ErrConflict) {
					// Idempotent replay: a publisher may retry a batch
					// after a partial success. Keep accepting the rest
					// instead of returning 500 and trapping the publisher
					// in an infinite retry loop.
					continue
				}
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), map[string]any{"index": i})
				return
			}
			markJobTerminalIfNeeded(db, batch[i])
			fanoutWebhooks(db, batch[i])
			notifyOnChange(sink, batch[i])
		}
		httpx.WriteJSON(w, r, http.StatusCreated, map[string]any{"accepted": len(batch)})
	})
}

// notifyOnChange delivers the ONE in-product notification for a detected
// change. The recipient is the schedule's creator (payload.created_by, set by
// ChangeMonitorWF). Best-effort + synchronous so it isn't lost on shutdown,
// but it never fails the event append: a notification-core outage must not
// block change tracking or webhook fanout. Idempotent on event_id so the
// idempotent-replay path (ErrConflict → continue) never double-notifies.
func notifyOnChange(sink notify.Sink, ev quarrycontracts.Event) {
	if sink == nil || ev.Type != quarrycontracts.EvtChangeDetected {
		return
	}
	recipient, _ := ev.Payload["created_by"].(string)
	if strings.TrimSpace(recipient) == "" {
		// No creator recorded (legacy schedule / non-user path) — honest skip.
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := sink.NotifyChange(ctx, recipient, string(ev.EventID), ev.Payload); err != nil {
		log.Warn().Err(err).Str("event_id", string(ev.EventID)).Msg("change notification delivery failed")
	}
}

func markJobTerminalIfNeeded(db store.DB, ev quarrycontracts.Event) {
	if ev.JobID == nil {
		return
	}
	status, ok := jobStatusForEvent(ev.Type)
	if !ok {
		return
	}
	job, exists := db.Jobs().Get(*ev.JobID)
	if !exists {
		return
	}
	job.Status = status
	_ = db.Jobs().Update(job)
}

func jobStatusForEvent(eventType quarrycontracts.EventType) (string, bool) {
	switch eventType {
	case quarrycontracts.EvtRunCompleted:
		return "completed", true
	case quarrycontracts.EvtRunFailed:
		return "failed", true
	case quarrycontracts.EvtRunCancelled:
		return "cancelled", true
	default:
		return "", false
	}
}

func mountSimple[T any](r chi.Router, path string, s store.ResourceStore[T]) {
	r.Route(path, func(r chi.Router) {
		r.Get("/", func(w http.ResponseWriter, r *http.Request) {
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 50
			}
			items, _ := s.List(limit, r.URL.Query().Get("cursor"))
			httpx.WriteJSON(w, r, http.StatusOK, items)
		})
		r.Post("/", func(w http.ResponseWriter, r *http.Request) {
			var v T
			if err := json.NewDecoder(r.Body).Decode(&v); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
				return
			}
			if vv, ok := any(&v).(interface{ Validate() error }); ok {
				if err := vv.Validate(); err != nil {
					httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
					return
				}
			}
			if err := s.Create(v); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
				return
			}
			httpx.WriteJSON(w, r, http.StatusCreated, v)
		})
		r.Get("/{id}", func(w http.ResponseWriter, r *http.Request) {
			id := quarrycontracts.ID(chi.URLParam(r, "id"))
			v, ok := s.Get(id)
			if !ok {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteJSON(w, r, http.StatusOK, v)
		})
	})
}

// listJobs backs GET /v1/jobs. UNSCOPED BY DESIGN: quarry-orchestrator's
// jobs dispatcher (services/quarry-orchestrator/internal/jobs/dispatcher.go
// listJobs) polls this exact endpoint with NO org_id — it must see every
// tenant's `accepted` jobs to start the matching Temporal workflow, the
// same way the schedules reconciler polls GET /v1/schedules unscoped.
// There is no tenant-facing caller: quarry-edge never forwards to this
// bare path (only to the org-scoped GET /v1/{kind}/jobs — see
// MountJobsByKind), and the GraphQL `jobs` resolver is an inert stub that
// never reaches control. Scoping this endpoint would silently stop every
// job from ever leaving "accepted".
func listJobs(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, _ := db.Jobs().List(limit, r.URL.Query().Get("cursor"))
		httpx.WriteJSON(w, r, http.StatusOK, items)
	}
}

// createJob backs POST /v1/jobs. org_id is read from the edge-verified
// `?org_id` query param (never the request body) and stamped onto the
// job — mirrors createSourceHandler's guard, which rejects an empty org
// rather than let a job land unattributed to any tenant.
func createJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		var in createJobInput
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		policy, err := resolveJobPolicy(in)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		// Honor `Idempotency-Key` header: if a job with the same key
		// already exists, return it (200 OK) rather than creating a
		// duplicate (201). This lets verevon safely retry the POST on
		// a network hiccup, and turns a user's double-click into a
		// single crawl. Keys longer than 128 chars are rejected to
		// bound the index.
		idemKey := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
		if len(idemKey) > 128 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "idempotency key too long (max 128)", nil)
			return
		}
		if idemKey != "" {
			if existing, ok := db.Jobs().FindByIdempotencyKey(org, idemKey); ok {
				httpx.WriteJSON(w, r, http.StatusOK, existing)
				return
			}
		}
		job := store.Job{
			ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
			OrgID:     org,
			Kind:      in.Kind,
			Status:    "accepted",
			Policy:    policy,
			Params:    in.Params,
			CreatedAt: time.Now().UnixMilli(),
		}
		if idemKey != "" {
			k := idemKey
			job.IdempotencyKey = &k
		}
		if err := db.Jobs().Create(job); err != nil {
			// Race: another request with the same idempotency key
			// landed between our lookup and our Create. Re-fetch and
			// return that one.
			if idemKey != "" {
				if existing, ok := db.Jobs().FindByIdempotencyKey(org, idemKey); ok {
					httpx.WriteJSON(w, r, http.StatusOK, existing)
					return
				}
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusCreated, job)
	}
}

type createJobInput struct {
	Kind   string                     `json:"kind"`
	Policy *quarrycontracts.RunPolicy `json:"policy"`
	Preset string                     `json:"preset,omitempty"`
	Params map[string]any             `json:"params,omitempty"`
}

// resolveJobPolicy honors the precedence: explicit policy > preset > default.
// An unknown preset name is a 400 (BadRequest), not a silent fallback.
func resolveJobPolicy(in createJobInput) (quarrycontracts.RunPolicy, error) {
	if in.Policy != nil {
		return *in.Policy, nil
	}
	if in.Preset != "" {
		p, ok := ResolvePreset(in.Preset)
		if !ok {
			return quarrycontracts.RunPolicy{}, fmt.Errorf("unknown preset: %s", in.Preset)
		}
		return p, nil
	}
	return quarrycontracts.DefaultRunPolicy(), nil
}

// getJob backs GET /v1/jobs/{id}, scoped to the edge-verified `?org_id`
// query param via GetByOrg — a cross-tenant id reads as 404, same as
// getSourceHandler-style guards elsewhere in this package.
func getJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		v, ok := db.Jobs().GetByOrg(org, id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, v)
	}
}

// updateJob accepts a partial update of the mutable fields on a job
// (status + run_id). UNSCOPED BY DESIGN: it's called only by the
// orchestrator's jobs dispatcher (markRunning/markFailed in
// services/quarry-orchestrator/internal/jobs/dispatcher.go) to flip
// accepted → running/failed and stamp the Temporal run_id — a trusted
// internal service transition by job ID, not a tenant-facing read, and
// the dispatcher never sends org_id. The existing kind/params/policy/
// created_at fields are preserved so a caller can send
// `{"status": "running", "run_id": "run_…"}` without re-sending the
// original create payload.
func updateJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		existing, ok := db.Jobs().Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		var patch struct {
			Status *string                    `json:"status,omitempty"`
			RunID  *quarrycontracts.ID        `json:"run_id,omitempty"`
			Policy *quarrycontracts.RunPolicy `json:"policy,omitempty"`
		}
		if err := json.NewDecoder(r.Body).Decode(&patch); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if patch.Status != nil {
			existing.Status = *patch.Status
		}
		if patch.RunID != nil {
			existing.RunID = patch.RunID
		}
		if patch.Policy != nil {
			existing.Policy = *patch.Policy
		}
		if err := db.Jobs().Update(existing); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, existing)
	}
}

// jobEvents backs GET /v1/jobs/{id}/events. Confirms the id belongs to the
// caller's org via GetByOrg before returning any events — quarry-edge's
// list_job_events already sends the verified org_id on every call
// (resource_routes.rs's forward_one always includes it), so this adds no
// new requirement for that caller, only closes the gap for a direct or
// future caller that omits it.
func jobEvents(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if _, ok := db.Jobs().GetByOrg(org, id); !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 100
		}
		evts := db.Events().ForJob(id, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, evts)
	}
}

// jobHistory returns the merged view for a job: job record + its events.
// Contract §1.2: GET /v1/jobs/{id}/history. Scoped to the edge-verified
// `?org_id` via GetByOrg, same guard as getJob/jobEvents.
func jobHistory(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindJob); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		job, ok := db.Jobs().GetByOrg(org, id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 500
		}
		evts := db.Events().ForJob(id, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"job":    job,
			"events": evts,
		})
	}
}

// fanoutWebhooks creates pending WebhookDelivery rows for every active webhook
// whose Events filter matches the event. The dispatcher worker pool will claim
// and POST them with HMAC signing and retry/backoff.
func fanoutWebhooks(db store.DB, ev quarrycontracts.Event) {
	payloadBytes, err := json.Marshal(ev)
	if err != nil {
		return
	}
	whs, _ := db.Webhooks().List(1000, "")
	now := time.Now().Unix()
	evType := string(ev.Type)
	for _, wh := range whs {
		if !wh.Active {
			continue
		}
		matched := len(wh.Events) == 0
		if !matched {
			for _, et := range wh.Events {
				if et == "*" || et == evType {
					matched = true
					break
				}
			}
		}
		if !matched {
			continue
		}
		_ = db.WebhookDeliveries().Create(store.WebhookDelivery{
			ID:            quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
			WebhookID:     wh.ID,
			EventID:       ev.EventID,
			Payload:       string(payloadBytes),
			Status:        "pending",
			Attempt:       0,
			NextAttemptAt: now,
			CreatedAt:     now,
		})
	}
}
