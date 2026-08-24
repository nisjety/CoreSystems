package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newCycle24Server(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSnapshotsV2(r, db)
	MountRequestQueuesV2(r, db)
	MountTeam(r, db)
	return r, db
}

// getJSON issues GET and decodes the response body as raw JSON — for the
// bare-object team aggregate endpoints the edge's forward_one parses.
func getJSON(t *testing.T, h http.Handler, path string, out any) int {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if err := json.Unmarshal(w.Body.Bytes(), out); err != nil {
		t.Fatalf("decode %s: %v; body=%s", path, err, w.Body.String())
	}
	return w.Code
}

// getEnvelope issues GET and decodes the {request_id,data} envelope that
// writePage list handlers emit.
func getEnvelope(t *testing.T, h http.Handler, path string, data any) int {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	var env struct {
		Data json.RawMessage `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode %s envelope: %v; body=%s", path, err, w.Body.String())
	}
	if len(env.Data) == 0 {
		t.Fatalf("%s: no data envelope; body=%s", path, w.Body.String())
	}
	if err := json.Unmarshal(env.Data, data); err != nil {
		t.Fatalf("decode %s data: %v; body=%s", path, err, w.Body.String())
	}
	return w.Code
}

func TestTeamEndpoints_RequireOrgID(t *testing.T) {
	t.Parallel()
	h, _ := newCycle24Server(t)

	for _, path := range []string{
		"/v1/team/credit-usage",
		"/v1/team/token-usage",
		"/v1/team/concurrency",
		"/v1/team/queue-status",
		"/v1/team/activity",
		"/v1/snapshots",
	} {
		req := httptest.NewRequest(http.MethodGet, path, nil)
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusBadRequest {
			t.Fatalf("%s without org_id: status=%d want=400", path, w.Code)
		}
	}
}

// TestTeamAggregates_BareObjectWireShape pins the exact JSON keys the edge's
// forward_one::<T> deserializes — no {data,...} envelope, RFC3339-free scalar
// fields matching quarry_core::resources::{TeamCreditUsage,TeamTokenUsage,
// TeamConcurrency,TeamQueueStatus}.
func TestTeamAggregates_BareObjectWireShape(t *testing.T) {
	t.Parallel()
	h, _ := newCycle24Server(t)

	var credit struct {
		OrgID              string   `json:"org_id"`
		Period             string   `json:"period"`
		CreditsUsed        float64  `json:"credits_used"`
		CreditsLimit       *float64 `json:"credits_limit"`
		UtilizationPercent float64  `json:"utilization_percent"`
	}
	if code := getJSON(t, h, "/v1/team/credit-usage?org_id=o1&period=7d", &credit); code != http.StatusOK {
		t.Fatalf("credit-usage status=%d", code)
	}
	if credit.OrgID != "o1" || credit.Period != "7d" || credit.CreditsUsed != 0 {
		t.Fatalf("credit-usage body=%+v", credit)
	}

	var token struct {
		OrgID        string `json:"org_id"`
		Period       string `json:"period"`
		InputTokens  uint64 `json:"input_tokens"`
		OutputTokens uint64 `json:"output_tokens"`
		TotalTokens  uint64 `json:"total_tokens"`
	}
	if code := getJSON(t, h, "/v1/team/token-usage?org_id=o1", &token); code != http.StatusOK {
		t.Fatalf("token-usage status=%d", code)
	}
	if token.OrgID != "o1" || token.TotalTokens != 0 {
		t.Fatalf("token-usage body=%+v", token)
	}

	// by_host must be present as an EMPTY ARRAY, never null — serde's
	// Vec<T> with #[serde(default)] accepts [], not null.
	var conc struct {
		OrgID   string `json:"org_id"`
		Current uint32 `json:"current"`
		Ceiling uint32 `json:"ceiling"`
		ByHost  []struct {
			Host    string  `json:"host"`
			Current uint32  `json:"current"`
			Ceiling uint32  `json:"ceiling"`
			EWMA    *float64 `json:"ewma_latency_ms"`
		} `json:"by_host"`
	}
	if code := getJSON(t, h, "/v1/team/concurrency?org_id=o1", &conc); code != http.StatusOK {
		t.Fatalf("concurrency status=%d", code)
	}
	if conc.ByHost == nil {
		t.Fatal("concurrency by_host is null; want []")
	}

	var qs struct {
		OrgID         string `json:"org_id"`
		QueuedTotal   uint64 `json:"queued_total"`
		InFlightTotal uint64 `json:"in_flight_total"`
		ByQueue       []struct {
			QueueID  string `json:"queue_id"`
			Name     string `json:"name"`
			Queued   uint64 `json:"queued"`
			InFlight uint64 `json:"in_flight"`
		} `json:"by_queue"`
	}
	if code := getJSON(t, h, "/v1/team/queue-status?org_id=o1", &qs); code != http.StatusOK {
		t.Fatalf("queue-status status=%d", code)
	}
	if qs.ByQueue == nil {
		t.Fatal("queue-status by_queue is null; want []")
	}
}

// TestTeamCreditUsage_DerivesFromEvents seeds the in-memory event log via a
// real job + page_fetched events and asserts the aggregate is computed from
// them (not hardcoded zeros).
func TestTeamCreditUsage_DerivesFromEvents(t *testing.T) {
	t.Parallel()
	h, db := newCycle24Server(t)

	job := store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     "o1",
		Kind:      "crawl",
		Status:    "completed",
		CreatedAt: time.Now().Add(-time.Hour).UnixMilli(),
	}
	if err := db.Jobs().Create(job); err != nil {
		t.Fatalf("seed job: %v", err)
	}
	now := time.Now().UTC()
	for i := 0; i < 3; i++ {
		jid := job.ID
		if err := db.Events().Append(quarrycontracts.Event{
			EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
			Type:      quarrycontracts.EvtPageFetched,
			JobID:     &jid,
			Timestamp: now,
			Payload:   map[string]any{"pages": 2.0},
		}); err != nil {
			t.Fatalf("seed event: %v", err)
		}
	}

	var credit struct {
		CreditsUsed float64 `json:"credits_used"`
		Period      string  `json:"period"`
	}
	getJSON(t, h, "/v1/team/credit-usage?org_id=o1&period=7d", &credit)
	if credit.CreditsUsed != 6.0 { // 3 events × 2 pages × 1.0 credits/page
		t.Fatalf("credits_used=%v want=6.0", credit.CreditsUsed)
	}

	// A different org must see zero — org scoping on aggregates.
	var other struct {
		CreditsUsed float64 `json:"credits_used"`
	}
	getJSON(t, h, "/v1/team/credit-usage?org_id=o2", &other)
	if other.CreditsUsed != 0 {
		t.Fatalf("cross-tenant leak: credits_used=%v want=0", other.CreditsUsed)
	}
}

// TestTeamActivity_PaginatesAndShapes asserts the Page<TeamActivityEntry>
// envelope, the required per-entry fields, limit handling, and cursor
// continuation through the shared keyset token.
func TestTeamActivity_PaginatesAndShapes(t *testing.T) {
	t.Parallel()
	h, db := newCycle24Server(t)

	job := store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     "o1",
		Kind:      "crawl",
		Status:    "running",
		CreatedAt: time.Now().Add(-time.Hour).UnixMilli(),
	}
	if err := db.Jobs().Create(job); err != nil {
		t.Fatalf("seed job: %v", err)
	}
	base := time.Now().Add(-time.Minute).UTC()
	for i := 0; i < 5; i++ {
		jid := job.ID
		url := "https://a.example/page-" + strings.Repeat("x", 0) + string(rune('a'+i))
		if err := db.Events().Append(quarrycontracts.Event{
			EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
			Type:      quarrycontracts.EvtPageFetched,
			JobID:     &jid,
			Timestamp: base.Add(time.Duration(i) * time.Second),
			Payload:   map[string]any{"url": url},
		}); err != nil {
			t.Fatalf("seed event %d: %v", i, err)
		}
	}

	type pageShape struct {
		Items []struct {
			EventID   string    `json:"event_id"`
			OrgID     string    `json:"org_id"`
			EventType string    `json:"event_type"`
			RunID     *string   `json:"run_id"`
			Ts        time.Time `json:"ts"`
			Summary   string    `json:"summary"`
		} `json:"items"`
		NextCursor     *string `json:"next_cursor"`
		TotalEstimated *uint64 `json:"total_estimated"`
	}

	var p1 pageShape
	if code := getEnvelope(t, h, "/v1/team/activity?org_id=o1&limit=2", &p1); code != http.StatusOK {
		t.Fatalf("activity status=%d", code)
	}
	if len(p1.Items) != 2 || p1.NextCursor == nil || *p1.NextCursor == "" {
		t.Fatalf("page1 items=%d next=%v want 2+cursor", len(p1.Items), p1.NextCursor)
	}
	first := p1.Items[0]
	if first.OrgID != "o1" || first.EventType != "page_fetched" || first.Summary == "" {
		t.Fatalf("entry shape wrong: %+v", first)
	}
	if _, err := time.Parse(time.RFC3339, first.Ts.Format(time.RFC3339)); err != nil {
		t.Fatalf("ts not RFC3339-parseable: %v", err)
	}

	var p2 pageShape
	if code := getEnvelope(t, h, "/v1/team/activity?org_id=o1&limit=2&cursor="+*p1.NextCursor, &p2); code != http.StatusOK {
		t.Fatalf("activity page2 status=%d", code)
	}
	if len(p2.Items) != 2 {
		t.Fatalf("page2 items=%d want=2", len(p2.Items))
	}
	if p2.Items[0].EventID == p1.Items[0].EventID {
		t.Fatal("page2 repeats page1 item — cursor did not advance")
	}

	var p3 pageShape
	getEnvelope(t, h, "/v1/team/activity?org_id=o1&limit=2&cursor="+*p2.NextCursor, &p3)
	if len(p3.Items)+4 != 5 { // 2+2+1
		t.Fatalf("page3 items=%d want=1 total=5", len(p3.Items))
	}
	if p3.NextCursor != nil {
		t.Fatalf("final page should have no next_cursor, got %q", *p3.NextCursor)
	}
}

// TestSnapshotsV2_WireShapeAndScoping seeds the enriched snapshot store and
// pins the quarry_core::resources::Snapshot field names the edge requires.
func TestSnapshotsV2_WireShapeAndScoping(t *testing.T) {
	t.Parallel()
	h, db := newCycle24Server(t)

	src := quarrycontracts.ID("src_01TESTSOURCE")
	art := quarrycontracts.ID("art_01TESTARTIFACT")
	prev := "fp-old"
	mustCreate := func(org, snap string) {
		t.Helper()
		sid := quarrycontracts.ID(snap)
		err := db.SnapshotsV2().Create(store.SnapshotV2{
			ID:              sid,
			OrgID:           org,
			SourceID:        &src,
			URL:             "https://x.example/",
			Fingerprint:     "fp-new",
			PrevFingerprint: &prev,
			ChangeStatus:    "modified",
			CreatedAt:       time.Now().UnixMilli(),
			ArtifactID:      &art,
		})
		if err != nil {
			t.Fatalf("seed snapshot %s: %v", snap, err)
		}
	}
	mustCreate("o1", "snap_01AAAA")
	mustCreate("o1", "snap_01BBBB")
	mustCreate("o2", "snap_01CCCC")

	var page struct {
		Items []quarrycontracts.SnapshotWire `json:"items"`
	}
	code := getEnvelope(t, h, "/v1/snapshots?org_id=o1", &page)
	if code != http.StatusOK || len(page.Items) != 2 {
		t.Fatalf("snapshots status=%d items=%d want 200/2", code, len(page.Items))
	}
	// Both rows were written in the same millisecond, so either order is a
	// valid newest-first result; assert the SET and every wire field.
	saw := map[string]bool{}
	for _, it := range page.Items {
		saw[it.SnapshotID] = true
	}
	if !saw["snap_01AAAA"] || !saw["snap_01BBBB"] {
		t.Fatalf("o1 snapshots missing expected ids: %v", page.Items)
	}
	got := page.Items[0]
	if (got.SnapshotID != "snap_01AAAA" && got.SnapshotID != "snap_01BBBB") ||
		got.OrgID != "o1" ||
		got.SourceID == nil || *got.SourceID != string(src) ||
		got.Fingerprint != "fp-new" || got.PrevFingerprint == nil ||
		got.ChangeStatus != "modified" || got.CapturedAt.IsZero() ||
		got.ArtifactID == nil {
		t.Fatalf("snapshot wire shape wrong: %+v", got)
	}

	// Org scoping: o2 sees exactly its own row.
	var scoped struct {
		Items []quarrycontracts.SnapshotWire `json:"items"`
	}
	getEnvelope(t, h, "/v1/snapshots?org_id=o2", &scoped)
	if len(scoped.Items) != 1 || scoped.Items[0].SnapshotID != "snap_01CCCC" {
		t.Fatalf("o2 snapshots=%+v", scoped.Items)
	}

	// Status filter matches change_status.
	var filtered struct {
		Items []quarrycontracts.SnapshotWire `json:"items"`
	}
	getEnvelope(t, h, "/v1/snapshots?org_id=o1&status=new", &filtered)
	if len(filtered.Items) != 0 {
		t.Fatalf("status=new should match nothing, got %d", len(filtered.Items))
	}
}

// TestRequestQueues_InMemoryFallback pins that the in-memory backend still
// serves an honest empty page in the FULL wire shape (items array present,
// stats object would be required per-item if any rows existed).
func TestRequestQueues_InMemoryFallback(t *testing.T) {
	t.Parallel()
	h, _ := newCycle24Server(t)

	var page struct {
		Items []quarrycontracts.RequestQueueSummaryWire `json:"items"`
	}
	code := getEnvelope(t, h, "/v1/request-queues?org_id=o1", &page)
	if code != http.StatusOK || page.Items == nil || len(page.Items) != 0 {
		t.Fatalf("request-queues status=%d items=%v want 200/[]", code, page.Items)
	}
}

// TestParseListFilter covers the sort/limit/time-bound translation of the
// edge's ListFilter query string.
func TestParseListFilter(t *testing.T) {
	t.Parallel()

	r := httptest.NewRequest(http.MethodGet, "/x?limit=7&sort=asc&"+
		"created_before=2026-08-01T00%3A00%3A00Z&created_after=2026-07-01T00%3A00%3A00Z", nil)
	f := parseListFilter(r, 50)
	if f.Limit != 7 || f.SortDescending {
		t.Fatalf("filter=%+v", f)
	}
	if f.CreatedBefore == nil || f.CreatedAfter == nil {
		t.Fatalf("time bounds not parsed: %+v", f)
	}
	if !f.CreatedBefore.After(*f.CreatedAfter) {
		t.Fatalf("bound order wrong: %+v", f)
	}

	r2 := httptest.NewRequest(http.MethodGet, "/x?sort=desc", nil)
	f2 := parseListFilter(r2, 25)
	if !f2.SortDescending || f2.Limit != 25 {
		t.Fatalf("defaults wrong: %+v", f2)
	}
}
