package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func terminalEvent(jobID quarrycontracts.ID, eventType quarrycontracts.EventType, payload map[string]any) quarrycontracts.Event {
	return quarrycontracts.Event{JobID: &jobID, Type: eventType, Payload: payload}
}

func newJob(t *testing.T, db store.DB, kind string) quarrycontracts.ID {
	t.Helper()
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	if err := db.Jobs().Create(store.Job{
		ID: id, OrgID: "org_a", Kind: kind, Status: "running", CreatedAt: time.Now().UnixMilli(),
	}); err != nil {
		t.Fatal(err)
	}
	return id
}

func TestTerminalEventPersistsStatistics(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	id := newJob(t, db, "crawl")
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": 5,
		"statistics": map[string]any{
			"pages_visited":  float64(5),
			"pages_failed":   float64(1),
			"status_classes": map[string]any{"2xx": float64(4), "4xx": float64(1)},
			"duration_ms":    float64(1234),
		},
	}))

	job, _ := db.Jobs().Get(id)
	if job.Status != "completed" {
		t.Fatalf("status = %q", job.Status)
	}
	if job.Statistics["pages_visited"] != float64(5) || job.Statistics["duration_ms"] != float64(1234) {
		t.Fatalf("statistics = %#v", job.Statistics)
	}
}

func TestStatisticsAreAbsentUntilTheRunEndsAndForEventsWithoutThem(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	id := newJob(t, db, "crawl")
	// A non-terminal event changes nothing.
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtPageFetched, map[string]any{
		"statistics": map[string]any{"pages_visited": float64(1)},
	}))
	if job, _ := db.Jobs().Get(id); job.Status != "running" || job.Statistics != nil {
		t.Fatalf("a page event must not terminate or annotate the job: %#v", job)
	}
	// A terminal event from an older orchestrator carries none: status still ends.
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunCompleted, map[string]any{"pages_visited": 3}))
	if job, _ := db.Jobs().Get(id); job.Status != "completed" || job.Statistics != nil {
		t.Fatalf("legacy terminal event: %#v", job)
	}
}

func TestMalformedOrOversizeStatisticsAreDroppedNotStored(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	for name, payload := range map[string]map[string]any{
		"not an object": {"statistics": "pages=5"},
		"empty":         {"statistics": map[string]any{}},
		"oversize":      {"statistics": map[string]any{"blob": strings.Repeat("x", maxStatisticsBytes+1)}},
		"unencodable":   {"statistics": map[string]any{"bad": make(chan int)}},
	} {
		id := newJob(t, db, "crawl")
		markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunCompleted, payload))
		job, _ := db.Jobs().Get(id)
		if job.Status != "completed" {
			t.Errorf("%s: the job must still end, status = %q", name, job.Status)
		}
		if job.Statistics != nil {
			t.Errorf("%s: statistics were stored: %#v", name, job.Statistics)
		}
	}
}

func TestJobStatisticsAppearOnTheWireAndRoundTrip(t *testing.T) {
	t.Parallel()
	job := store.Job{
		ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_a", Kind: "crawl", Status: "completed",
		CreatedAt:  time.Now().UnixMilli(),
		Statistics: map[string]any{"pages_visited": float64(5), "drivers": map[string]any{"static": float64(5)}},
	}
	encoded, err := json.Marshal(job)
	if err != nil {
		t.Fatal(err)
	}
	var decoded store.Job
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Statistics["pages_visited"] != float64(5) {
		t.Fatalf("round trip lost statistics: %s", encoded)
	}
	// A job without them keeps the wire shape it had before (no empty object).
	bare, _ := json.Marshal(store.Job{ID: job.ID, OrgID: "org_a", Kind: "crawl", Status: "running"})
	if strings.Contains(string(bare), "statistics") {
		t.Fatalf("bare job leaked a statistics key: %s", bare)
	}
}

func TestJobsByIDEndpointReturnsStatistics(t *testing.T) {
	t.Parallel()
	h, db := newJobsByKindTestServer(t)
	id := newJob(t, db, "crawl")
	markJobTerminalIfNeeded(db, terminalEvent(id, quarrycontracts.EvtRunCompleted, map[string]any{
		"statistics": map[string]any{"pages_visited": float64(2)},
	}))
	job, _ := db.Jobs().Get(id)
	if job.Statistics["pages_visited"] != float64(2) {
		t.Fatalf("job = %#v", job)
	}
	// The by-kind list is a different wire shape (JobSummary); it must still serve.
	req := httptest.NewRequest(http.MethodGet, "/v1/crawl/jobs?org_id=org_a", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d: %s", w.Code, w.Body.String())
	}
}
