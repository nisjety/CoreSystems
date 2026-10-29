package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newJobsByKindTestServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobsByKind(r, db)
	return r, db
}

// TestJobsByKind_WireShapeRoundTrips pins the exact JSON shape GET
// /v1/{kind}/jobs must emit for quarry_core::resources::JobSummary
// (crates/quarry-core/src/resources.rs) to deserialize it: job_id (NOT
// id), kind, org_id, status, created_at as RFC3339. It also closes the
// cycle23.go TODO this was blocked on — a search-kind job must never
// leak into the crawl list.
func TestJobsByKind_WireShapeRoundTrips(t *testing.T) {
	t.Parallel()
	h, db := newJobsByKindTestServer(t)

	crawlID := quarrycontracts.NewID(quarrycontracts.KindJob)
	searchID := quarrycontracts.NewID(quarrycontracts.KindJob)
	now := time.Now().UnixMilli()
	if err := db.Jobs().Create(store.Job{ID: crawlID, OrgID: "org_a", Kind: "crawl", Status: "accepted", CreatedAt: now}); err != nil {
		t.Fatal(err)
	}
	if err := db.Jobs().Create(store.Job{ID: searchID, OrgID: "org_a", Kind: "search", Status: "accepted", CreatedAt: now}); err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/crawl/jobs?org_id=org_a", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env struct {
		Data struct {
			Items []map[string]any `json:"items"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	items := env.Data.Items
	if len(items) != 1 {
		t.Fatalf("want 1 item (crawl only, search must not leak in), got %d: %v", len(items), items)
	}
	got := items[0]

	if _, hasBareID := got["id"]; hasBareID {
		t.Fatalf("wire shape must not carry a bare `id` field (JobSummary has no such field), got %v", got)
	}
	if got["job_id"] != string(crawlID) {
		t.Fatalf("job_id = %v, want %s", got["job_id"], crawlID)
	}
	if got["kind"] != "crawl" {
		t.Fatalf("kind = %v, want crawl", got["kind"])
	}
	if got["org_id"] != "org_a" {
		t.Fatalf("org_id = %v, want org_a (echoed from the edge-verified ?org_id= query param)", got["org_id"])
	}
	createdAt, _ := got["created_at"].(string)
	if _, err := time.Parse(time.RFC3339Nano, createdAt); err != nil {
		t.Fatalf("created_at = %q is not RFC3339 (JobSummary.created_at is DateTime<Utc>): %v", createdAt, err)
	}
}

// TestToJobWire_SkipsUnknownKind guards against the stale "schedule" value
// that used to be documented as a valid Job.Kind (store.go's old comment)
// but has no quarry_core::resources::JobResourceKind variant and was never
// actually produced by createJob.
func TestToJobWire_SkipsUnknownKind(t *testing.T) {
	t.Parallel()
	j := store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "schedule", Status: "accepted", CreatedAt: time.Now().UnixMilli()}
	if _, ok := toJobWire(j); ok {
		t.Fatal("toJobWire should skip a kind with no JobResourceKind variant")
	}
}

// TestToJobWire_AcceptsScrape guards the JobResourceKind::Scrape variant
// added alongside this shim — scrape is the most common Job.Kind value in
// this codebase (createJob has no kind whitelist) and previously had no
// valid representation on the Rust side at all.
func TestToJobWire_AcceptsScrape(t *testing.T) {
	t.Parallel()
	j := store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "scrape", Status: "accepted", CreatedAt: time.Now().UnixMilli()}
	wire, ok := toJobWire(j)
	if !ok {
		t.Fatal("toJobWire should accept scrape")
	}
	if wire.Kind != "scrape" {
		t.Fatalf("kind = %q, want scrape", wire.Kind)
	}
	if wire.OrgID != "org_a" {
		t.Fatalf("org_id = %q, want org_a", wire.OrgID)
	}
}

// TestToJobWire_ForwardsRunID guards the GET /v1/{kind}/jobs -> Ingestions
// evidence panel path: run_id must ride along so a caller resolving a run's
// event history (which only accepts run ids) never falls back to the job id
// and 400s against quarry-edge. A job still queued/accepted has no run_id
// yet, so the omitted case must stay nil rather than an empty string.
func TestToJobWire_ForwardsRunID(t *testing.T) {
	t.Parallel()
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	withRun := store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "crawl", Status: "completed", CreatedAt: time.Now().UnixMilli(), RunID: &runID}
	wire, ok := toJobWire(withRun)
	if !ok {
		t.Fatal("toJobWire should accept crawl")
	}
	if wire.RunID == nil || *wire.RunID != string(runID) {
		t.Fatalf("run_id = %v, want %s", wire.RunID, runID)
	}

	withoutRun := store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "crawl", Status: "accepted", CreatedAt: time.Now().UnixMilli()}
	wire, ok = toJobWire(withoutRun)
	if !ok {
		t.Fatal("toJobWire should accept crawl")
	}
	if wire.RunID != nil {
		t.Fatalf("run_id = %v, want nil for a not-yet-dispatched job", *wire.RunID)
	}
}
