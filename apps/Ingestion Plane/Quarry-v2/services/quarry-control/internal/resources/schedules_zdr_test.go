package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// WS4.H: the caller's ZDR posture is stamped on a schedule at create and rides
// into the change-monitor workflow args, so the orchestrator can declare it on
// /v1/internal/change/record. These tests pin the wire contract in both
// directions (stamped and legacy/unstamped).

func firstArgs(t *testing.T, env map[string]any) map[string]any {
	t.Helper()
	data, _ := env["data"].(map[string]any)
	if data == nil {
		t.Fatalf("missing data; env=%v", env)
	}
	args, _ := data["args"].([]any)
	if len(args) != 1 {
		t.Fatalf("args len=%d, want 1; data=%v", len(args), data)
	}
	a0, _ := args[0].(map[string]any)
	if a0 == nil {
		t.Fatalf("args[0] is not an object: %v", args[0])
	}
	return a0
}

func TestCreateScheduleStampsZdr(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		name string
		zdr  string
		want bool
	}{{"zdr org", `true`, true}, {"standard org", `false`, false}} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			h, db := newSchedulesServer(t)
			code, env := postSchedule(t, h,
				`{"org_id":"org_z","target_kind":"change_monitor","target_ref":"https://example.com/p","preset":"daily","enabled":true,"zdr":`+tc.zdr+`}`)
			if code != http.StatusCreated {
				t.Fatalf("status=%d env=%v", code, env)
			}
			a0 := firstArgs(t, env)
			got, present := a0["zdr"]
			if tc.want {
				if got != true {
					t.Fatalf("args[0].zdr=%v present=%v, want true", got, present)
				}
			} else {
				// A stamped-standard schedule is false, not absent: omitempty on
				// a *bool keeps an explicit false.
				if !present || got != false {
					t.Fatalf("args[0].zdr=%v present=%v, want explicit false", got, present)
				}
			}
			rows, _ := db.Schedules().List(10, "")
			if len(rows) != 1 || rows[0].Zdr == nil || *rows[0].Zdr != tc.want {
				t.Fatalf("stored stamp=%v, want %v", rows, tc.want)
			}
		})
	}
}

// A schedule created before the stamp existed (or by a caller that sent no
// posture) must serialise to the exact args it always had: no "zdr" key.
func TestUnstampedScheduleArgsAreUnchanged(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	code, env := postSchedule(t, h,
		`{"org_id":"org_legacy","target_kind":"change_monitor","target_ref":"https://example.com/p","preset":"daily","enabled":true}`)
	if code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	if _, present := firstArgs(t, env)["zdr"]; present {
		t.Fatalf("an unstamped schedule must not emit a zdr key: %v", firstArgs(t, env))
	}
	raw, err := json.Marshal(changeMonitorArgs{OrgID: "o", URL: "https://x", CreatedBy: "u"})
	if err != nil {
		t.Fatal(err)
	}
	if string(raw) != `{"org_id":"o","url":"https://x","created_by":"u"}` {
		t.Fatalf("legacy args bytes changed: %s", raw)
	}
}

func TestWorkflowForCarriesZdr(t *testing.T) {
	t.Parallel()
	yes, no := true, false
	for _, tc := range []struct {
		name string
		in   *bool
	}{{"stamped true", &yes}, {"stamped false", &no}, {"nil", nil}} {
		wf, args := workflowFor(store.Schedule{OrgID: "o", TargetKind: store.TargetKindChangeMonitor, TargetRef: "https://x", Zdr: tc.in})
		if wf != "ChangeMonitorWF" || len(args) != 1 {
			t.Fatalf("%s: wf=%q args=%v", tc.name, wf, args)
		}
		a, ok := args[0].(changeMonitorArgs)
		if !ok {
			t.Fatalf("%s: args[0] type %T", tc.name, args[0])
		}
		if (a.Zdr == nil) != (tc.in == nil) || (tc.in != nil && *a.Zdr != *tc.in) {
			t.Fatalf("%s: args zdr=%v want %v", tc.name, a.Zdr, tc.in)
		}
	}
}

// The stamp must also survive the list view the orchestrator reconciler reads.
func TestListScheduleCarriesStampToReconciler(t *testing.T) {
	t.Parallel()
	h, _ := newSchedulesServer(t)
	if code, env := postSchedule(t, h,
		`{"org_id":"org_z","target_kind":"change_monitor","target_ref":"https://example.com/p","preset":"daily","enabled":true,"zdr":true}`); code != http.StatusCreated {
		t.Fatalf("status=%d env=%v", code, env)
	}
	req := httptest.NewRequest(http.MethodGet, "/v1/schedules", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	env := decodeEnvelope(t, w.Body.Bytes())
	list, _ := env["data"].([]any)
	if len(list) != 1 {
		t.Fatalf("list=%v", env)
	}
	row, _ := list[0].(map[string]any)
	args, _ := row["args"].([]any)
	a0, _ := args[0].(map[string]any)
	if a0["zdr"] != true {
		t.Fatalf("list args[0]=%v, want zdr true", a0)
	}
}

func TestSourceMonitorCreateStampsZdr(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSources(r, db)

	body := func(url string, zdr string) string {
		b := `{"name":"w","url":"` + url + `","kind":"scrape","monitor":true,"preset":"daily"`
		if zdr != "" {
			b += `,"zdr":` + zdr
		}
		return b + `}`
	}
	if code, _ := createSource(t, r, "org_a", body("https://a.example/zdr", "true")); code != http.StatusCreated {
		t.Fatalf("zdr monitor create code=%d", code)
	}
	if code, _ := createSource(t, r, "org_a", body("https://a.example/std", "false")); code != http.StatusCreated {
		t.Fatalf("std monitor create code=%d", code)
	}
	if code, _ := createSource(t, r, "org_a", body("https://a.example/legacy", "")); code != http.StatusCreated {
		t.Fatalf("legacy monitor create code=%d", code)
	}
	scheds, _ := db.Schedules().List(50, "")
	if len(scheds) != 3 {
		t.Fatalf("want 3 schedules, got %d", len(scheds))
	}
	byURL := map[string]*bool{}
	for _, s := range scheds {
		byURL[s.TargetRef] = s.Zdr
	}
	if z := byURL["https://a.example/zdr"]; z == nil || !*z {
		t.Fatalf("zdr source monitor stamp=%v, want true", z)
	}
	if z := byURL["https://a.example/std"]; z == nil || *z {
		t.Fatalf("standard source monitor stamp=%v, want false", z)
	}
	if z := byURL["https://a.example/legacy"]; z != nil {
		t.Fatalf("a monitor created with no posture must stay unstamped, got %v", *z)
	}
}

// A plain source (monitor:false) creates no schedule, so the posture field is
// accepted and ignored there.
func TestSourceWithoutMonitorIgnoresZdr(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSources(r, db)
	req := httptest.NewRequest(http.MethodPost, "/v1/sources?org_id=org_a",
		strings.NewReader(`{"name":"w","url":"https://a.example/plain","kind":"scrape","zdr":true}`))
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("code=%d body=%s", w.Code, w.Body.String())
	}
	if scheds, _ := db.Schedules().List(10, ""); len(scheds) != 0 {
		t.Fatalf("a plain source must create no schedule, got %d", len(scheds))
	}
}
