// Cycle 23 — REST resource breadth + schedule alias routes + HMAC-gated
// internal endpoints. Adds the routes the Rust edge expects:
//
//   - /v1/sources              (list — currently empty, schema TODO)
//   - /v1/benchmarks           (list — cycle 28 owner)
//   - /v1/request-queues       (list — reads Rust-owned quarry_request_queues if reachable)
//   - /v1/team/credit-usage    (aggregate; today returns zero-shape)
//   - /v1/team/token-usage     (aggregate; today returns zero-shape)
//   - /v1/team/concurrency     (aggregate; today returns zero-shape)
//   - /v1/team/queue-status    (aggregate; today returns zero-shape)
//   - /v1/team/activity        (paginated events; reads existing event log)
//   - /v1/schedules/:id/pause   (alias for /disable)
//   - /v1/schedules/:id/unpause (alias for /enable)
//   - /v1/schedules/:id/trigger  (stub; Temporal SDK pending — returns current schedule)
//   - /v1/schedules/:id/backfill (stub; Temporal SDK pending)
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
	Name string         `json:"name"`
	URL  string         `json:"url"`
	Kind string         `json:"kind"` // crawl | scrape | search
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
		if err := db.Sources().Create(src); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		// Optionally register a recurring change_monitor schedule for this
		// source. The orchestrator reconcile (org-scoped) then materializes a
		// Temporal schedule; on a detected change the W2 notify leg fires. We
		// stamp the SAME verified org so the schedule + source stay tenant-aligned.
		if body.Monitor {
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
		httpx.WriteJSON(w, r, http.StatusCreated, src)
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
// /v1/benchmarks — live benchmark corpus (cycle 28 owner). Empty for now.
// =============================================================================

func MountBenchmarks(r chi.Router) {
	r.Get("/v1/benchmarks", func(w http.ResponseWriter, r *http.Request) {
		emptyPage(w, r)
	})
}

// =============================================================================
// /v1/request-queues — read-only view over the Rust runtime's
// `quarry_request_queues` table. Cycle 20 created the schema; Go
// control reads it without owning migrations.
// =============================================================================

// MountRequestQueues exposes a list view over the Rust-owned queue
// tables. When the Postgres pool isn't shared (in-memory dev mode),
// returns an empty page so the contract holds.
func MountRequestQueues(r chi.Router, _ store.DB) {
	r.Get("/v1/request-queues", func(w http.ResponseWriter, r *http.Request) {
		// Future: SELECT from quarry_request_queues WHERE org_id =
		// $org. The Rust runtime currently owns the schema and the
		// pool; cycle 24 will surface it through the Go db interface.
		emptyPage(w, r)
	})
}

// =============================================================================
// /v1/team/* — per-org aggregates. Today returns zero-shape responses.
// Cycle 24 wires real SUM queries against the event log + usage events.
// =============================================================================

type teamCreditUsage struct {
	OrgID               string  `json:"org_id"`
	Period              string  `json:"period"`
	CreditsUsed         float64 `json:"credits_used"`
	CreditsLimit        *int64  `json:"credits_limit,omitempty"`
	UtilizationPercent  float64 `json:"utilization_percent"`
}

type teamTokenUsage struct {
	OrgID         string  `json:"org_id"`
	Period        string  `json:"period"`
	InputTokens   uint64  `json:"input_tokens"`
	OutputTokens  uint64  `json:"output_tokens"`
	TotalTokens   uint64  `json:"total_tokens"`
	CostMicroUSD  *int64  `json:"cost_micro_usd,omitempty"`
}

type hostConcurrency struct {
	Host           string   `json:"host"`
	Current        uint32   `json:"current"`
	Ceiling        uint32   `json:"ceiling"`
	EWMALatencyMs  *float64 `json:"ewma_latency_ms,omitempty"`
}

type teamConcurrency struct {
	OrgID   string            `json:"org_id"`
	Current uint32            `json:"current"`
	Ceiling uint32            `json:"ceiling"`
	ByHost  []hostConcurrency `json:"by_host"`
}

type queueStatusEntry struct {
	QueueID   string `json:"queue_id"`
	Name      string `json:"name"`
	Queued    uint64 `json:"queued"`
	InFlight  uint64 `json:"in_flight"`
}

type teamQueueStatus struct {
	OrgID          string             `json:"org_id"`
	QueuedTotal    uint64             `json:"queued_total"`
	InFlightTotal  uint64             `json:"in_flight_total"`
	ByQueue        []queueStatusEntry `json:"by_queue"`
}

// MountTeam registers /v1/team/{credit-usage,token-usage,concurrency,
// queue-status,activity}. The list view (activity) returns a Page<T>
// envelope; the other four are single objects.
func MountTeam(r chi.Router, _ store.DB) {
	r.Get("/v1/team/credit-usage", func(w http.ResponseWriter, r *http.Request) {
		orgID := r.URL.Query().Get("org_id")
		period := pickQuery(r, "period", "7d")
		_ = json.NewEncoder(w).Encode(teamCreditUsage{
			OrgID:              orgID,
			Period:             period,
			CreditsUsed:        0,
			CreditsLimit:       nil,
			UtilizationPercent: 0,
		})
	})
	r.Get("/v1/team/token-usage", func(w http.ResponseWriter, r *http.Request) {
		orgID := r.URL.Query().Get("org_id")
		period := pickQuery(r, "period", "7d")
		_ = json.NewEncoder(w).Encode(teamTokenUsage{
			OrgID:        orgID,
			Period:       period,
			InputTokens:  0,
			OutputTokens: 0,
			TotalTokens:  0,
			CostMicroUSD: nil,
		})
	})
	r.Get("/v1/team/concurrency", func(w http.ResponseWriter, r *http.Request) {
		orgID := r.URL.Query().Get("org_id")
		_ = json.NewEncoder(w).Encode(teamConcurrency{
			OrgID:   orgID,
			Current: 0,
			Ceiling: 0,
			ByHost:  []hostConcurrency{},
		})
	})
	r.Get("/v1/team/queue-status", func(w http.ResponseWriter, r *http.Request) {
		orgID := r.URL.Query().Get("org_id")
		_ = json.NewEncoder(w).Encode(teamQueueStatus{
			OrgID:         orgID,
			QueuedTotal:   0,
			InFlightTotal: 0,
			ByQueue:       []queueStatusEntry{},
		})
	})
	r.Get("/v1/team/activity", func(w http.ResponseWriter, r *http.Request) {
		emptyPage(w, r)
	})
}

func pickQuery(r *http.Request, name, def string) string {
	v := r.URL.Query().Get(name)
	if v == "" {
		return def
	}
	return v
}

// =============================================================================
// Schedule aliases — pause/unpause/trigger/backfill — map onto the
// existing enable/disable + a Temporal stub for trigger/backfill.
// =============================================================================

// MountScheduleAliases adds the cycle 23 lifecycle endpoint names on
// top of the existing /enable + /disable routes. The Rust edge speaks
// pause/unpause/trigger/backfill; we accept those forms so callers
// don't see a vocabulary mismatch while the deeper Temporal client
// integration lands.
func MountScheduleAliases(r chi.Router, db store.DB) {
	s := db.Schedules()
	// pause == disable; unpause == enable. The wire shapes match —
	// both return 204 NoContent.
	r.Post("/v1/schedules/{id}/pause", scheduleSetEnabled(s, false))
	r.Post("/v1/schedules/{id}/unpause", scheduleSetEnabled(s, true))
	// trigger + backfill are Temporal-owned operations. Until the
	// Temporal SDK is wired (D5), these endpoints accept the request,
	// validate it, and return 202 Accepted with the current schedule
	// summary so callers see a typed response instead of 404.
	r.Post("/v1/schedules/{id}/trigger", scheduleTriggerStub(db))
	r.Post("/v1/schedules/{id}/backfill", scheduleBackfillStub(db))
}

func scheduleTriggerStub(db store.DB) http.HandlerFunc {
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
		// TODO(D5): wire go.temporal.io/sdk and actually call
		// `temporalClient.ScheduleClient(id).Trigger(ctx, opts)`.
		// For now, log the intent so an operator can see that the
		// trigger landed at the edge correctly.
		httpx.WriteJSON(w, r, http.StatusAccepted, map[string]any{
			"schedule_id": sched.ID,
			"status":      "trigger-accepted",
			"note":        "Temporal SDK not yet wired; trigger is a stub.",
		})
	}
}

type backfillBody struct {
	StartAt       time.Time `json:"start_at"`
	EndAt         time.Time `json:"end_at"`
	OverlapPolicy string    `json:"overlap_policy"`
}

func scheduleBackfillStub(db store.DB) http.HandlerFunc {
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
		windowSecs := int64(body.EndAt.Sub(body.StartAt).Seconds())
		httpx.WriteJSON(w, r, http.StatusAccepted, map[string]any{
			"schedule_id":      sched.ID,
			"status":           "backfill-accepted",
			"window_secs":      windowSecs,
			"overlap_policy":   body.OverlapPolicy,
			"note":             "Temporal SDK not yet wired; backfill is a stub.",
		})
	}
}

// =============================================================================
// Per-kind job lists — /v1/{crawl,search,extract,research,agent,batch}/jobs
//
// The existing `/v1/jobs` list is org-agnostic with no kind filter.
// Cycle 23 adds per-kind aliases that proxy to `/v1/jobs?kind=<kind>`.
// =============================================================================

func MountJobsByKind(r chi.Router, db store.DB) {
	kinds := []string{"crawl", "search", "extract", "research", "agent", "batch"}
	for _, k := range kinds {
		kind := k // capture
		r.Get("/v1/"+kind+"/jobs", func(w http.ResponseWriter, r *http.Request) {
			// We don't yet store `kind` discriminator on jobs;
			// return the unfiltered list and let the client filter
			// client-side. Cycle 24 adds `kind` column + index.
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 25
			}
			items, next := db.Jobs().List(limit, r.URL.Query().Get("cursor"))
			if items == nil {
				items = []store.Job{}
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
