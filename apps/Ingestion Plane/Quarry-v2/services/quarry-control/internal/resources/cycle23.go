// Cycle 23 — REST resource breadth + schedule alias routes + HMAC-gated
// internal endpoints. Adds the routes the Rust edge expects:
//
//   - /v1/sources              (real org-scoped CRUD over quarry_sources)
//   - /v1/team/* aggregates    → moved to cycle24.go (real store-backed reads)
//   - /v1/team/activity        → moved to cycle24.go
//   - /v1/schedules/:id/pause   (alias for /disable)
//   - /v1/schedules/:id/unpause (alias for /enable)
//   - /v1/schedules/:id/trigger  (proxies to Temporal when wired; 501 otherwise)
//   - /v1/schedules/:id/backfill (proxies to Temporal when wired; 501 otherwise)
//
// /v1/benchmarks was removed in cycle 28 follow-up: with no source
// of truth (no `quarry_benchmarks` table, no `lab/evals` table reader
// in the wire path) the empty-page response was a 200-OK lie. Per
// the gap-quarry honesty rule, the route was deleted instead of
// shipped with fake data; the edge `forward_list` will tolerate the
// 404 by returning an empty `Page<>`.
//
// All routes follow the existing pagination contract: response shape
// `{items, next_cursor?, total_estimated?}` matching
// `quarry-core::pagination::Page<T>` byte-for-byte so the Rust edge's
// `forward_list<T>` deserializer doesn't need branching.

package resources

import (
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	temporalclient "github.com/triodelab/quarry-v2/services/quarry-control/internal/temporal"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// page mirrors Rust `quarry_core::pagination::Page<T>` exactly so the
// edge's forward_list deserializer doesn't need a separate Go-flavor
// envelope.
type page struct {
	Items          interface{} `json:"items"`
	NextCursor     *string     `json:"next_cursor,omitempty"`
	TotalEstimated *uint64     `json:"total_estimated,omitempty"`
}

// writePage emits a Page<T> envelope wrapped in the control-plane
// response shape. Empty pages render as `{items: [], total_estimated: 0}`.
func writePage(w http.ResponseWriter, r *http.Request, items interface{}, total *uint64, next *string) {
	httpx.WriteJSON(w, r, http.StatusOK, page{
		Items:          items,
		NextCursor:     next,
		TotalEstimated: total,
	})
}

func emptyPage(w http.ResponseWriter, r *http.Request) {
	zero := uint64(0)
	writePage(w, r, []struct{}{}, &zero, nil)
}

// =============================================================================
// /v1/sources — durable record of recurring ingestion targets.
// Cycle 23: schema lives in `005_sources.sql` (this commit). Handler
// reads via a typed store.
// =============================================================================

// sourceCreateBody is the POST /v1/sources request shape. org_id is NOT read
// from the body — the edge stamps the verified org as the `?org_id` query
// param (mirroring the list/forward_json contract), so a client can never
// register a source under another tenant.
type sourceCreateBody struct {
	Name string `json:"name"`
	URL  string `json:"url"`
	Kind string `json:"kind"` // crawl | scrape | search
	// Monitor, when true, also registers a recurring change_monitor schedule
	// (preset-driven) so the orchestrator reconcile materializes a Temporal
	// schedule for this source. Sources without it are durable records only.
	Monitor bool           `json:"monitor,omitempty"`
	Preset  string         `json:"preset,omitempty"` // hourly|daily|weekly when Monitor
	Config  map[string]any `json:"config,omitempty"`
}

var validSourceKinds = map[string]bool{"crawl": true, "scrape": true, "search": true}

// MountSources registers the /v1/sources CRUD routes backed by the durable
// `quarry_sources` table (migration 005). EVERY op is org-scoped: the org id
// is the verified `?org_id` the edge stamps from the JWT, never a body field,
// so there is no cross-tenant IDOR vector.
//
//   - GET    /v1/sources      — list the org's live sources (Page<Source>).
//   - POST   /v1/sources      — create a source (mints src_<ulid>); optionally
//     registers a change_monitor schedule so the orchestrator drives recurring
//     refresh through Temporal.
//   - DELETE /v1/sources/{id} — soft-delete (sets deleted_at), org-scoped.
func MountSources(r chi.Router, db store.DB) {
	r.Get("/v1/sources", listSourcesHandler(db))
	r.Post("/v1/sources", createSourceHandler(db))
	r.Delete("/v1/sources/{id}", deleteSourceHandler(db))
}

// orgFromQuery reads the verified org the edge stamped as `?org_id`. Empty is
// rejected by callers — an unscoped sources query must never succeed.
func orgFromQuery(r *http.Request) string {
	return strings.TrimSpace(r.URL.Query().Get("org_id"))
}

func listSourcesHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, next := db.Sources().ListByOrg(org, limit, r.URL.Query().Get("cursor"))
		if items == nil {
			items = []store.Source{}
		}
		var nextPtr *string
		if next != "" {
			nextPtr = &next
		}
		writePage(w, r, items, nil, nextPtr)
	}
}

func createSourceHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		var body sourceCreateBody
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		body.Name = strings.TrimSpace(body.Name)
		body.URL = strings.TrimSpace(body.URL)
		body.Kind = strings.TrimSpace(body.Kind)
		if body.Name == "" || body.URL == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "name and url are required", nil)
			return
		}
		if !validSourceKinds[body.Kind] {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "kind must be one of crawl|scrape|search", nil)
			return
		}
		now := time.Now().UnixMilli()
		src := store.Source{
			ID:        quarrycontracts.NewID(quarrycontracts.KindSource),
			OrgID:     org,
			Name:      body.Name,
			URL:       body.URL,
			Kind:      body.Kind,
			Status:    "active",
			Config:    body.Config,
			CreatedAt: now,
			UpdatedAt: now,
		}
		// 2026-07-20 Aquatiq crawl-to-KB audit fix: upsert-by-(org_id, url)
		// instead of a plain insert. quarry-runtime's crawl pipeline now calls
		// this endpoint once per successfully ingested page (best-effort,
		// from PageRunner's SourceRegistrar), so repeat calls for the same
		// website — one per page of a multi-page crawl, or a re-run of the
		// same crawl — MUST collapse into a single durable "tracked website"
		// row rather than duplicating it once per page. The manual
		// Ingestions-page registration flow goes through this same handler
		// and benefits identically: registering the same URL twice no longer
		// creates two rows.
		result, created, err := db.Sources().UpsertByOrgAndURL(src)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		// Optionally register a recurring change_monitor schedule for this
		// source. The orchestrator reconcile (org-scoped) then materializes a
		// Temporal schedule; on a detected change the W2 notify leg fires. We
		// stamp the SAME verified org so the schedule + source stay tenant-aligned.
		// Only on the newly-inserted branch — an idempotent repeat
		// registration (e.g. the crawl pipeline's per-page calls) must not
		// spin up a duplicate schedule every time.
		if body.Monitor && created {
			preset := strings.TrimSpace(body.Preset)
			if preset == "" {
				preset = "daily"
			}
			sched := store.Schedule{
				OrgID:      org,
				TargetKind: store.TargetKindChangeMonitor,
				TargetRef:  body.URL,
				Enabled:    true,
				Preset:     preset,
			}
			if err := sched.Validate(); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "monitor: "+err.Error(), nil)
				return
			}
			if err := db.Schedules().Create(sched); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "monitor schedule: "+err.Error(), nil)
				return
			}
		}
		status := http.StatusCreated
		if !created {
			status = http.StatusOK
		}
		httpx.WriteJSON(w, r, status, result)
	}
}

func deleteSourceHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSource); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if err := db.Sources().SoftDeleteByOrg(org, id); err != nil {
			// ErrNotFound covers BOTH a genuinely missing id AND a cross-tenant
			// delete attempt — they are indistinguishable by design.
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}
}

// =============================================================================
// /v1/benchmarks — REMOVED (cycle 28 follow-up).
//
// The previous implementation returned an empty `Page<>` for every
// call. Without a `quarry_benchmarks` table or a `lab/evals`
// reader on the wire path, that was a 200 OK lie. The edge
// `forward_list` deserializes a 404 as an empty `Page<>` with no
// items, so deleting the route here is the safe option. If/when a
// benchmark corpus lands it gets re-introduced with a real
// store-backed read.
// =============================================================================


func pickQuery(r *http.Request, name, def string) string {
	v := r.URL.Query().Get(name)
	if v == "" {
		return def
	}
	return v
}

// =============================================================================
// Schedule aliases — pause/unpause/trigger/backfill — map onto the
// existing enable/disable + a Temporal client for trigger/backfill.
// =============================================================================

// MountScheduleAliases adds the cycle 23 lifecycle endpoint names on
// top of the existing /enable + /disable routes. The Rust edge speaks
// pause/unpause/trigger/backfill; we accept those forms so callers
// don't see a vocabulary mismatch.
//
// `tc` may be nil — in that case trigger/backfill return 501 with a
// typed `UNSUPPORTED` envelope (per the gap-quarry honesty rule:
// never 202-fake an effect that was never executed). Production
// wires a real `temporal.SDKClient` once `go.temporal.io/sdk` is in
// go.mod; dev/test environments may pass nil.
func MountScheduleAliases(r chi.Router, db store.DB, tc temporalclient.Client) {
	s := db.Schedules()
	// pause == disable; unpause == enable. The wire shapes match —
	// both return 204 NoContent.
	r.Post("/v1/schedules/{id}/pause", scheduleSetEnabled(s, false))
	r.Post("/v1/schedules/{id}/unpause", scheduleSetEnabled(s, true))
	// trigger + backfill are Temporal-owned operations. When the
	// client is wired we proxy through to it; when it isn't we
	// surface an honest 501 rather than a fake 202.
	r.Post("/v1/schedules/{id}/trigger", scheduleTrigger(db, tc))
	r.Post("/v1/schedules/{id}/backfill", scheduleBackfill(db, tc))
}

// scheduleTrigger calls the real Temporal client when one is
// configured, and returns 501 otherwise. The previous implementation
// always returned 202 with a "Temporal SDK not yet wired" note — that
// was a hand-wave the gap-quarry audit called out as "accepted but
// not executed" under-delivery.
func scheduleTrigger(db store.DB, tc temporalclient.Client) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		sched, ok := db.Schedules().Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		if tc == nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported,
				"trigger requires the Temporal client; set QUARRY_TEMPORAL_HOSTPORT or pass --temporal",
				map[string]any{"hint": "configure Temporal then restart control"})
			return
		}
		if err := tc.Trigger(r.Context(), sched.OrgID, string(sched.ID)); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "temporal trigger: "+err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"schedule_id": sched.ID,
			"status":      "triggered",
		})
	}
}

type backfillBody struct {
	StartAt       time.Time `json:"start_at"`
	EndAt         time.Time `json:"end_at"`
	OverlapPolicy string    `json:"overlap_policy"`
}

func scheduleBackfill(db store.DB, tc temporalclient.Client) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindSchedule); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		sched, ok := db.Schedules().Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		var body backfillBody
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if !body.EndAt.After(body.StartAt) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "end_at must be strictly after start_at", nil)
			return
		}
		if tc == nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnsupported,
				"backfill requires the Temporal client; set QUARRY_TEMPORAL_HOSTPORT or pass --temporal",
				map[string]any{"hint": "configure Temporal then restart control"})
			return
		}
		err := tc.Backfill(r.Context(), temporalclient.BackfillOptions{
			OrgID:         sched.OrgID,
			ScheduleID:    string(sched.ID),
			StartAt:       body.StartAt,
			EndAt:         body.EndAt,
			OverlapPolicy: body.OverlapPolicy,
		})
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "temporal backfill: "+err.Error(), nil)
			return
		}
		windowSecs := int64(body.EndAt.Sub(body.StartAt).Seconds())
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"schedule_id":    sched.ID,
			"status":         "backfill-queued",
			"window_secs":    windowSecs,
			"overlap_policy": body.OverlapPolicy,
		})
	}
}

// =============================================================================
// Per-kind job lists — /v1/{crawl,search,extract,research,agent,batch,scrape}/jobs
//
// Each alias filters to its own kind via JobsStore.ListByKind (the `kind`
// column + jobs_kind_status_created_idx index have existed since the very
// first jobs migration — this was just never wired up), and projects the
// result through job_wire.go's toJobWire so the response matches
// quarry_core::resources::JobSummary field-for-field.
// =============================================================================

func MountJobsByKind(r chi.Router, db store.DB) {
	kinds := []string{"crawl", "search", "extract", "research", "agent", "batch", "scrape"}
	for _, k := range kinds {
		kind := k // capture
		r.Get("/v1/"+kind+"/jobs", func(w http.ResponseWriter, r *http.Request) {
			orgID := orgFromQuery(r)
			if orgID == "" {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
				return
			}
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 25
			}
			raw, next := db.Jobs().ListByKind(orgID, kind, limit, r.URL.Query().Get("cursor"))
			items := make([]jobWire, 0, len(raw))
			for _, j := range raw {
				if wire, ok := toJobWire(j); ok {
					items = append(items, wire)
				}
			}
			var nextPtr *string
			if next != "" {
				nextPtr = &next
			}
			writePage(w, r, items, nil, nextPtr)
		})
	}
}

// =============================================================================
// IdempotencyKey middleware
//
// D3 / cluster #14 — POST routes that mutate state honour the
// `Idempotency-Key` header. The middleware records the (org_id, key,
// route) tuple in memory for the 24h TTL window; a repeated request
// with the same key short-circuits with the original response (or a
// 409 if the original is still in flight).
//
// In-memory impl is suitable for single-instance dev; production
// migrates to the `quarry_idempotency_keys` table (migration 005).
// =============================================================================

// IdempotencyKeyHandler is a placeholder hook that handlers MAY call
// to dedupe. The real implementation comes in cycle 24 when the
// pg-backed table is wired; today the function is a no-op so the
// edge can already send the header without breaking anything.
func IdempotencyKeyHandler(_ http.ResponseWriter, _ *http.Request) bool {
	return false
}
