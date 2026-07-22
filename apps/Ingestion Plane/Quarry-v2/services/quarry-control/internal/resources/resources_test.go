package resources

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newTestServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobs(r, db)
	MountEvents(r, db, "", nil)
	return r, db
}

func newTestServerAuth(t *testing.T, key string) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobs(r, db)
	MountEvents(r, db, key, nil)
	return r, db
}

func TestCreateJob_ReturnsEnvelope(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	body := `{"kind":"crawl","params":{"url":"https://a.com","max_pages":5}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data, body=%s", w.Body.String())
	}
	id, _ := data["id"].(string)
	if !strings.HasPrefix(id, "job_") {
		t.Fatalf("expected job_ prefix, got %q", id)
	}
	if data["kind"] != "crawl" {
		t.Fatalf("kind = %v", data["kind"])
	}
}

func TestCreateJob_Idempotency_ReturnsExistingOnDuplicateKey(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	body := `{"kind":"crawl","params":{"url":"https://idem.test"}}`
	post := func(key string) *httptest.ResponseRecorder {
		req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		if key != "" {
			req.Header.Set("Idempotency-Key", key)
		}
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		return rec
	}

	first := post("crawl-test-key-1")
	if first.Code != http.StatusCreated {
		t.Fatalf("first POST status = %d, want 201; body=%s", first.Code, first.Body.String())
	}
	var env1 struct {
		Data map[string]any `json:"data"`
	}
	_ = json.Unmarshal(first.Body.Bytes(), &env1)
	firstID, _ := env1.Data["id"].(string)
	if firstID == "" {
		t.Fatalf("missing first id; body=%s", first.Body.String())
	}

	// Same key → 200 OK with the SAME id.
	second := post("crawl-test-key-1")
	if second.Code != http.StatusOK {
		t.Fatalf("second POST status = %d, want 200; body=%s", second.Code, second.Body.String())
	}
	var env2 struct {
		Data map[string]any `json:"data"`
	}
	_ = json.Unmarshal(second.Body.Bytes(), &env2)
	secondID, _ := env2.Data["id"].(string)
	if secondID != firstID {
		t.Fatalf("second id = %q, want same as first %q", secondID, firstID)
	}

	// Different key → 201 with a NEW id.
	third := post("crawl-test-key-2")
	if third.Code != http.StatusCreated {
		t.Fatalf("third POST status = %d, want 201", third.Code)
	}
	var env3 struct {
		Data map[string]any `json:"data"`
	}
	_ = json.Unmarshal(third.Body.Bytes(), &env3)
	thirdID, _ := env3.Data["id"].(string)
	if thirdID == "" || thirdID == firstID {
		t.Fatalf("third id = %q (firstID=%q): expected new id", thirdID, firstID)
	}

	// No key at all → 201 each time (legacy behaviour preserved).
	noKeyA := post("")
	noKeyB := post("")
	if noKeyA.Code != http.StatusCreated || noKeyB.Code != http.StatusCreated {
		t.Fatalf("no-key statuses: %d %d", noKeyA.Code, noKeyB.Code)
	}
}

func TestCreateJob_Idempotency_RejectsOverlongKey(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)
	body := `{"kind":"crawl","params":{"url":"https://x"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", strings.Repeat("a", 200))
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 for overlong key; body=%s", rec.Code, rec.Body.String())
	}
}

func TestUpdateJob_TransitionsStatusAndRunID(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)

	// Seed an accepted job directly via store.
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, Kind: "crawl", Status: "accepted"})

	// PUT to transition.
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	patch := struct {
		Status string             `json:"status"`
		RunID  quarrycontracts.ID `json:"run_id"`
	}{Status: "running", RunID: runID}
	raw, _ := json.Marshal(patch)
	req := httptest.NewRequest(http.MethodPut, "/v1/jobs/"+string(id), bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
	got, _ := db.Jobs().Get(id)
	if got.Status != "running" {
		t.Fatalf("status = %s, want running", got.Status)
	}
	if got.RunID == nil || *got.RunID != runID {
		t.Fatalf("run_id = %v, want %v", got.RunID, runID)
	}

	// PUT on a missing id → 404.
	req404 := httptest.NewRequest(http.MethodPut, "/v1/jobs/job_DOESNOTEXIST", bytes.NewReader(raw))
	req404.Header.Set("Content-Type", "application/json")
	rec404 := httptest.NewRecorder()
	h.ServeHTTP(rec404, req404)
	if rec404.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for missing id", rec404.Code)
	}
}

func TestGetJob_NotFoundReturnsError(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/job_DOESNOTEXIST?org_id=org_test", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", w.Code)
	}
	var env map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "NOT_FOUND" {
		t.Fatalf("expected NOT_FOUND error, got %s", w.Body.String())
	}
}

// TestGetJob_MissingOrgID_Returns400 guards against an unscoped GET
// silently falling back to an unfiltered lookup.
func TestGetJob_MissingOrgID_Returns400(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, OrgID: "org_test", Kind: "crawl", Status: "accepted"})

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(id), nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 without org_id; body=%s", w.Code, w.Body.String())
	}
}

// TestGetJob_CrossTenantRejected is the IDOR guard: org_b must not be able
// to read org_a's job by id.
func TestGetJob_CrossTenantRejected(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, OrgID: "org_a", Kind: "crawl", Status: "accepted"})

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(id)+"?org_id=org_b", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for cross-tenant read; body=%s", w.Code, w.Body.String())
	}
}

// TestJobEvents_CrossTenantRejected guards GET /v1/jobs/{id}/events: an
// org that doesn't own the job must see 404 for its events, not a mix of
// missing-org-id and cross-tenant leakage.
func TestJobEvents_CrossTenantRejected(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, OrgID: "org_a", Kind: "crawl", Status: "accepted"})
	_ = db.Events().Append(quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		JobID:   &id,
		Type:    quarrycontracts.EvtPageFetched,
		Seq:     1,
	})

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(id)+"/events?org_id=org_b", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for cross-tenant read; body=%s", w.Code, w.Body.String())
	}
}

// TestJobEvents_MissingOrgID_Returns400 guards against an unscoped query
// silently returning the events regardless of org.
func TestJobEvents_MissingOrgID_Returns400(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, OrgID: "org_a", Kind: "crawl", Status: "accepted"})

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(id)+"/events", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 without org_id; body=%s", w.Code, w.Body.String())
	}
}

// TestJobEvents_OwnerCanRead confirms the org guard doesn't break the
// legitimate owner's read path.
func TestJobEvents_OwnerCanRead(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Jobs().Create(store.Job{ID: id, OrgID: "org_a", Kind: "crawl", Status: "accepted"})
	_ = db.Events().Append(quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		JobID:   &id,
		Type:    quarrycontracts.EvtPageFetched,
		Seq:     1,
	})

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(id)+"/events?org_id=org_a", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200; body=%s", w.Code, w.Body.String())
	}
	var env struct {
		Data []quarrycontracts.Event `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if len(env.Data) != 1 {
		t.Fatalf("want 1 event, got %d", len(env.Data))
	}
}

func TestAppendAndFetchEvents(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)

	// A run's events only resolve org_id/kind via the job that owns the
	// run_id (see event_wire.go's toJobHistoryEvent) — GetByRunID 404s
	// without one.
	jobID := quarrycontracts.NewID(quarrycontracts.KindJob)
	if err := db.Jobs().Create(store.Job{ID: jobID, OrgID: "org_a", Kind: "crawl", Status: "running", RunID: &runID}); err != nil {
		t.Fatal(err)
	}

	// seed via store directly
	evt := quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		RunID:   &runID,
		Type:    quarrycontracts.EvtPageFetched,
		Seq:     1,
	}
	_ = db.Events().Append(evt)

	// append via HTTP. The caller-supplied seq is intentionally wrong:
	// control owns durable per-run ordering and should rewrite it to 2.
	post := quarrycontracts.Event{Type: quarrycontracts.EvtPageFailed, Seq: 99}
	raw, _ := json.Marshal([]quarrycontracts.Event{post})
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/"+string(runID)+"/events", bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("append status = %d, body=%s", w.Code, w.Body.String())
	}

	// list — decoded as jobHistoryEventWire, the shape quarry-edge's
	// list_run_events actually requires (org_id/kind/stage/status), not
	// the legacy quarrycontracts.Event shape this endpoint used to emit.
	listReq := httptest.NewRequest(http.MethodGet, "/v1/runs/"+string(runID)+"/events", nil)
	listW := httptest.NewRecorder()
	h.ServeHTTP(listW, listReq)
	if listW.Code != 200 {
		t.Fatalf("list status = %d, body=%s", listW.Code, listW.Body.String())
	}
	var env struct {
		Data []jobHistoryEventWire `json:"data"`
	}
	if err := json.Unmarshal(listW.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if len(env.Data) != 2 {
		t.Fatalf("want 2 events, got %d", len(env.Data))
	}
	if env.Data[1].Seq != 2 {
		t.Fatalf("control should assign next seq=2, got %d", env.Data[1].Seq)
	}
	if env.Data[0].OrgID != "org_a" || env.Data[0].Kind != "crawl" {
		t.Fatalf("org_id/kind should come from the owning job, got org_id=%q kind=%q", env.Data[0].OrgID, env.Data[0].Kind)
	}
}

// TestRunEvents_UnknownRunID_Returns404 guards the GetByRunID miss path: a
// run_id no job ever dispatched (should never legitimately happen, but a
// stale/forged id must not fall through to a nonsensical empty-org event
// page) returns 404 rather than 200 with an empty or org_id-less list.
func TestRunEvents_UnknownRunID_Returns404(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	req := httptest.NewRequest(http.MethodGet, "/v1/runs/"+string(runID)+"/events", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404, body=%s", w.Code, w.Body.String())
	}
}

func TestAppendEvents_RejectsNonRunID(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/job_01HZZZ/events",
		strings.NewReader(`[{"type":"page_fetched"}]`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", w.Code)
	}
}

func TestAppendTerminalEventUpdatesJobStatus(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	createReq := httptest.NewRequest(
		http.MethodPost,
		"/v1/jobs?org_id=org_test",
		strings.NewReader(`{"kind":"crawl","params":{"url":"https://a.com"}}`),
	)
	createReq.Header.Set("Content-Type", "application/json")
	createW := httptest.NewRecorder()
	h.ServeHTTP(createW, createReq)
	if createW.Code != http.StatusCreated {
		t.Fatalf("create status = %d, body=%s", createW.Code, createW.Body.String())
	}
	var createEnv struct {
		Data store.Job `json:"data"`
	}
	if err := json.Unmarshal(createW.Body.Bytes(), &createEnv); err != nil {
		t.Fatal(err)
	}

	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	evt := quarrycontracts.Event{
		JobID: &createEnv.Data.ID,
		Type:  quarrycontracts.EvtRunCompleted,
	}
	raw, _ := json.Marshal([]quarrycontracts.Event{evt})
	appendReq := httptest.NewRequest(
		http.MethodPost,
		"/v1/runs/"+string(runID)+"/events",
		bytes.NewReader(raw),
	)
	appendReq.Header.Set("Content-Type", "application/json")
	appendW := httptest.NewRecorder()
	h.ServeHTTP(appendW, appendReq)
	if appendW.Code != http.StatusCreated {
		t.Fatalf("append status = %d, body=%s", appendW.Code, appendW.Body.String())
	}

	getReq := httptest.NewRequest(
		http.MethodGet,
		"/v1/jobs/"+string(createEnv.Data.ID)+"?org_id=org_test",
		nil,
	)
	getW := httptest.NewRecorder()
	h.ServeHTTP(getW, getReq)
	if getW.Code != http.StatusOK {
		t.Fatalf("get status = %d, body=%s", getW.Code, getW.Body.String())
	}
	var getEnv struct {
		Data store.Job `json:"data"`
	}
	if err := json.Unmarshal(getW.Body.Bytes(), &getEnv); err != nil {
		t.Fatal(err)
	}
	if getEnv.Data.Status != "completed" {
		t.Fatalf("job status = %q, want completed", getEnv.Data.Status)
	}
}

func TestJobHistory_MergesJobAndEvents(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)

	// Create job via HTTP
	createBody := `{"kind":"scrape","params":{"url":"https://a.com"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(createBody))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("create status = %d", w.Code)
	}
	var env struct {
		Data store.Job `json:"data"`
	}
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	jobID := env.Data.ID

	// Seed two events tagged with job_id
	for i := uint64(1); i <= 2; i++ {
		e := quarrycontracts.Event{
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
			JobID:   &jobID,
			Type:    quarrycontracts.EvtPageFetched,
			Seq:     i,
		}
		_ = db.Events().Append(e)
	}
	// Noise event on a different job
	otherID := quarrycontracts.NewID(quarrycontracts.KindJob)
	_ = db.Events().Append(quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		JobID:   &otherID,
		Type:    quarrycontracts.EvtPageFetched,
		Seq:     1,
	})

	histReq := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(jobID)+"/history?org_id=org_test", nil)
	histW := httptest.NewRecorder()
	h.ServeHTTP(histW, histReq)
	if histW.Code != http.StatusOK {
		t.Fatalf("history status = %d, body=%s", histW.Code, histW.Body.String())
	}

	var payload struct {
		Data struct {
			Job    store.Job               `json:"job"`
			Events []quarrycontracts.Event `json:"events"`
		} `json:"data"`
	}
	if err := json.Unmarshal(histW.Body.Bytes(), &payload); err != nil {
		t.Fatal(err)
	}
	if payload.Data.Job.ID != jobID {
		t.Fatalf("job id mismatch: %s", payload.Data.Job.ID)
	}
	if len(payload.Data.Events) != 2 {
		t.Fatalf("want 2 events for job, got %d", len(payload.Data.Events))
	}
}
