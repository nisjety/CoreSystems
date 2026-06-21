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

func newSchedulesServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db)
	return r, db
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

	body := `{"cron":"not a cron","target_kind":"scrape","target_ref":"https://example.com"}`
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules", strings.NewReader(body))
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

	body := `{"org_id":"org_test","cron":"*/5 * * * *","target_kind":"scrape","target_ref":"https://example.com","enabled":true}`
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data, body=%s", w.Body.String())
	}
	id, _ := data["id"].(string)
	if !strings.HasPrefix(id, "sch_") {
		t.Fatalf("expected sch_ prefix, got %q", id)
	}
	if data["enabled"] != true {
		t.Fatalf("enabled = %v, want true", data["enabled"])
	}
}

func createSchedule(t *testing.T, h http.Handler) string {
	t.Helper()
	body := `{"org_id":"org_test","cron":"*/5 * * * *","target_kind":"scrape","target_ref":"https://example.com","enabled":true}`
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules", bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("create: status=%d body=%s", w.Code, w.Body.String())
	}
	env := decodeEnvelope(t, w.Body.Bytes())
	data, _ := env["data"].(map[string]any)
	id, _ := data["id"].(string)
	if id == "" {
		t.Fatalf("no id in create response")
	}
	return id
}

func getSchedule(t *testing.T, h http.Handler, id string) map[string]any {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/v1/schedules/"+id, nil)
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
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/"+id+"/disable", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNoContent {
		t.Fatalf("disable status=%d body=%s", w.Code, w.Body.String())
	}
	if got := getSchedule(t, h, id); got["enabled"] != false {
		t.Fatalf("after disable, enabled=%v want false", got["enabled"])
	}

	// enable
	req = httptest.NewRequest(http.MethodPost, "/v1/schedules/"+id+"/enable", nil)
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

	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/sch_missing/enable", nil)
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
	db.Jobs().Create(store.Job{ID: j1ID, Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: 1000})
	db.Jobs().Create(store.Job{ID: j2ID, Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: 2000})

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs")
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
	code, env := getRuns(t, h, "/v1/schedules/sch_missing/runs")
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
	code, env := getRuns(t, h, "/v1/schedules/job_xxx/runs")
	if code != http.StatusBadRequest {
		t.Fatalf("status=%d, want 400; env=%v", code, env)
	}
	errObj, _ := env["error"].(map[string]any)
	if errObj == nil || errObj["code"] != "BAD_REQUEST" {
		t.Fatalf("expected BAD_REQUEST, got %v", env)
	}
}

func TestScheduleRuns_Empty_ReturnsEmptyArray(t *testing.T) {
	h, _ := newSchedulesServer(t)
	idStr := createSchedule(t, h)

	code, env := getRuns(t, h, "/v1/schedules/"+idStr+"/runs")
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
		db.Jobs().Create(store.Job{ID: jID, Kind: "scrape", Status: "queued", ScheduleID: &schID, CreatedAt: int64(1000 + i)})
	}

	seen := 0
	cursor := ""
	for page := 0; page < 5; page++ {
		path := "/v1/schedules/" + idStr + "/runs?limit=1"
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
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules", strings.NewReader(body))
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
// preset mapped to a literal 5-field cron, and paused == !enabled.
func TestCreateSchedule_ChangeMonitor_EmitsWorkflowArgs(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)

	code, env := postSchedule(t, h,
		`{"org_id":"org_velion","target_kind":"change_monitor","target_ref":"https://example.com/pricing","preset":"daily","enabled":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d, want 201; env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data; env=%v", env)
	}
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
	if arg0["org_id"] != "org_velion" || arg0["url"] != "https://example.com/pricing" {
		t.Fatalf("args[0]=%v, want {org_id:org_velion, url:.../pricing}", arg0)
	}

	// The list view (what the reconciler reads) must carry the same shape.
	req := httptest.NewRequest(http.MethodGet, "/v1/schedules", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	listEnv := decodeEnvelope(t, w.Body.Bytes())
	list, _ := listEnv["data"].([]any)
	if len(list) != 1 {
		t.Fatalf("list len=%d, want 1; env=%v", len(list), listEnv)
	}
	first, _ := list[0].(map[string]any)
	if first["workflow"] != "ChangeMonitorWF" {
		t.Fatalf("list[0].workflow=%v, want ChangeMonitorWF", first["workflow"])
	}
}

func TestCreateSchedule_ChangeMonitor_BadPreset_Returns400(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postSchedule(t, h,
		`{"org_id":"org_velion","target_kind":"change_monitor","target_ref":"https://example.com","preset":"every-minute"}`)
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
		`{"org_id":"org_test","cron":"*/5 * * * *","target_kind":"scrape","target_ref":"https://example.com","enabled":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d, want 201; env=%v", code, env)
	}
	data, _ := env["data"].(map[string]any)
	if data["workflow"] != "" {
		t.Fatalf("scrape workflow=%v, want empty (unmapped in MVP)", data["workflow"])
	}
}
