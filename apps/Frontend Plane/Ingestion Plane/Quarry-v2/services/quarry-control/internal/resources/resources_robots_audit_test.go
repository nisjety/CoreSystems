package resources

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/audit"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// fakeRobotsAuditor records every audit write and, at the moment of the write,
// whether the job it names already exists. That second record is what proves
// audit-before-ack: the job must not exist yet when its audit row is written.
type fakeRobotsAuditor struct {
	mu          sync.Mutex
	db          store.DB
	err         error
	events      []audit.Event
	jobExisted  []bool
	hadDeadline []bool
}

func (f *fakeRobotsAuditor) RecordAudit(ctx context.Context, ev audit.Event) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	_, existed := f.db.Jobs().Get(quarrycontracts.ID(ev.ResourceID))
	_, deadline := ctx.Deadline()
	f.events = append(f.events, ev)
	f.jobExisted = append(f.jobExisted, existed)
	f.hadDeadline = append(f.hadDeadline, deadline)
	return f.err
}

func (f *fakeRobotsAuditor) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.events)
}

func robotsAuditServer(t *testing.T, withSink bool, sinkErr error) (http.Handler, store.DB, *fakeRobotsAuditor) {
	t.Helper()
	db := store.NewMemory()
	sink := &fakeRobotsAuditor{db: db, err: sinkErr}
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	if withSink {
		MountJobs(r, db, WithRobotsAudit(sink))
	} else {
		MountJobs(r, db)
	}
	return r, db, sink
}

func postJobWithKey(h http.Handler, org, idemKey string, body map[string]any) *httptest.ResponseRecorder {
	encoded, _ := json.Marshal(body)
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs/?org_id="+org, bytes.NewReader(encoded))
	if idemKey != "" {
		req.Header.Set("Idempotency-Key", idemKey)
	}
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w
}

func ignoreObjectJob() map[string]any {
	return map[string]any{
		"kind": "crawl",
		"params": map[string]any{
			"kind":    "crawl",
			"url":     "https://Fixture-A.example/private/secret?q=1",
			"user_id": "user_a",
			"robots": map[string]any{
				"mode":      "ignore",
				"authority": "web.robots_override",
				"rev":       7,
			},
		},
	}
}

func jobsOf(t *testing.T, db store.DB, org string) []store.Job {
	t.Helper()
	jobs, _ := db.Jobs().ListByOrg(org, 100, "")
	return jobs
}

func createdJobID(t *testing.T, w *httptest.ResponseRecorder) string {
	t.Helper()
	var env struct {
		Data struct {
			ID string `json:"id"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil || env.Data.ID == "" {
		t.Fatalf("decode created job: %v body=%s", err, w.Body.String())
	}
	return env.Data.ID
}

func errorCodeOf(t *testing.T, w *httptest.ResponseRecorder) (string, map[string]any) {
	t.Helper()
	var env struct {
		Error struct {
			Code    string         `json:"code"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode error: %v body=%s", err, w.Body.String())
	}
	return env.Error.Code, env.Error.Details
}

func TestCreateJob_RobotsIgnore_WritesAuditBeforeAck(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, nil)

	w := postJobWithKey(h, "org_a", "", ignoreObjectJob())
	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d: %s", w.Code, w.Body.String())
	}
	jobID := createdJobID(t, w)

	if sink.count() != 1 {
		t.Fatalf("audit writes = %d, want exactly 1", sink.count())
	}
	ev := sink.events[0]
	if sink.jobExisted[0] {
		t.Fatal("the job existed before its audit row was written; the audit must come first")
	}
	if !sink.hadDeadline[0] {
		t.Fatal("the audit write must be bounded by a deadline")
	}
	if ev.Event != "crawl.robots_ignored" || ev.ResourceID != jobID || ev.OrgID != "org_a" || ev.UserID != "user_a" {
		t.Fatalf("audit event = %+v", ev)
	}
	if ev.EventID != "quarry-control:crawl.robots_ignored:"+jobID {
		t.Fatalf("event_id = %q, want one derived from the job id", ev.EventID)
	}
	if ev.OccurredAt.IsZero() {
		t.Fatal("occurred_at missing")
	}
	if ev.Details["seed_host"] != "fixture-a.example" {
		t.Fatalf("seed_host = %v, want the lower-cased host only", ev.Details["seed_host"])
	}
	if ev.Details["authority"] != "web.robots_override" {
		t.Fatalf("authority = %v", ev.Details["authority"])
	}
	if ev.Details["rev"] != 7.0 {
		t.Fatalf("rev = %v (%T), want 7", ev.Details["rev"], ev.Details["rev"])
	}
	for _, v := range ev.Details {
		if s, ok := v.(string); ok && (strings.Contains(s, "/private") || strings.Contains(s, "q=1")) {
			t.Fatalf("audit details carry the seed path or query: %v", ev.Details)
		}
	}
	if _, ok := db.Jobs().GetByOrg("org_a", quarrycontracts.ID(jobID)); !ok {
		t.Fatal("the acknowledged job was not stored")
	}
}

func TestCreateJob_RobotsIgnore_StringModeIsAuditedToo(t *testing.T) {
	t.Parallel()
	// The orchestrator honours params.robots == "ignore" (a string). That shape
	// must not slip past the audit, whatever its casing.
	h, _, sink := robotsAuditServer(t, true, nil)
	for i, mode := range []string{"ignore", " IGNORE "} {
		w := postJobWithKey(h, "org_a", "", map[string]any{
			"kind":   "crawl",
			"params": map[string]any{"url": "https://example.com", "user_id": "user_a", "robots": mode},
		})
		if w.Code != http.StatusCreated {
			t.Fatalf("mode %q: status = %d: %s", mode, w.Code, w.Body.String())
		}
		if sink.count() != i+1 {
			t.Fatalf("mode %q: audit writes = %d, want %d", mode, sink.count(), i+1)
		}
	}
}

func TestCreateJob_RobotsIgnore_AuditFailureCreatesNoJob(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, errors.New("audit-core returned HTTP 500"))

	w := postJobWithKey(h, "org_a", "", ignoreObjectJob())
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503: %s", w.Code, w.Body.String())
	}
	code, details := errorCodeOf(t, w)
	if code != string(quarrycontracts.CodeRuntimeNotReady) || details["reason"] != "robots_override_audit_unavailable" {
		t.Fatalf("error = %s %v", code, details)
	}
	if strings.Contains(w.Body.String(), "HTTP 500") {
		t.Fatal("the upstream audit error leaked into the response")
	}
	if sink.count() != 1 {
		t.Fatalf("audit attempts = %d, want 1", sink.count())
	}
	if jobs := jobsOf(t, db, "org_a"); len(jobs) != 0 {
		t.Fatalf("a job was stored after the audit failed: %+v", jobs)
	}
}

func TestCreateJob_RobotsIgnore_RefusedWhenNoAuditSinkIsConfigured(t *testing.T) {
	t.Parallel()
	// AUDIT_CORE_URL / AUDIT_CORE_SERVICE_TOKEN unset: main mounts no sink.
	h, db, _ := robotsAuditServer(t, false, nil)

	w := postJobWithKey(h, "org_a", "", ignoreObjectJob())
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503: %s", w.Code, w.Body.String())
	}
	if jobs := jobsOf(t, db, "org_a"); len(jobs) != 0 {
		t.Fatalf("an override job was stored without an audit sink: %+v", jobs)
	}

	// The same server still takes an ordinary job.
	if w := postJobWithKey(h, "org_a", "", map[string]any{"kind": "crawl", "params": map[string]any{"url": "https://example.com"}}); w.Code != http.StatusCreated {
		t.Fatalf("ordinary job without a sink: %d %s", w.Code, w.Body.String())
	}
}

func TestCreateJob_RobotsIgnore_RequiresUserID(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, nil)
	body := ignoreObjectJob()
	delete(body["params"].(map[string]any), "user_id")

	w := postJobWithKey(h, "org_a", "", body)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400: %s", w.Code, w.Body.String())
	}
	if sink.count() != 0 || len(jobsOf(t, db, "org_a")) != 0 {
		t.Fatal("an override without a person was audited or stored")
	}
}

func TestCreateJob_Respect_WritesNoAudit(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, nil)
	for _, params := range []map[string]any{
		{"url": "https://example.com"},
		{"url": "https://example.com", "robots": "respect"},
		{"url": "https://example.com", "robots": nil},
		{"url": "https://example.com", "robots": map[string]any{"mode": "respect", "authority": "default"}},
		{"url": "https://example.com", "robots": map[string]any{"mode": "strict"}},
		{"url": "https://example.com", "robots": 1},
	} {
		if w := postJobWithKey(h, "org_a", "", map[string]any{"kind": "crawl", "params": params}); w.Code != http.StatusCreated {
			t.Fatalf("params %v: status = %d: %s", params, w.Code, w.Body.String())
		}
	}
	if sink.count() != 0 {
		t.Fatalf("audit writes = %d for jobs that respect robots.txt, want 0", sink.count())
	}
	if n := len(jobsOf(t, db, "org_a")); n != 6 {
		t.Fatalf("stored jobs = %d, want 6", n)
	}
}

func TestCreateJob_RobotsIgnore_IdempotentRetryWritesOneAudit(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, nil)

	first := postJobWithKey(h, "org_a", "retry-1", ignoreObjectJob())
	if first.Code != http.StatusCreated {
		t.Fatalf("first: %d %s", first.Code, first.Body.String())
	}
	second := postJobWithKey(h, "org_a", "retry-1", ignoreObjectJob())
	if second.Code != http.StatusOK {
		t.Fatalf("retry: %d %s", second.Code, second.Body.String())
	}
	if createdJobID(t, first) != createdJobID(t, second) {
		t.Fatal("the retry returned a different job")
	}
	if sink.count() != 1 {
		t.Fatalf("audit writes = %d, want exactly 1 per job", sink.count())
	}
	if n := len(jobsOf(t, db, "org_a")); n != 1 {
		t.Fatalf("stored jobs = %d, want 1", n)
	}
}

func TestCreateJob_RobotsIgnore_AuditFailureThenRetrySucceedsWithOneJob(t *testing.T) {
	t.Parallel()
	h, db, sink := robotsAuditServer(t, true, errors.New("down"))
	if w := postJobWithKey(h, "org_a", "retry-2", ignoreObjectJob()); w.Code != http.StatusServiceUnavailable {
		t.Fatalf("first: %d %s", w.Code, w.Body.String())
	}
	sink.mu.Lock()
	sink.err = nil
	sink.mu.Unlock()
	if w := postJobWithKey(h, "org_a", "retry-2", ignoreObjectJob()); w.Code != http.StatusCreated {
		t.Fatalf("retry after recovery: %d %s", w.Code, w.Body.String())
	}
	if n := len(jobsOf(t, db, "org_a")); n != 1 {
		t.Fatalf("stored jobs = %d, want 1", n)
	}
}
