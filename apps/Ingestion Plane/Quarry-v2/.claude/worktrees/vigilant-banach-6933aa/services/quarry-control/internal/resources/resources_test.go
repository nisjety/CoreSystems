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
	MountEvents(r, db, "")
	return r, db
}

func newTestServerAuth(t *testing.T, key string) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobs(r, db)
	MountEvents(r, db, key)
	return r, db
}

func TestCreateJob_ReturnsEnvelope(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	body := `{"kind":"crawl","params":{"url":"https://a.com","max_pages":5}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs", strings.NewReader(body))
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

func TestGetJob_NotFoundReturnsError(t *testing.T) {
	t.Parallel()
	h, _ := newTestServer(t)

	req := httptest.NewRequest(http.MethodGet, "/v1/jobs/job_DOESNOTEXIST", nil)
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

func TestAppendAndFetchEvents(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)

	// seed via store directly
	evt := quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		RunID:   &runID,
		Type:    quarrycontracts.EvtPageFetched,
		Seq:     1,
	}
	_ = db.Events().Append(evt)

	// append via HTTP
	post := quarrycontracts.Event{Type: quarrycontracts.EvtPageFailed, Seq: 2}
	raw, _ := json.Marshal([]quarrycontracts.Event{post})
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/"+string(runID)+"/events", bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("append status = %d, body=%s", w.Code, w.Body.String())
	}

	// list
	listReq := httptest.NewRequest(http.MethodGet, "/v1/runs/"+string(runID)+"/events", nil)
	listW := httptest.NewRecorder()
	h.ServeHTTP(listW, listReq)
	if listW.Code != 200 {
		t.Fatalf("list status = %d", listW.Code)
	}
	var env struct {
		Data []quarrycontracts.Event `json:"data"`
	}
	if err := json.Unmarshal(listW.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if len(env.Data) != 2 {
		t.Fatalf("want 2 events, got %d", len(env.Data))
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

func TestJobHistory_MergesJobAndEvents(t *testing.T) {
	t.Parallel()
	h, db := newTestServer(t)

	// Create job via HTTP
	createBody := `{"kind":"scrape","params":{"url":"https://a.com"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs", strings.NewReader(createBody))
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

	histReq := httptest.NewRequest(http.MethodGet, "/v1/jobs/"+string(jobID)+"/history", nil)
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
