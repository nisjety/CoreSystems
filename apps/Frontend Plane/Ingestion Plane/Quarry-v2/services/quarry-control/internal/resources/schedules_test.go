package resources

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/pg"
)

func newSchedulesServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db)
	return r, db
}

// desiredRows reads GET /v1/internal/schedules/desired, the orchestrator's
// all-orgs read in the wire shape (workflow, args, paused). The create response
// is the tenant view and does not carry these fields (WS15.S1).
func desiredRows(t *testing.T, h http.Handler) []map[string]any {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/v1/internal/schedules/desired", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("desired status=%d body=%s", w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	list, _ := env["data"].([]any)
	out := make([]map[string]any, 0, len(list))
	for _, it := range list {
		row, _ := it.(map[string]any)
		out = append(out, row)
	}
	return out
}

func decodeEnvelope(t *testing.T, body []byte) map[string]any {
	t.Helper()
	var env map[string]any
	if err := json.Unmarshal(body, &env); err != nil {
		t.Fatalf("decode envelope: %v; body=%s", err, string(body))
	}
	return env
}

func TestCreateSchedule_InvalidCron_Returns400(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)

	body := `{"cron":"not a cron","target_kind":"scrape","config":{"url":"https://example.com"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "BAD_REQUEST" {
		t.Fatalf("expected BAD_REQUEST, got %s", w.Body.String())
	}
}

func TestCreateSchedule_Valid_Returns201(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)

	// WS15.S1: the org comes from the query; an activated schedule is enabled.
	body := `{"name":"Five-minute scrape","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"},"activate":true}`
	code, env := postScheduleAs(t, h, "org_test", body)
	if code != http.StatusCreated {
		t.Fatalf("status = %d, want 201; env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data, env=%v", env)
	}
	id, _ := data["id"].(string)
	if !strings.HasPrefix(id, "sch_") {
		t.Fatalf("expected sch_ prefix, got %q", id)
	}
	if data["enabled"] != true || data["needs_activation"] != false {
		t.Fatalf("activated schedule: enabled=%v needs_activation=%v, want true/false", data["enabled"], data["needs_activation"])
	}
}

func createSchedule(t *testing.T, h http.Handler) string {
	t.Helper()
	body := `{"name":"Test scrape","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"},"activate":true}`
	code, env := postScheduleAs(t, h, "org_test", body)
	if code != http.StatusCreated {
		t.Fatalf("create: status=%d env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	id, _ := data["id"].(string)
	if id == "" {
		t.Fatalf("no id in create response")
	}
	return id
}

func getSchedule(t *testing.T, h http.Handler, id string) map[string]any {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/v1/schedules/"+id+"?org_id=org_test", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("get %s: status=%d body=%s", id, w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data on get; body=%s", w.Body.String())
	}
	return data
}

func TestEnableDisable_TogglesState(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	id := createSchedule(t, h)

	// disable
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/"+id+"/disable?org_id=org_test", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNoContent {
		t.Fatalf("disable status=%d body=%s", w.Code, w.Body.String())
	}
	if got := getSchedule(t, h, id); got["enabled"] != false {
		t.Fatalf("after disable, enabled=%v want false", got["enabled"])
	}

	// enable
	req = httptest.NewRequest(http.MethodPost, "/v1/schedules/"+id+"/enable?org_id=org_test", nil)
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNoContent {
		t.Fatalf("enable status=%d body=%s", w.Code, w.Body.String())
	}
	if got := getSchedule(t, h, id); got["enabled"] != true {
		t.Fatalf("after enable, enabled=%v want true", got["enabled"])
	}
}

func TestEnable_MissingID_Returns404(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)

	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/sch_missing/enable?org_id=org_test", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusNotFound {
		t.Fatalf("status=%d, want 404; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "NOT_FOUND" {
		t.Fatalf("expected NOT_FOUND, got %s", w.Body.String())
	}
}

func getRuns(t *testing.T, h http.Handler, path string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w.Code, decodeEnvelope(t, w.Body.Bytes())
}

func TestScheduleRuns_ListsJobs_DescOrder(t *testing.T) {
	h, db := newSchedulesServer(t)
	idStr := createSchedule(t, h)
	schID := quarrycontracts.ID(idStr)

	j1ID := quarrycontracts.NewID(quarrycontracts.KindJob)
	j2ID := quarrycontracts.NewID(quarrycontracts.KindJob)
	db.Jobs().Create(store.Job{ID: j1ID, OrgID: "org_test", Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: 1000})
	db.Jobs().Create(store.Job{ID: j2ID, OrgID: "org_test", Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: 2000})

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs?org_id=org_test")
	if code != http.StatusOK {
		t.Fatalf("status=%d, want 200; env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data; env=%v", env)
	}
	items, _ := data["items"].([]any)
	if len(items) != 2 {
		t.Fatalf("items len=%d, want 2", len(items))
	}
	first, _ := items[0].(map[string]any)
	if first["id"] != string(j2ID) {
		t.Fatalf("expected newest job first: got %v, want %s", first["id"], j2ID)
	}
}

func TestScheduleRuns_UnknownID_Returns404(t *testing.T) {
	h, _ := newSchedulesServer(t)
	code, env := getRuns(t, h, "/v1/schedules/sch_missing/runs?org_id=org_test")
	if code != http.StatusNotFound {
		t.Fatalf("status=%d, want 404; env=%v", code, env)
	}
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "NOT_FOUND" {
		t.Fatalf("expected NOT_FOUND, got %v", env)
	}
}

func TestScheduleRuns_WrongKind_Returns400(t *testing.T) {
	h, _ := newSchedulesServer(t)
	code, env := getRuns(t, h, "/v1/schedules/job_xxx/runs?org_id=org_test")
	if code != http.StatusBadRequest {
		t.Fatalf("status=%d, want 400; env=%v", code, env)
	}
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "BAD_REQUEST" {
		t.Fatalf("expected BAD_REQUEST, got %v", env)
	}
}

// TestScheduleRuns_CrossTenantRejected guards the IDOR: an org that
// doesn't own the schedule must see 404, identical to an unknown id —
// never another tenant's job history.
func TestScheduleRuns_CrossTenantRejected(t *testing.T) {
	h, _ := newSchedulesServer(t)
	idStr := createSchedule(t, h) // owned by org_test

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs?org_id=org_other")
	if code != http.StatusNotFound {
		t.Fatalf("status=%d, want 404 for cross-tenant access; env=%v", code, env)
	}
}

// TestScheduleRuns_MissingOrgID_Returns400 guards against an unscoped
// query silently listing whichever org happens to own the schedule.
func TestScheduleRuns_MissingOrgID_Returns400(t *testing.T) {
	h, _ := newSchedulesServer(t)
	idStr := createSchedule(t, h)

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs")
	if code != http.StatusBadRequest {
		t.Fatalf("status=%d, want 400 without org_id; env=%v", code, env)
	}
}

func TestScheduleRuns_Empty_ReturnsEmptyArray(t *testing.T) {
	h, _ := newSchedulesServer(t)
	idStr := createSchedule(t, h)

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs?org_id=org_test")
	if code != http.StatusOK {
		t.Fatalf("status=%d, want 200; env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data; env=%v", env)
	}
	items, ok := data["items"].([]any)
	if !ok {
		t.Fatalf("items missing or wrong type; data=%v", data)
	}
	if len(items) != 0 {
		t.Fatalf("items len=%d, want 0", len(items))
	}
}

func TestScheduleRuns_CursorPagination(t *testing.T) {
	h, db := newSchedulesServer(t)
	idStr := createSchedule(t, h)
	schID := quarrycontracts.ID(idStr)

	for i := 0; i < 3; i++ {
		jID := quarrycontracts.NewID(quarrycontracts.KindJob)
		db.Jobs().Create(store.Job{ID: jID, OrgID: "org_test", Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: int64(1000 + i)})
	}

	seen := 0
	cursor := ""
	for page := 0; page < 5; page++ {
		path := "/v1/schedules/" + idStr + "/runs?org_id=org_test&limit=1"
		if cursor != "" {
			path += "&cursor=" + cursor
		}
		code, env := getRuns(t, h, path)
		if code != http.StatusOK {
			t.Fatalf("page %d: status=%d; env=%v", page, code, env)
		}
		data, _ := env["data"].(map[string]any)
		items, _ := data["items"].([]any)
		seen += len(items)
		next, _ := data["next_cursor"].(string)
		if next == "" {
			break
		}
		cursor = next
	}
	if seen != 3 {
		t.Fatalf("paged through %d items, want 3", seen)
	}
}

// --- W2: org_id + change_monitor preset → orchestrator wire contract -------

func postSchedule(t *testing.T, h http.Handler, body string) (int, map[string]any) {
	t.Helper()
	var probe struct {
		OrgID string `json:"org_id"`
	}
	_ = json.Unmarshal([]byte(body), &probe)
	target := "/v1/schedules"
	if probe.OrgID != "" {
		target += "?org_id=" + url.QueryEscape(probe.OrgID)
	}
	req := httptest.NewRequest(http.MethodPost, target, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w.Code, decodeEnvelope(t, w.Body.Bytes())
}

func TestCreateSchedule_MissingOrgID_Returns400(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postSchedule(t, h,
		`{"cron":"*/5 * * * *","target_kind":"scrape","target_ref":"https://example.com"}`)
	if code != http.StatusBadRequest {
		t.Fatalf("status=%d, want 400; env=%v", code, env)
	}
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "BAD_REQUEST" {
		t.Fatalf("expected BAD_REQUEST, got %v", env)
	}
}

// TestCreateSchedule_ChangeMonitor_EmitsWorkflowArgs is the load-bearing W2
// test: a preset-driven change_monitor schedule must serialize to the
// orchestrator with Workflow=ChangeMonitorWF, Args carrying org_id+url, the
// preset mapped to a literal 5-field cron, and paused == !enabled. The wire
// shape is read from the desired endpoint: since WS15.S1 the create response is
// the tenant view, which carries cron and status but not workflow or args.
func TestCreateSchedule_ChangeMonitor_EmitsWorkflowArgs(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)

	code, env := postSchedule(t, h,
		`{"org_id":"org_verevon","target_kind":"change_monitor","target_ref":"https://example.com/pricing","preset":"daily","activate":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d, want 201; env=%v", code, env)
	}
	if view, _ := env["data"].(map[string]any); view == nil || view["cron"] != "0 9 * * *" || view["status"] != "active" {
		t.Fatalf("tenant view cron/status=%v, want daily preset and active", view)
	}
	rows := desiredRows(t, h)
	if len(rows) != 1 {
		t.Fatalf("desired len=%d, want 1; rows=%v", len(rows), rows)
	}
	data := rows[0]
	if data["workflow"] != "ChangeMonitorWF" {
		t.Fatalf("workflow=%v, want ChangeMonitorWF", data["workflow"])
	}
	if data["cron"] != "0 9 * * *" {
		t.Fatalf("cron=%v, want daily preset '0 9 * * *'", data["cron"])
	}
	if data["paused"] != false {
		t.Fatalf("paused=%v, want false (enabled schedule)", data["paused"])
	}
	args, _ := data["args"].([]any)
	if len(args) != 1 {
		t.Fatalf("args len=%d, want 1; data=%v", len(args), data)
	}
	arg0, _ := args[0].(map[string]any)
	if arg0["org_id"] != "org_verevon" || arg0["url"] != "https://example.com/pricing" {
		t.Fatalf("args[0]=%v, want {org_id:org_verevon, url:.../pricing}", arg0)
	}
}

func TestCreateSchedule_ChangeMonitor_BadPreset_Returns400(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postSchedule(t, h,
		`{"org_id":"org_verevon","target_kind":"change_monitor","target_ref":"https://example.com","preset":"every-minute"}`)
	if code != http.StatusBadRequest {
		t.Fatalf("status=%d, want 400; env=%v", code, env)
	}
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "BAD_REQUEST" {
		t.Fatalf("expected BAD_REQUEST, got %v", env)
	}
}

// Non-change_monitor schedules are intentionally NOT mapped to a Temporal
// workflow in the W2 MVP — the reconciler skips empty-workflow specs.
func TestCreateSchedule_Scrape_EmptyWorkflow(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postSchedule(t, h,
		`{"org_id":"org_test","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"},"activate":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d, want 201; env=%v", code, env)
	}
	_ = env
	// The kill switch QUARRY_SCHEDULED_JOBS_ENABLED is unset here, so the kind
	// stays unmapped in the orchestrator wire shape.
	rows := desiredRows(t, h)
	if len(rows) != 1 {
		t.Fatalf("desired len=%d, want 1; rows=%v", len(rows), rows)
	}
	if rows[0]["workflow"] != "" {
		t.Fatalf("scrape workflow=%v, want empty (unmapped while the kill switch is off)", rows[0]["workflow"])
	}
}

// --- WS15.S1: tenant-safe schedule persistence and creation ------------------

const crawlScheduleBody = `{"name":"Nightly crawl","cron":"0 7 * * *","timezone":"Europe/Oslo","target_kind":"crawl","config":{"url":"https://example.com/docs","max_pages":20}}`

// postScheduleAs posts a create with org_id on the query, as the edge does.
func postScheduleAs(t *testing.T, h http.Handler, org, body string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules?org_id="+url.QueryEscape(org), strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w.Code, decodeEnvelope(t, w.Body.Bytes())
}

// callAs sends a bodyless request and returns the status and raw body.
func callAs(t *testing.T, h http.Handler, method, target string) (int, []byte) {
	t.Helper()
	req := httptest.NewRequest(method, target, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w.Code, w.Body.Bytes()
}

// errReason is error.details.reason, or "" when the envelope carries none.
func errReason(env map[string]any) string {
	errObj, _ := env["error"].(map[string]any)
	details, _ := errObj["details"].(map[string]any)
	reason, _ := details["reason"].(string)
	return reason
}

func TestCreateSchedule_Crawl_PersistsConfigAndStartsDisabled(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postScheduleAs(t, h, "org_a", crawlScheduleBody)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data["enabled"] != false || data["status"] != "paused" || data["needs_activation"] != true {
		t.Fatalf("a new crawl must start disabled and need activation: %v", data)
	}
	if data["name"] != "Nightly crawl" || data["timezone"] != "Europe/Oslo" || data["kind"] != "crawl" {
		t.Fatalf("definition not stored: %v", data)
	}
	cfg, _ := data["config"].(map[string]any)
	if cfg["url"] != "https://example.com/docs" || cfg["max_pages"] != float64(20) {
		t.Fatalf("config not stored: %v", cfg)
	}
	id, _ := data["schedule_id"].(string)
	if !strings.HasPrefix(id, "sch_") {
		t.Fatalf("schedule_id=%q", id)
	}

	// Read back through the tenant route: the same definition, still disabled.
	code, raw := callAs(t, h, http.MethodGet, "/v1/schedules/"+id+"?org_id=org_a")
	if code != http.StatusOK {
		t.Fatalf("read back: status=%d body=%s", code, raw)
	}
	back, _ := decodeEnvelope(t, raw)["data"].(map[string]any)
	if back["name"] != "Nightly crawl" || back["enabled"] != false {
		t.Fatalf("read back: %v", back)
	}
}

func TestCreateSchedule_OrgFromQueryIgnoresBodyOrg(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	body := `{"org_id":"org_b","name":"Scrape","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"}}`
	code, env := postScheduleAs(t, h, "org_a", body)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	id, _ := env["data"].(map[string]any)["schedule_id"].(string)
	if code, _ := callAs(t, h, http.MethodGet, "/v1/schedules/"+id+"?org_id=org_a"); code != http.StatusOK {
		t.Fatalf("the row must belong to the query org (org_a): status=%d", code)
	}
	if code, _ := callAs(t, h, http.MethodGet, "/v1/schedules/"+id+"?org_id=org_b"); code != http.StatusNotFound {
		t.Fatalf("the body org_id must not own the row: status=%d, want 404", code)
	}
}

func TestCreateSchedule_StartsDisabledUnlessActivated(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	// The body's enabled flag is not read: a create without activate is disabled.
	code, env := postScheduleAs(t, h, "org_a",
		`{"cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"},"enabled":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	if data := env["data"].(map[string]any); data["enabled"] != false {
		t.Fatalf("enabled=%v, want false without activate", data["enabled"])
	}
	code, env = postScheduleAs(t, h, "org_a",
		`{"cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://example.com"},"activate":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	if data := env["data"].(map[string]any); data["enabled"] != true || data["status"] != "active" {
		t.Fatalf("activated: %v", data)
	}
}

// TestCreateSchedule_RefusesDefinitionsItCannotRun: each refusal is a 400 with a
// stable reason in error.details.reason.
func TestCreateSchedule_RefusesDefinitionsItCannotRun(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	cases := []struct {
		name, body, reason string
	}{
		{"unknown kind", `{"cron":"*/5 * * * *","target_kind":"agent","config":{"url":"https://example.com"}}`, "unsupported_schedule_kind"},
		{"bad timezone", `{"cron":"0 7 * * *","timezone":"Mars/Olympus","target_kind":"crawl","config":{"url":"https://example.com"}}`, "invalid_timezone"},
		{"local timezone", `{"cron":"0 7 * * *","timezone":"Local","target_kind":"crawl","config":{"url":"https://example.com"}}`, "invalid_timezone"},
		{"too frequent crawl", `{"cron":"*/10 * * * *","target_kind":"crawl","config":{"url":"https://example.com"}}`, "cadence_too_frequent"},
		{"scrape without url", `{"cron":"*/5 * * * *","target_kind":"scrape"}`, "invalid_config"},
		{"url with credentials", `{"cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://user:pw@example.com"}}`, "invalid_config"},
		{"batch with no urls", `{"cron":"0 7 * * *","target_kind":"batch","config":{"urls":[]}}`, "invalid_config"},
		{"bad overlap", `{"cron":"*/5 * * * *","target_kind":"scrape","overlap_policy":"queue","config":{"url":"https://example.com"}}`, "invalid_overlap_policy"},
		{"catch-up too long", `{"cron":"0 7 * * *","target_kind":"crawl","catchup_window_s":90000,"config":{"url":"https://example.com"}}`, "invalid_catchup_window"},
	}
	for _, tc := range cases {
		code, env := postScheduleAs(t, h, "org_a", tc.body)
		if code != http.StatusBadRequest {
			t.Errorf("%s: status=%d env=%v, want 400", tc.name, code, env)
			continue
		}
		if got := errReason(env); got != tc.reason {
			t.Errorf("%s: reason=%q, want %q", tc.name, got, tc.reason)
		}
	}
}

func TestListSchedules_OrgScoped(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	if code, env := postScheduleAs(t, h, "org_a", crawlScheduleBody); code != http.StatusCreated {
		t.Fatalf("org_a create: status=%d env=%v", code, env)
	}
	if code, env := postScheduleAs(t, h, "org_b",
		`{"name":"B scrape","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://b.example.com"}}`); code != http.StatusCreated {
		t.Fatalf("org_b create: status=%d env=%v", code, env)
	}
	for org, want := range map[string]string{"org_a": "Nightly crawl", "org_b": "B scrape"} {
		code, raw := callAs(t, h, http.MethodGet, "/v1/schedules?org_id="+org)
		if code != http.StatusOK {
			t.Fatalf("%s list: status=%d body=%s", org, code, raw)
		}
		page, _ := decodeEnvelope(t, raw)["data"].(map[string]any)
		items, _ := page["items"].([]any)
		if len(items) != 1 {
			t.Fatalf("%s sees %d rows, want exactly its own 1: %s", org, len(items), raw)
		}
		row, _ := items[0].(map[string]any)
		if row["name"] != want {
			t.Fatalf("%s sees %v, want %q", org, row["name"], want)
		}
	}
}

// TestGetDeleteEnableDisable_ForeignOrgIs404: org B can read, change or delete
// nothing of org A, and a foreign id answers exactly like a missing one.
func TestGetDeleteEnableDisable_ForeignOrgIs404(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postScheduleAs(t, h, "org_a", crawlScheduleBody)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	id, _ := env["data"].(map[string]any)["schedule_id"].(string)
	base := "/v1/schedules/" + id
	for _, call := range []struct{ method, path string }{
		{http.MethodGet, base + "?org_id=org_b"},
		{http.MethodDelete, base + "?org_id=org_b"},
		{http.MethodPost, base + "/enable?org_id=org_b"},
		{http.MethodPost, base + "/disable?org_id=org_b"},
	} {
		if code, raw := callAs(t, h, call.method, call.path); code != http.StatusNotFound {
			t.Fatalf("%s %s as org_b: status=%d body=%s, want 404", call.method, call.path, code, raw)
		}
	}
	// The row is intact: still org_a's, still disabled.
	code, raw := callAs(t, h, http.MethodGet, base+"?org_id=org_a")
	if code != http.StatusOK {
		t.Fatalf("org_a read after foreign calls: status=%d body=%s", code, raw)
	}
	if got, _ := decodeEnvelope(t, raw)["data"].(map[string]any); got["enabled"] != false {
		t.Fatalf("foreign calls changed the row: %v", got)
	}
	// Its own org can delete it, and then it is gone.
	if code, _ := callAs(t, h, http.MethodDelete, base+"?org_id=org_a"); code != http.StatusNoContent {
		t.Fatalf("own delete: status=%d, want 204", code)
	}
	if code, _ := callAs(t, h, http.MethodGet, base+"?org_id=org_a"); code != http.StatusNotFound {
		t.Fatalf("after own delete: status=%d, want 404", code)
	}
}

// TestInternalDesired_ReturnsAllOrgs: the orchestrator reads every org's rows in
// the wire shape on the internal path.
func TestInternalDesired_ReturnsAllOrgs(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	if code, env := postScheduleAs(t, h, "org_a", crawlScheduleBody); code != http.StatusCreated {
		t.Fatalf("org_a create: status=%d env=%v", code, env)
	}
	if code, env := postScheduleAs(t, h, "org_b",
		`{"cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://b.example.com"}}`); code != http.StatusCreated {
		t.Fatalf("org_b create: status=%d env=%v", code, env)
	}
	code, raw := callAs(t, h, http.MethodGet, "/v1/internal/schedules/desired")
	if code != http.StatusOK {
		t.Fatalf("desired: status=%d body=%s", code, raw)
	}
	rows, _ := decodeEnvelope(t, raw)["data"].([]any)
	if len(rows) != 2 {
		t.Fatalf("desired returned %d rows, want 2 (all orgs): %s", len(rows), raw)
	}
	orgs := map[string]bool{}
	for _, r := range rows {
		row, _ := r.(map[string]any)
		orgs[row["org_id"].(string)] = true
		if _, ok := row["workflow"]; !ok {
			t.Fatalf("desired row lacks the wire field workflow: %v", row)
		}
	}
	if !orgs["org_a"] || !orgs["org_b"] {
		t.Fatalf("orgs=%v, want org_a and org_b", orgs)
	}
}

// TestListSchedules_WithoutOrgIsRefused (D2-042): GET /v1/schedules without an
// org is a 400 whatever the environment says, and never an all-orgs list. The
// old QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG switch is retired, so the answer is
// the same unset, 1 and 0. Not parallel: it sets an environment variable.
func TestListSchedules_WithoutOrgIsRefused(t *testing.T) {
	h, _ := newSchedulesServer(t)
	for _, org := range []string{"org_a", "org_b"} {
		if code, env := postScheduleAs(t, h, org, crawlScheduleBody); code != http.StatusCreated {
			t.Fatalf("%s create: status=%d env=%v", org, code, env)
		}
	}
	for _, setting := range []string{"", "1", "0"} {
		t.Setenv(EnvSchedulesRequireOrg, setting)
		for _, target := range []string{"/v1/schedules", "/v1/schedules?org_id=", "/v1/schedules?limit=50"} {
			code, raw := callAs(t, h, http.MethodGet, target)
			if code != http.StatusBadRequest {
				t.Fatalf("%s=%q GET %s: status=%d, want 400 (body=%s)", EnvSchedulesRequireOrg, setting, target, code, raw)
			}
			if strings.Contains(string(raw), "org_a") || strings.Contains(string(raw), "org_b") {
				t.Fatalf("the refusal must not carry any org's rows: %s", raw)
			}
		}
	}
	// The orchestrator reconciles from the internal desired read, so removing the
	// branch cannot take its input away.
	if rows := desiredRows(t, h); len(rows) != 2 || rows[0]["args"] == nil {
		t.Fatalf("the desired read must still answer both orgs in the wire shape: %v", rows)
	}
}

// TestListSchedules_NeverCrossesOrgs (D2-042): with several rows per org, each
// org lists only its own, across pages, and an org with no rows gets an empty
// page rather than somebody else's.
func TestListSchedules_NeverCrossesOrgs(t *testing.T) {
	t.Parallel()
	h, db := newSchedulesServer(t)
	body := func(n string) string {
		return `{"name":"` + n + `","cron":"*/5 * * * *","target_kind":"scrape","config":{"url":"https://` + n + `.example.com"}}`
	}
	want := map[string]map[string]bool{"org_a": {}, "org_b": {}}
	for org, names := range map[string][]string{"org_a": {"a1", "a2", "a3"}, "org_b": {"b1", "b2"}} {
		for _, n := range names {
			if code, env := postScheduleAs(t, h, org, body(n)); code != http.StatusCreated {
				t.Fatalf("%s create %s: status=%d env=%v", org, n, code, env)
			}
			want[org][n] = true
		}
	}
	for org, names := range want {
		got := map[string]bool{}
		cursor := ""
		for page := 0; page < 10; page++ {
			target := "/v1/schedules?org_id=" + org + "&limit=2"
			if cursor != "" {
				target += "&cursor=" + url.QueryEscape(cursor)
			}
			code, raw := callAs(t, h, http.MethodGet, target)
			if code != http.StatusOK {
				t.Fatalf("%s list: status=%d body=%s", org, code, raw)
			}
			data, _ := decodeEnvelope(t, raw)["data"].(map[string]any)
			items, _ := data["items"].([]any)
			for _, it := range items {
				row, _ := it.(map[string]any)
				if row["org_id"] != nil && row["org_id"] != org {
					t.Fatalf("%s page carries a row of %v: %v", org, row["org_id"], row)
				}
				n, _ := row["name"].(string)
				if !names[n] {
					t.Fatalf("%s sees %q, which belongs to the other org", org, n)
				}
				got[n] = true
			}
			next, _ := data["next_cursor"].(string)
			if next == "" {
				break
			}
			cursor = next
		}
		if len(got) != len(names) {
			t.Fatalf("%s paged through %v, want exactly %v", org, got, names)
		}
	}
	code, raw := callAs(t, h, http.MethodGet, "/v1/schedules?org_id=org_empty")
	if code != http.StatusOK {
		t.Fatalf("empty org list: status=%d body=%s", code, raw)
	}
	data, _ := decodeEnvelope(t, raw)["data"].(map[string]any)
	if items, _ := data["items"].([]any); len(items) != 0 {
		t.Fatalf("an org with no schedules must get an empty page, got %v", items)
	}
	_ = db
}

// TestPgListSchedules_NeverCrossesOrgs is the same guarantee against Postgres.
// It skips unless QUARRY_TEST_DSN is set, and fails instead when
// QUARRY_REQUIRE_DB=1 (the CI database lane), like the store/pg tests.
func TestPgListSchedules_NeverCrossesOrgs(t *testing.T) {
	dsn := os.Getenv("QUARRY_TEST_DSN")
	if dsn == "" {
		if os.Getenv("QUARRY_REQUIRE_DB") == "1" {
			t.Fatalf("QUARRY_TEST_DSN is unset but QUARRY_REQUIRE_DB=1: the postgres tests must run here")
		}
		t.Skip("QUARRY_TEST_DSN unset: skipping the postgres schedule scoping test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	db, err := pg.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pg.New: %v", err)
	}
	suffix := strings.ToLower(string(quarrycontracts.NewID(quarrycontracts.KindSchedule)))
	orgA, orgB := "org_scope_a_"+suffix, "org_scope_b_"+suffix
	var made []quarrycontracts.ID
	for _, org := range []string{orgA, orgB} {
		for i := 0; i < 2; i++ {
			sc := store.Schedule{
				ID: quarrycontracts.NewID(quarrycontracts.KindSchedule), OrgID: org,
				Cron: "0 9 * * *", TargetKind: store.TargetKindChangeMonitor, TargetRef: "https://x.example/" + org,
				Enabled: false, CreatedAt: time.Now().UnixMilli(),
			}
			if err := db.Schedules().Create(sc); err != nil {
				t.Fatalf("create: %v", err)
			}
			made = append(made, sc.ID)
		}
	}
	t.Cleanup(func() {
		for _, id := range made {
			_ = db.Schedules().Delete(id)
		}
	})
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db)

	if code, raw := callAs(t, r, http.MethodGet, "/v1/schedules"); code != http.StatusBadRequest {
		t.Fatalf("no org: status=%d, want 400 (body=%s)", code, raw)
	}
	for org, other := range map[string]string{orgA: orgB, orgB: orgA} {
		code, raw := callAs(t, r, http.MethodGet, "/v1/schedules?org_id="+org+"&limit=100")
		if code != http.StatusOK {
			t.Fatalf("%s list: status=%d body=%s", org, code, raw)
		}
		data, _ := decodeEnvelope(t, raw)["data"].(map[string]any)
		items, _ := data["items"].([]any)
		if len(items) != 2 {
			t.Fatalf("%s sees %d rows, want its own 2: %s", org, len(items), raw)
		}
		if strings.Contains(string(raw), other) {
			t.Fatalf("%s page carries the other org %s: %s", org, other, raw)
		}
	}
}
