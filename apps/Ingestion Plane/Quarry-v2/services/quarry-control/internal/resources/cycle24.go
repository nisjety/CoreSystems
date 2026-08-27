// Cycle 24 — replaces the cycle-23 zero-shape stubs for the
// edge-forwarded list families with REAL org-scoped reads:
//
//   - GET /v1/team/credit-usage   → TeamUsage().CreditUsage (real SUM over events)
//   - GET /v1/team/token-usage    → TeamUsage().TokenUsage
//   - GET /v1/team/concurrency    → TeamUsage().Concurrency
//   - GET /v1/team/queue-status   → TeamUsage().QueueStatus
//   - GET /v1/team/activity       → TeamUsage().Activity as Page<TeamActivityEntry>
//   - GET /v1/snapshots           → SnapshotsV2() projected to
//     quarry_core::resources::Snapshot (replaces the legacy {id,run_id,bucket}
//     shape that failed forward_list::<Snapshot> deserialization at the edge)
//   - GET /v1/request-queues      → pg ListRequestQueueSummaries upgraded to
//     quarry_core::resources::RequestQueueSummary (org_id/kind/status/RFC3339/
//     required stats block)
//
// Wire-shape rules (pinned by pkg/quarrycontracts roundtrip tests):
//   - The four single-object team endpoints are decoded by the edge's
//     forward_one::<T>, which parses the response body as a BARE JSON object.
//     They must NOT go through httpx.WriteJSON's {data,...} envelope.
//   - List endpoints go through writePage's raw `{items,next_cursor?,
//     total_estimated?}` Page<T> shape; forward_list unwraps a `data` wrapper
//     when present, but the bare Page is what listSourcesHandler already
//     serves and what the Rust tests pin against.
//   - Every timestamp crossing to the edge renders RFC3339 (serde
//     DateTime<Utc> rejects unix-millis integers).
package resources

import (
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// parseListFilter translates the exact query string quarry-edge's
// forward_list sends (status / created_before / created_after / limit /
// cursor / sort) into the store ListFilter. Timestamps are RFC3339 per
// ListQuery::into_filter on the Rust side. Unknown period/status tokens are
// passed through untouched: each resource's taxonomy owns its own matching.
func parseListFilter(r *http.Request, defLimit int) store.ListFilter {
	q := r.URL.Query()
	f := store.ListFilter{
		Status:         strings.TrimSpace(q.Get("status")),
		Limit:          defLimit,
		SortDescending: true,
	}
	if v, err := strconv.Atoi(q.Get("limit")); err == nil && v > 0 {
		f.Limit = v
	}
	f.Cursor = q.Get("cursor")
	switch strings.ToLower(strings.TrimSpace(q.Get("sort"))) {
	case "asc", "oldest", "ascending":
		f.SortDescending = false
	}
	parseTS := func(name string) *time.Time {
		raw := strings.TrimSpace(q.Get(name))
		if raw == "" {
			return nil
		}
		for _, layout := range []string{time.RFC3339Nano, time.RFC3339} {
			if t, err := time.Parse(layout, raw); err == nil {
				return &t
			}
		}
		return nil
	}
	f.CreatedBefore = parseTS("created_before")
	f.CreatedAfter = parseTS("created_after")
	return f
}

// writeBareObject emits a single JSON object WITHOUT the REST envelope.
// Used only where the Rust caller decodes the body as a bare T
// (forward_one), never for list endpoints.
func writeBareObject(w http.ResponseWriter, r *http.Request, body any) {
	httpx.WriteRawJSON(w, r, http.StatusOK, body)
}

// ---- wire projections ------------------------------------------------------

// teamCreditWire mirrors quarry_core::resources::TeamCreditUsage. Period is
// echoed verbatim (the edge already normalized "unknown → 7d"; control keeps
// the same echo contract).
type teamCreditWire struct {
	OrgID              string  `json:"org_id"`
	Period             string  `json:"period"`
	CreditsUsed        float64 `json:"credits_used"`
	CreditsLimit       *float64 `json:"credits_limit,omitempty"`
	UtilizationPercent float64 `json:"utilization_percent"`
}

type teamTokenWire struct {
	OrgID        string `json:"org_id"`
	Period       string `json:"period"`
	InputTokens  uint64 `json:"input_tokens"`
	OutputTokens uint64 `json:"output_tokens"`
	TotalTokens  uint64 `json:"total_tokens"`
	CostMicroUSD *int64 `json:"cost_micro_usd,omitempty"`
}

type hostConcurrencyWire struct {
	Host          string   `json:"host"`
	Current       uint32   `json:"current"`
	Ceiling       uint32   `json:"ceiling"`
	EWMALatencyMs *float64 `json:"ewma_latency_ms,omitempty"`
}

type teamConcurrencyWire struct {
	OrgID   string                `json:"org_id"`
	Current uint32                `json:"current"`
	Ceiling uint32                `json:"ceiling"`
	ByHost  []hostConcurrencyWire `json:"by_host"`
}

type queueStatusEntryWire struct {
	QueueID  string `json:"queue_id"`
	Name     string `json:"name"`
	Queued   uint64 `json:"queued"`
	InFlight uint64 `json:"in_flight"`
}

type teamQueueStatusWire struct {
	OrgID         string                 `json:"org_id"`
	QueuedTotal   uint64                 `json:"queued_total"`
	InFlightTotal uint64                 `json:"in_flight_total"`
	ByQueue       []queueStatusEntryWire `json:"by_queue"`
}

type activityEntryWire struct {
	EventID   string    `json:"event_id"`
	OrgID     string    `json:"org_id"`
	EventType string    `json:"event_type"`
	RunID     *string   `json:"run_id,omitempty"`
	Ts        time.Time `json:"ts"`
	Summary   string    `json:"summary"`
}

// snapshotWire mirrors quarrycontracts.SnapshotWire field-for-field (kept
// local like jobWire rather than importing pkg types into handler structs).
type snapshotWire struct {
	SnapshotID      string    `json:"snapshot_id"`
	OrgID           string    `json:"org_id"`
	SourceID        *string   `json:"source_id,omitempty"`
	URL             string    `json:"url"`
	Fingerprint     string    `json:"fingerprint"`
	PrevFingerprint *string   `json:"prev_fingerprint,omitempty"`
	ChangeStatus    string    `json:"change_status"`
	CapturedAt      time.Time `json:"captured_at"`
	ArtifactID      *string   `json:"artifact_id,omitempty"`
}

func idPtr(id *quarrycontracts.ID) *string {
	if id == nil {
		return nil
	}
	s := string(*id)
	return &s
}

func snapshotToWire(s store.SnapshotV2) snapshotWire {
	return snapshotWire{
		SnapshotID:      string(s.ID),
		OrgID:           s.OrgID,
		SourceID:        idPtr(s.SourceID),
		URL:             s.URL,
		Fingerprint:     s.Fingerprint,
		PrevFingerprint: s.PrevFingerprint,
		ChangeStatus:    s.ChangeStatus,
		CapturedAt:      time.UnixMilli(s.CreatedAt).UTC(),
		ArtifactID:      idPtr(s.ArtifactID),
	}
}

// ---- /v1/team/* ------------------------------------------------------------

// MountTeam registers the five /v1/team/* routes backed by the real
// TeamUsageStore. The four aggregate endpoints return BARE objects (the
// edge's forward_one decodes without an envelope); activity returns the
// shared Page<T> envelope. Every route requires the edge-verified ?org_id.
func MountTeam(r chi.Router, db store.DB) {
	tu := db.TeamUsage()

	r.Get("/v1/team/credit-usage", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		row, err := tu.CreditUsage(org, pickQuery(r, "period", "7d"))
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		writeBareObject(w, r, teamCreditWire{
			OrgID:              org,
			Period:             pickQuery(r, "period", "7d"),
			CreditsUsed:        row.CreditsUsed,
			CreditsLimit:       row.CreditsLimit,
			UtilizationPercent: row.UtilizationPercent,
		})
	})

	r.Get("/v1/team/token-usage", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		row, err := tu.TokenUsage(org, pickQuery(r, "period", "7d"))
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		writeBareObject(w, r, teamTokenWire{
			OrgID:        org,
			Period:       pickQuery(r, "period", "7d"),
			InputTokens:  row.InputTokens,
			OutputTokens: row.OutputTokens,
			TotalTokens:  row.TotalTokens,
			CostMicroUSD: row.CostMicroUSD,
		})
	})

	r.Get("/v1/team/concurrency", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		row, err := tu.Concurrency(org)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		byHost := make([]hostConcurrencyWire, 0, len(row.ByHost))
		for _, h := range row.ByHost {
			byHost = append(byHost, hostConcurrencyWire{
				Host:          h.Host,
				Current:       h.Current,
				Ceiling:       h.Ceiling,
				EWMALatencyMs: h.EWMALatencyMs,
			})
		}
		writeBareObject(w, r, teamConcurrencyWire{
			OrgID:   org,
			Current: row.Current,
			Ceiling: row.Ceiling,
			ByHost:  byHost,
		})
	})

	r.Get("/v1/team/queue-status", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		row, err := tu.QueueStatus(org)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		byQueue := make([]queueStatusEntryWire, 0, len(row.ByQueue))
		for _, e := range row.ByQueue {
			byQueue = append(byQueue, queueStatusEntryWire{
				QueueID:  e.QueueID,
				Name:     e.Name,
				Queued:   e.Queued,
				InFlight: e.InFlight,
			})
		}
		writeBareObject(w, r, teamQueueStatusWire{
			OrgID:         org,
			QueuedTotal:   row.QueuedTotal,
			InFlightTotal: row.InFlightTotal,
			ByQueue:       byQueue,
		})
	})

	r.Get("/v1/team/activity", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		f := parseListFilter(r, 50)
		entries, next := tu.Activity(org, f)
		items := make([]activityEntryWire, 0, len(entries))
		for _, e := range entries {
			items = append(items, activityEntryWire{
				EventID:   string(e.EventID),
				OrgID:     org,
				EventType: e.EventType,
				RunID:     idPtr(e.RunID),
				Ts:        e.Ts.UTC(),
				Summary:   e.Summary,
			})
		}
		var nextPtr *string
		if next != "" {
			nextPtr = &next
		}
		writePage(w, r, items, nil, nextPtr)
	})
}

// ---- GET /v1/snapshots (enriched) ------------------------------------------

// MountSnapshotsV2 serves the enriched Snapshot list from the
// quarry_snapshots_v2 read model. It REPLACES the cycle-22 mountSimple
// legacy list at the same path (whose {id,run_id,bucket} items failed
// forward_list::<Snapshot> decode at the edge). The legacy ResourceStore
// stays mounted nowhere tenant-facing; restore.go keeps reading it directly.
func MountSnapshotsV2(r chi.Router, db store.DB) {
	r.Get("/v1/snapshots", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		items, next := db.SnapshotsV2().ListByOrg(org, parseListFilter(r, 50))
		wire := make([]snapshotWire, 0, len(items))
		for _, s := range items {
			wire = append(wire, snapshotToWire(s))
		}
		var nextPtr *string
		if next != "" {
			nextPtr = &next
		}
		writePage(w, r, wire, nil, nextPtr)
	})
}

// ---- GET /v1/request-queues (enriched) -------------------------------------

// MountRequestQueuesV2 upgrades the queue list to the full
// RequestQueueSummary wire shape via the optional RequestQueueSummariesReader
// capability. When the backend can't serve it (in-memory dev mode has no
// durable frontier tables) it falls back to the legacy reader, and finally to
// an honest empty page so the contract holds in every deployment.
func MountRequestQueuesV2(r chi.Router, db store.DB) {
	r.Get("/v1/request-queues", func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		type summariesReader interface {
			ListRequestQueueSummaries(orgID string, f store.ListFilter) ([]store.RequestQueueSummaryV2, string, error)
		}
		if sr, ok := db.(summariesReader); ok {
			items, next, err := sr.ListRequestQueueSummaries(org, parseListFilter(r, 25))
			if err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "request queue read failed", nil)
				return
			}
			wire := make([]quarrycontracts.RequestQueueSummaryWire, 0, len(items))
			for _, q := range items {
				wire = append(wire, quarrycontracts.RequestQueueSummaryWire{
					QueueID:   q.QueueID,
					OrgID:     org,
					Name:      q.Name,
					Kind:      q.Kind,
					Status:    q.Status,
					CreatedAt: time.UnixMilli(q.CreatedAt).UTC(),
					Stats: quarrycontracts.RequestQueueStatsWire{
						Queued:   q.Stats.Queued,
						InFlight: q.Stats.InFlight,
						Acked:    q.Stats.Acked,
						Failed:   q.Stats.Failed,
					},
				})
			}
			var nextPtr *string
			if next != "" {
				nextPtr = &next
			}
			writePage(w, r, wire, nil, nextPtr)
			return
		}
		// In-memory fallback: keep serving the legacy projection rather than
		// pretending the ephemeral process has durable queues.
		reader, ok := db.(store.RequestQueueReader)
		if !ok {
			emptyPage(w, r)
			return
		}
		limit, _ := strconv.Atoi(pickQuery(r, "limit", "25"))
		if limit <= 0 {
			limit = 25
		}
		items, next, err := reader.ListRequestQueues(org, limit, pickQuery(r, "cursor", ""))
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "request queue read failed", nil)
			return
		}
		wire := make([]quarrycontracts.RequestQueueSummaryWire, 0, len(items))
		for _, q := range items {
			wire = append(wire, quarrycontracts.RequestQueueSummaryWire{
				QueueID:   q.QueueID,
				OrgID:     org,
				Name:      q.Name,
				Kind:      "crawl",
				Status:    "active",
				CreatedAt: time.UnixMilli(q.CreatedAt).UTC(),
				Stats: quarrycontracts.RequestQueueStatsWire{
					Queued:   q.Queued,
					InFlight: q.InFlight,
				},
			})
		}
		var nextPtr *string
		if next != "" {
			nextPtr = &next
		}
		writePage(w, r, wire, nil, nextPtr)
	})
}
