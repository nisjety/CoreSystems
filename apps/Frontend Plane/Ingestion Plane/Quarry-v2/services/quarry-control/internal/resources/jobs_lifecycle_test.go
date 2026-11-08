package resources

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// lcRouter mounts the job routes this unit adds (and the by-kind list) on an
// in-memory store.
func lcRouter(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobs(r, db)
	MountJobsByKind(r, db)
	return r, db
}

func lcDo(h http.Handler, method, path string, body any) *httptest.ResponseRecorder {
	var rd io.Reader
	if body != nil {
		b, _ := json.Marshal(body)
		rd = bytes.NewReader(b)
	}
	req := httptest.NewRequest(method, path, rd)
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

// lcSeedJob stores a job of org with the given kind, status and params.
func lcSeedJob(t *testing.T, db store.DB, org, kind, status string, params map[string]any) quarrycontracts.ID {
	t.Helper()
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	if err := db.Jobs().Create(store.Job{
		ID: id, OrgID: org, Kind: kind, Status: status, Params: params,
		Origin:    store.JobOrigin{Type: "user", Actor: "usr_1"},
		CreatedAt: time.Now().UnixMilli(),
	}); err != nil {
		t.Fatal(err)
	}
	return id
}

// lcAppend appends one event of a job. A zero seq is numbered by the caller.
func lcAppend(t *testing.T, db store.DB, jobID quarrycontracts.ID, typ quarrycontracts.EventType, seq uint64, payload map[string]any) {
	t.Helper()
	jid := jobID
	if err := db.Events().Append(quarrycontracts.Event{
		EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
		JobID:     &jid,
		Type:      typ,
		Seq:       seq,
		Timestamp: time.Now().UTC(),
		Payload:   payload,
	}); err != nil {
		t.Fatal(err)
	}
}

func TestJobWire_RoundTripsOriginAndStats(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	deadline := time.Now().Add(time.Hour).UnixMilli()
	job := store.Job{
		ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "crawl", Status: "running",
		CreatedAt:  time.Now().UnixMilli(),
		Params:     map[string]any{"max_pages": float64(20)},
		Origin:     store.JobOrigin{Type: "agent", Actor: "service:model-gateway", Ref: "run_1", CreatedBy: "usr_1"},
		DeadlineAt: &deadline,
		WorkflowID: "wf_1",
	}
	if err := db.Jobs().Create(job); err != nil {
		t.Fatal(err)
	}
	lcAppend(t, db, job.ID, quarrycontracts.EvtPageFetched, 1, nil)

	encoded, err := json.Marshal(job)
	if err != nil {
		t.Fatal(err)
	}
	var decoded store.Job
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Origin != job.Origin {
		t.Fatalf("origin did not round-trip: %#v", decoded.Origin)
	}
	if decoded.DeadlineAt == nil || *decoded.DeadlineAt != deadline || decoded.WorkflowID != "wf_1" {
		t.Fatalf("deadline or workflow id did not round-trip: %#v", decoded)
	}

	wire, ok := toJobWire(job)
	if !ok || wire.Origin.Type != "agent" || wire.Origin.Ref != "run_1" || wire.Phase != "running" {
		t.Fatalf("wire = %#v ok=%v", wire, ok)
	}
	list := jobWires(db, []store.Job{job})
	if len(list) != 1 {
		t.Fatalf("jobWires = %#v", list)
	}
	if list[0].Stats["completed"] != int64(1) || list[0].Stats["total"] != 20 {
		t.Fatalf("stats = %#v", list[0].Stats)
	}
}

func TestSummary_CountsFromEvents(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "running", map[string]any{"max_pages": float64(20)})
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 2, nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 3, nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFailed, 4, map[string]any{"final": true})
	lcAppend(t, db, id, quarrycontracts.EvtPageBlocked, 5, nil)

	rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/summary?org_id=org_a", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("status %d: %s", rec.Code, rec.Body.String())
	}
	var env struct {
		Data jobSummaryView `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	got := env.Data
	if got.Counts.Completed != 3 || got.Counts.Failed != 1 || got.Counts.Blocked != 1 {
		t.Fatalf("counts = %#v", got.Counts)
	}
	if got.Total != 20 || got.TotalKind != "budget" || got.LastSeq != 5 || got.Phase != "running" {
		t.Fatalf("summary = %#v", got)
	}
}

func TestSummary_OrgScopedCrossTenantIs404(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)

	if rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/summary?org_id=org_b", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("foreign summary status %d", rec.Code)
	}
	if rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/errors?org_id=org_b", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("foreign errors status %d", rec.Code)
	}
	if rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/summary", nil); rec.Code != http.StatusBadRequest {
		t.Fatalf("missing org status %d", rec.Code)
	}
}

func TestErrors_IncludesFinalRetriedAndBlocked(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFailed, 1, map[string]any{"url": "https://e.test/a", "final": true, "category": "client_4xx", "code": "client_4xx", "status": float64(404)})
	lcAppend(t, db, id, quarrycontracts.EvtPageFailed, 2, map[string]any{"url": "https://e.test/b", "final": false, "category": "timeout"})
	lcAppend(t, db, id, quarrycontracts.EvtPageRetried, 3, map[string]any{"url": "https://e.test/c", "final": true, "category": "server_5xx"})
	lcAppend(t, db, id, quarrycontracts.EvtPageBlocked, 4, map[string]any{"url": "https://blocked.test/", "category": "host_blocked"})

	rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/errors?org_id=org_a", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("status %d: %s", rec.Code, rec.Body.String())
	}
	var env struct {
		Data jobErrorsView `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if len(env.Data.Errors) != 2 {
		t.Fatalf("errors = %#v (a non-final failure must not appear)", env.Data.Errors)
	}
	if env.Data.Errors[0].Code != "client_4xx" || env.Data.Errors[0].Status != 404 {
		t.Fatalf("first error = %#v", env.Data.Errors[0])
	}
	if len(env.Data.Blocked) != 1 || env.Data.Blocked[0].URL != "https://blocked.test/" {
		t.Fatalf("blocked = %#v", env.Data.Blocked)
	}
	if env.Data.NextSeq != nil {
		t.Fatalf("a short page must not carry next_seq: %d", *env.Data.NextSeq)
	}
}

func TestUpdateJob_IfStatusConflictIs409(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "accepted", nil)

	stale := "paused"
	if rec := lcDo(h, http.MethodPut, "/v1/jobs/"+string(id), map[string]any{"status": "running", "if_status": stale}); rec.Code != http.StatusConflict {
		t.Fatalf("stale compare-and-set status %d", rec.Code)
	}
	if job, _ := db.Jobs().Get(id); job.Status != "accepted" {
		t.Fatalf("a refused write changed the status to %q", job.Status)
	}
	if rec := lcDo(h, http.MethodPut, "/v1/jobs/"+string(id), map[string]any{"status": "running", "if_status": "accepted"}); rec.Code != http.StatusOK {
		t.Fatalf("matching compare-and-set status %d: %s", rec.Code, rec.Body.String())
	}
	if job, _ := db.Jobs().Get(id); job.Status != "running" {
		t.Fatalf("status = %q", job.Status)
	}
}

func TestUpdateJob_CannotLeaveTerminal(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)

	if rec := lcDo(h, http.MethodPut, "/v1/jobs/"+string(id), map[string]any{"status": "running"}); rec.Code != http.StatusConflict {
		t.Fatalf("leaving a terminal status answered %d", rec.Code)
	}
	if job, _ := db.Jobs().Get(id); job.Status != "completed" {
		t.Fatalf("status = %q", job.Status)
	}
	if rec := lcDo(h, http.MethodPut, "/v1/jobs/"+string(id), map[string]any{"status": "completed"}); rec.Code != http.StatusOK {
		t.Fatalf("re-writing the same terminal status answered %d", rec.Code)
	}
}

func TestJobStatusForEvent_PausedResumedTimedOutSkipped(t *testing.T) {
	t.Parallel()
	cases := map[quarrycontracts.EventType]string{
		quarrycontracts.EvtRunPaused:                  "paused",
		quarrycontracts.EvtRunResumed:                 "running",
		quarrycontracts.EventType("run_timed_out"):    "timed_out",
		quarrycontracts.EventType("schedule_skipped"): "skipped",
		quarrycontracts.EvtRunCancelled:               "cancelled",
	}
	for typ, want := range cases {
		got, ok := jobStatusForEvent(typ)
		if !ok || got != want {
			t.Errorf("%s -> %q ok=%v, want %q", typ, got, ok, want)
		}
	}
	if _, ok := jobStatusForEvent(quarrycontracts.EvtPageFetched); ok {
		t.Fatal("a page event must not change the job status")
	}
}

func TestMarkJobTerminal_DoesNotRegress(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	id := lcSeedJob(t, db, "org_a", "crawl", "running", nil)

	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunCompleted, nil))
	job, _ := db.Jobs().Get(id)
	if job.Status != "completed" || job.FinishedAt == nil || job.TerminalReason != "run_completed" {
		t.Fatalf("terminal write: %#v", job)
	}
	finished := *job.FinishedAt

	// A late resume or failure after the job ended must not move it.
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunResumed, nil))
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunFailed, nil))
	job, _ = db.Jobs().Get(id)
	if job.Status != "completed" || job.TerminalReason != "run_completed" || *job.FinishedAt != finished {
		t.Fatalf("a late event moved the ended job: %#v", job)
	}
}

func TestCreateJob_OriginIsValidated(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	if rec := lcDo(h, http.MethodPost, "/v1/jobs?org_id=org_a", map[string]any{
		"kind": "crawl", "origin": map[string]any{"type": "robot"},
	}); rec.Code != http.StatusBadRequest {
		t.Fatalf("unknown origin type answered %d", rec.Code)
	}
	rec := lcDo(h, http.MethodPost, "/v1/jobs?org_id=org_a", map[string]any{
		"kind": "crawl", "origin": map[string]any{"type": "webhook", "ref": "hook_1"},
	})
	if rec.Code != http.StatusCreated {
		t.Fatalf("valid origin answered %d: %s", rec.Code, rec.Body.String())
	}
	var env struct {
		Data struct {
			ID string `json:"id"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	job, ok := db.Jobs().GetByOrg("org_a", quarrycontracts.ID(env.Data.ID))
	if !ok || job.Origin.Type != "webhook" || job.Origin.Ref != "hook_1" {
		t.Fatalf("stored origin = %#v ok=%v", job.Origin, ok)
	}
	// No origin at all is an API client.
	rec = lcDo(h, http.MethodPost, "/v1/jobs?org_id=org_a", map[string]any{"kind": "crawl"})
	if rec.Code != http.StatusCreated {
		t.Fatalf("no origin answered %d", rec.Code)
	}
}

func TestJobsByKind_StatusFilterAndOngoing(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	running := lcSeedJob(t, db, "org_a", "crawl", "running", nil)
	paused := lcSeedJob(t, db, "org_a", "crawl", "paused", nil)
	lcSeedJob(t, db, "org_b", "crawl", "running", nil)

	rec := lcDo(h, http.MethodGet, "/v1/crawl/jobs?org_id=org_a&status=running,paused", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("status filter answered %d: %s", rec.Code, rec.Body.String())
	}
	var page struct {
		Data struct {
			Items []jobWire `json:"items"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &page); err != nil {
		t.Fatal(err)
	}
	if len(page.Data.Items) != 2 {
		t.Fatalf("filtered items = %#v", page.Data.Items)
	}
	if rec := lcDo(h, http.MethodGet, "/v1/crawl/jobs?org_id=org_a&status=bogus", nil); rec.Code != http.StatusBadRequest {
		t.Fatalf("unknown status filter answered %d", rec.Code)
	}

	rec = lcDo(h, http.MethodGet, "/v1/jobs/ongoing?org_id=org_a", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("ongoing answered %d: %s", rec.Code, rec.Body.String())
	}
	var ongoing struct {
		Data []jobWire `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &ongoing); err != nil {
		t.Fatal(err)
	}
	seen := map[string]bool{}
	for _, j := range ongoing.Data {
		seen[j.JobID] = true
	}
	if len(ongoing.Data) != 2 || !seen[string(running)] || !seen[string(paused)] {
		t.Fatalf("ongoing = %#v (org_b and completed jobs must not appear)", ongoing.Data)
	}
}
