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
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// The in-memory request table answers the shared contract (WS15.S3).
func TestScheduleRequestsStore_MemoryConformance(t *testing.T) {
	storetest.ScheduleRequestsConformance(t, func(t *testing.T) store.DB { return store.NewMemory() })
}

func TestScheduleRequestsEnabled_DefaultOff(t *testing.T) {
	for v, want := range map[string]bool{"": false, "0": false, "off": false, "1": true, "true": true, " ON ": true, "yes": true} {
		got := ScheduleRequestsEnabled(func(string) string { return v })
		if got != want {
			t.Fatalf("ScheduleRequestsEnabled(%q)=%v, want %v", v, got, want)
		}
	}
}

func newScheduleRequestsServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	t.Setenv(EnvScheduledJobsEnabled, "1")
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	mountScheduleAliases(r, db, nil, true)
	return r, db
}

func seedRequestSchedule(t *testing.T, db store.DB, org, kind, cron string, enabled bool) quarrycontracts.ID {
	t.Helper()
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if err := db.Schedules().Create(store.Schedule{
		ID: id, OrgID: org, Cron: cron, TargetKind: kind,
		TargetRef: "https://example.com", Enabled: enabled,
		Config: map[string]any{"url": "https://example.com"},
	}); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}
	return id
}

func doJSON(t *testing.T, h http.Handler, method, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	var req *http.Request
	if body == "" {
		req = httptest.NewRequest(method, path, nil)
	} else {
		req = httptest.NewRequest(method, path, strings.NewReader(body))
	}
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w
}

func dataOf(t *testing.T, w *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var env struct {
		Data map[string]any `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode: %v; raw=%s", err, w.Body.String())
	}
	return env.Data
}

func refusalReason(t *testing.T, w *httptest.ResponseRecorder) string {
	t.Helper()
	var env struct {
		Error struct {
			Code    string         `json:"code"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode: %v; raw=%s", err, w.Body.String())
	}
	reason, _ := env.Error.Details["reason"].(string)
	return reason
}

func claimedOf(t *testing.T, w *httptest.ResponseRecorder) []claimedRequestView {
	t.Helper()
	if w.Code != http.StatusOK {
		t.Fatalf("claim: %d %s", w.Code, w.Body.String())
	}
	var env struct {
		Data claimRequestsResult `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode claim: %v", err)
	}
	return env.Data.Requests
}

func rfc(tm time.Time) string { return tm.UTC().Format(time.RFC3339) }

func backfillBodyJSON(start, end time.Time) string {
	return `{"start_at":"` + rfc(start) + `","end_at":"` + rfc(end) + `"}`
}

func TestMountScheduleAliases_FlagOffKeepsThePreviousAnswer(t *testing.T) {
	r := chi.NewRouter()
	db := store.NewMemory()
	mountScheduleAliases(r, db, nil, false)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	if w := doJSON(t, r, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", ""); w.Code != http.StatusNotImplemented {
		t.Fatalf("flag off trigger: %d, want 501", w.Code)
	}
	if w := doJSON(t, r, http.MethodPost, "/v1/internal/schedules/requests/claim", `{"worker":"w"}`); w.Code != http.StatusNotFound && w.Code != http.StatusMethodNotAllowed {
		t.Fatalf("flag off claim route must not be mounted, got %d", w.Code)
	}
}

func TestTrigger_InsertsPendingAndReturns202(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)

	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a&user_id=usr_1", "")
	if w.Code != http.StatusAccepted {
		t.Fatalf("trigger: %d %s", w.Code, w.Body.String())
	}
	d := dataOf(t, w)
	rid, _ := d["request_id"].(string)
	if !strings.HasPrefix(rid, string(quarrycontracts.KindRequest)) || d["status"] != "pending" || d["action"] != "trigger" {
		t.Fatalf("202 body: %v", d)
	}

	reqs, _ := scheduleRequestsOf(db)
	row, ok := reqs.GetByOrg(quarrycontracts.ID(rid), "org_a")
	if !ok || row.Status != store.ScheduleRequestPending || row.RequestedBy != "usr_1" || row.ScheduleID != id {
		t.Fatalf("stored row: %+v ok=%v", row, ok)
	}

	g := doJSON(t, h, http.MethodGet, "/v1/schedules/"+string(id)+"/requests/"+rid+"?org_id=org_a", "")
	if g.Code != http.StatusOK {
		t.Fatalf("get request: %d %s", g.Code, g.Body.String())
	}
	view := dataOf(t, g)
	if view["status"] != "pending" || view["request_id"] != rid {
		t.Fatalf("request view: %v", view)
	}
	if _, leaks := view["claimed_by"]; leaks {
		t.Fatal("the tenant view must not carry the worker")
	}
}

func TestTrigger_DisabledScheduleIs409(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", false)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	if w.Code != http.StatusConflict || refusalReason(t, w) != "needs_activation" {
		t.Fatalf("paused schedule: %d %s", w.Code, w.Body.String())
	}
}

func TestTrigger_SecondPendingIs409(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	if w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", ""); w.Code != http.StatusAccepted {
		t.Fatalf("first trigger: %d", w.Code)
	}
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	if w.Code != http.StatusConflict || refusalReason(t, w) != "request_pending" {
		t.Fatalf("second trigger: %d %s", w.Code, w.Body.String())
	}
	// A backfill counts as the same one open request.
	b := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/backfill?org_id=org_a",
		backfillBodyJSON(time.Now().Add(-48*time.Hour), time.Now().Add(-time.Hour)))
	if b.Code != http.StatusConflict || refusalReason(t, b) != "request_pending" {
		t.Fatalf("backfill while a trigger waits: %d %s", b.Code, b.Body.String())
	}
}

func TestTrigger_ScheduledJobsSwitchOffIs409(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	t.Setenv(EnvScheduledJobsEnabled, "0")
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	if w.Code != http.StatusConflict || refusalReason(t, w) != "scheduled_jobs_disabled" {
		t.Fatalf("kill switch off: %d %s", w.Code, w.Body.String())
	}
	// A change monitor is materialised without the switch, so it can be triggered.
	mon := seedRequestSchedule(t, db, "org_a", store.TargetKindChangeMonitor, "0 3 * * *", true)
	if w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(mon)+"/trigger?org_id=org_a", ""); w.Code != http.StatusAccepted {
		t.Fatalf("monitor trigger: %d %s", w.Code, w.Body.String())
	}
}

func TestBackfill_CrawlKindRefused(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	for _, kind := range []string{"crawl", "batch"} {
		id := seedRequestSchedule(t, db, "org_a", kind, "0 3 * * *", true)
		w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/backfill?org_id=org_a",
			backfillBodyJSON(time.Now().Add(-72*time.Hour), time.Now().Add(-time.Minute)))
		if w.Code != http.StatusConflict || refusalReason(t, w) != "backfill_kind_refused" {
			t.Fatalf("%s backfill: %d %s", kind, w.Code, w.Body.String())
		}
	}
}

func TestBackfill_OverTwentyFourWindowsRefused(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 * * * *", true)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/backfill?org_id=org_a",
		backfillBodyJSON(time.Now().Add(-48*time.Hour), time.Now().Add(-time.Minute)))
	if w.Code != http.StatusBadRequest || refusalReason(t, w) != "backfill_too_many_windows" {
		t.Fatalf("48 hourly windows: %d %s", w.Code, w.Body.String())
	}
	reqs, _ := scheduleRequestsOf(db)
	if reqs.HasPending(id, "") {
		t.Fatal("a refused backfill left a row")
	}
}

func TestBackfill_ThreeDailyWindowsAccepted(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	start, end := time.Now().Add(-72*time.Hour), time.Now().Add(-time.Minute)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/backfill?org_id=org_a", backfillBodyJSON(start, end))
	if w.Code != http.StatusAccepted {
		t.Fatalf("backfill: %d %s", w.Code, w.Body.String())
	}
	rid, _ := dataOf(t, w)["request_id"].(string)
	reqs, _ := scheduleRequestsOf(db)
	row, ok := reqs.GetByOrg(quarrycontracts.ID(rid), "org_a")
	if !ok || row.Action != store.ScheduleRequestBackfill {
		t.Fatalf("row: %+v", row)
	}
	if row.Params["start_at"] != rfc(start) || row.Params["end_at"] != rfc(end) {
		t.Fatalf("window not stored: %v", row.Params)
	}
	if n, _ := row.Params["windows"].(int64); n != 3 {
		t.Fatalf("windows=%v, want 3", row.Params["windows"])
	}
}

func TestBackfill_InvalidWindowsRefused(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	path := "/v1/schedules/" + string(id) + "/backfill?org_id=org_a"
	now := time.Now()
	if w := doJSON(t, h, http.MethodPost, path, backfillBodyJSON(now.Add(-time.Hour), now.Add(-2*time.Hour))); w.Code != http.StatusBadRequest {
		t.Fatalf("end before start: %d", w.Code)
	}
	if w := doJSON(t, h, http.MethodPost, path, backfillBodyJSON(now.Add(-time.Hour), now.Add(48*time.Hour))); w.Code != http.StatusBadRequest || refusalReason(t, w) != "backfill_window_in_future" {
		t.Fatalf("future window: %d %s", w.Code, w.Body.String())
	}
	bad := `{"start_at":"` + rfc(now.Add(-48*time.Hour)) + `","end_at":"` + rfc(now.Add(-time.Hour)) + `","overlap_policy":"bogus"}`
	if w := doJSON(t, h, http.MethodPost, path, bad); w.Code != http.StatusBadRequest {
		t.Fatalf("bad overlap policy: %d", w.Code)
	}
}

func TestRequest_CrossOrgIs404(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	if w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_b", ""); w.Code != http.StatusNotFound {
		t.Fatalf("cross-org trigger: %d, want 404", w.Code)
	}
	if w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/backfill?org_id=org_b",
		backfillBodyJSON(time.Now().Add(-48*time.Hour), time.Now().Add(-time.Hour))); w.Code != http.StatusNotFound {
		t.Fatalf("cross-org backfill: %d, want 404", w.Code)
	}
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	rid, _ := dataOf(t, w)["request_id"].(string)
	if g := doJSON(t, h, http.MethodGet, "/v1/schedules/"+string(id)+"/requests/"+rid+"?org_id=org_b", ""); g.Code != http.StatusNotFound {
		t.Fatalf("cross-org read: %d, want 404", g.Code)
	}
	other := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	if g := doJSON(t, h, http.MethodGet, "/v1/schedules/"+string(other)+"/requests/"+rid+"?org_id=org_a", ""); g.Code != http.StatusNotFound {
		t.Fatalf("read under another schedule: %d, want 404", g.Code)
	}
}

func TestClaimAndResult_RoundTrip(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	rid, _ := dataOf(t, w)["request_id"].(string)

	c := doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/claim", `{"worker":"orch-1","limit":5,"lease_s":30}`)
	if c.Code != http.StatusOK {
		t.Fatalf("claim: %d %s", c.Code, c.Body.String())
	}
	var env struct {
		Data claimRequestsResult `json:"data"`
	}
	if err := json.Unmarshal(c.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if len(env.Data.Requests) != 1 || env.Data.Requests[0].ID != rid || env.Data.Requests[0].OrgID != "org_a" ||
		env.Data.Requests[0].Action != "trigger" || env.Data.Requests[0].Attempts != 1 {
		t.Fatalf("claimed: %+v", env.Data.Requests)
	}
	if again := claimedOf(t, doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/claim", `{"worker":"orch-2"}`)); len(again) != 0 {
		t.Fatalf("a held row was claimed twice: %+v", again)
	}

	resultPath := "/v1/internal/schedules/requests/" + rid + "/result"
	if r := doJSON(t, h, http.MethodPost, resultPath, `{"worker":"orch-2","status":"done"}`); r.Code != http.StatusConflict {
		t.Fatalf("result by a worker that does not hold it: %d, want 409", r.Code)
	}
	if r := doJSON(t, h, http.MethodPost, resultPath, `{"worker":"orch-1","status":"bogus"}`); r.Code != http.StatusBadRequest {
		t.Fatalf("bad status: %d, want 400", r.Code)
	}
	if r := doJSON(t, h, http.MethodPost, resultPath, `{"worker":"orch-1","status":"done","result":{"schedule_id":"`+string(id)+`"}}`); r.Code != http.StatusNoContent {
		t.Fatalf("result: %d %s", r.Code, r.Body.String())
	}
	g := doJSON(t, h, http.MethodGet, "/v1/schedules/"+string(id)+"/requests/"+rid+"?org_id=org_a", "")
	if v := dataOf(t, g); v["status"] != "done" {
		t.Fatalf("after result: %v", v)
	}
	unknown := "/v1/internal/schedules/requests/" + string(quarrycontracts.NewID(quarrycontracts.KindRequest)) + "/result"
	if r := doJSON(t, h, http.MethodPost, unknown, `{"worker":"orch-1","status":"done"}`); r.Code != http.StatusNotFound {
		t.Fatalf("unknown request: %d, want 404", r.Code)
	}
	// The schedule is free again.
	if w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", ""); w.Code != http.StatusAccepted {
		t.Fatalf("trigger after done: %d", w.Code)
	}
}

func TestClaim_RetryGivesTheRowBack(t *testing.T) {
	h, db := newScheduleRequestsServer(t)
	id := seedRequestSchedule(t, db, "org_a", "scrape", "0 3 * * *", true)
	w := doJSON(t, h, http.MethodPost, "/v1/schedules/"+string(id)+"/trigger?org_id=org_a", "")
	rid, _ := dataOf(t, w)["request_id"].(string)
	doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/claim", `{"worker":"orch-1"}`)
	if r := doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/"+rid+"/result",
		`{"worker":"orch-1","status":"retry","error":"temporal unavailable"}`); r.Code != http.StatusNoContent {
		t.Fatalf("retry: %d %s", r.Code, r.Body.String())
	}
	got := claimedOf(t, doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/claim", `{"worker":"orch-2"}`))
	if len(got) != 1 || got[0].Attempts != 2 {
		t.Fatalf("reclaim after retry: %+v", got)
	}
}

func TestClaim_WorkerRequired(t *testing.T) {
	h, _ := newScheduleRequestsServer(t)
	if w := doJSON(t, h, http.MethodPost, "/v1/internal/schedules/requests/claim", `{}`); w.Code != http.StatusBadRequest {
		t.Fatalf("claim without worker: %d, want 400", w.Code)
	}
}

// The internal routes are in control's orchestrator-only HMAC group.
func TestScheduleRequestRoutes_AreOrchestratorOnly(t *testing.T) {
	for _, p := range []string{
		"/v1/internal/schedules/requests/claim",
		"/v1/internal/schedules/requests/req_01/result",
	} {
		if !httpx.IsOrchestratorOnlyPath(p) {
			t.Fatalf("%s is not in the orchestrator-only group", p)
		}
	}
}
