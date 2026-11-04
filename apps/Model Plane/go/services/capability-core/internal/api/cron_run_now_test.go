package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

const runNowTestRequestID = "6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10"

// runNowStubDB is the transaction pool a Run now request opens. Its one transaction
// answers a schedule created by creator, or no schedule when creator is empty, and
// accepts every write.
type runNowStubDB struct{ creator string }

func (db runNowStubDB) Begin(context.Context) (pgx.Tx, error) { return &runNowStubTx{creator: db.creator}, nil }

type runNowStubTx struct {
	pgx.Tx
	creator string
}

func (tx *runNowStubTx) Commit(context.Context) error { return nil }

func (tx *runNowStubTx) Rollback(context.Context) error { return nil }

func (tx *runNowStubTx) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}

func (tx *runNowStubTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	switch {
	case strings.Contains(sql, "FROM cron_schedules"):
		return runNowStubRow(func(dest ...any) error {
			if tx.creator == "" {
				return pgx.ErrNoRows
			}
			for i, value := range []string{"sched-1", "org-1", "space-1", tx.creator, "sha256:" + strings.Repeat("a", 64), "0 9 * * *", "UTC"} {
				*dest[i].(*string) = value
			}
			*dest[7].(*json.RawMessage) = json.RawMessage(`{"kind":"cron","title":"Nightly"}`)
			return nil
		})
	case strings.Contains(sql, "count(*)"):
		return runNowStubRow(func(dest ...any) error {
			*dest[0].(*int), *dest[1].(*int) = 0, 0
			return nil
		})
	default:
		return runNowStubRow(func(...any) error { return pgx.ErrNoRows })
	}
}

type runNowStubRow func(dest ...any) error

func (row runNowStubRow) Scan(dest ...any) error { return row(dest...) }

type allowingFireAuthorizer struct{ calls int }

func (a *allowingFireAuthorizer) AuthorizeFire(context.Context, cron.FireIntent) error {
	a.calls++
	return nil
}

func runNowRequest(target, body, actor string) *http.Request {
	request := httptest.NewRequest(http.MethodPost, target, strings.NewReader(body))
	return request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: "org-1", ActorID: actor, PrincipalType: "user",
	}))
}

func TestCronRunRoute404WhenOff(t *testing.T) {
	body := `{"request_id":"` + runNowTestRequestID + `"}`
	for name, handler := range map[string]*CronHandler{
		"switch off":       NewCronHandler(unreachableDatabase{t}).WithRunNow(runNowStubDB{creator: "user-1"}, &allowingFireAuthorizer{}, false),
		"no authorizer":    NewCronHandler(unreachableDatabase{t}).WithRunNow(runNowStubDB{creator: "user-1"}, nil, true),
		"never configured": NewCronHandler(unreachableDatabase{t}),
	} {
		mux := http.NewServeMux()
		handler.Register(mux)
		recorder := httptest.NewRecorder()
		mux.ServeHTTP(recorder, runNowRequest("/api/v1/cron/sched-1/run", body, "user-1"))
		if recorder.Code != http.StatusNotFound {
			t.Fatalf("%s: status %d, want 404", name, recorder.Code)
		}
	}
}

func TestCronRunRouteTypedErrors(t *testing.T) {
	cases := []struct {
		name     string
		creator  string
		actor    string
		body     string
		wantCode int
		wantErr  string
	}{
		{name: "a member who did not create the schedule", creator: "user-2", actor: "user-1",
			body: `{"request_id":"` + runNowTestRequestID + `"}`, wantCode: http.StatusForbidden, wantErr: "run_now_not_creator"},
		{name: "a schedule outside the org", creator: "", actor: "user-1",
			body: `{"request_id":"` + runNowTestRequestID + `"}`, wantCode: http.StatusNotFound, wantErr: "not_found"},
		{name: "a request id that is not one", creator: "user-1", actor: "user-1",
			body: `{"request_id":"x"}`, wantCode: http.StatusBadRequest, wantErr: "invalid_request_id"},
		{name: "a body that is not JSON", creator: "user-1", actor: "user-1",
			body: `request_id`, wantCode: http.StatusBadRequest, wantErr: "invalid_request"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			authorizer := &allowingFireAuthorizer{}
			mux := http.NewServeMux()
			NewCronHandler(unreachableDatabase{t}).WithRunNow(runNowStubDB{creator: tc.creator}, authorizer, true).Register(mux)
			recorder := httptest.NewRecorder()
			mux.ServeHTTP(recorder, runNowRequest("/api/v1/cron/sched-1/run", tc.body, tc.actor))
			if recorder.Code != tc.wantCode {
				t.Fatalf("status %d, want %d: %s", recorder.Code, tc.wantCode, recorder.Body.String())
			}
			var envelope struct {
				Error struct {
					Code    string         `json:"code"`
					Message string         `json:"message"`
					Details map[string]any `json:"details"`
				} `json:"error"`
			}
			if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
				t.Fatalf("decode %s: %v", recorder.Body.String(), err)
			}
			if envelope.Error.Code != tc.wantErr || envelope.Error.Message == "" || envelope.Error.Details == nil {
				t.Fatalf("error envelope = %s, want code %s with a message and an object of details", recorder.Body.String(), tc.wantErr)
			}
			if authorizer.calls != 0 {
				t.Fatalf("a refused press reached Control %d time(s)", authorizer.calls)
			}
		})
	}

	// Only POST makes a fire.
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).WithRunNow(runNowStubDB{creator: "user-1"}, &allowingFireAuthorizer{}, true).Register(mux)
	recorder := httptest.NewRecorder()
	request := runNowRequest("/api/v1/cron/sched-1/run", "", "user-1")
	request.Method = http.MethodGet
	mux.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("GET of the run route: status %d, want 404", recorder.Code)
	}
}

func TestCronRunRouteAcceptsTheCreator(t *testing.T) {
	authorizer := &allowingFireAuthorizer{}
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).WithRunNow(runNowStubDB{creator: "user-1"}, authorizer, true).Register(mux)
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, runNowRequest("/api/v1/cron/sched-1/run", `{"request_id":"`+runNowTestRequestID+`"}`, "user-1"))
	if recorder.Code != http.StatusAccepted {
		t.Fatalf("status %d, want 202: %s", recorder.Code, recorder.Body.String())
	}
	var envelope struct {
		Data cron.RunNowResult `json:"data"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
		t.Fatal(err)
	}
	if envelope.Data.FireKey != "manual:"+runNowTestRequestID || envelope.Data.Status != "pending" ||
		envelope.Data.Deduplicated || envelope.Data.FireID == "" || envelope.Data.TaskID == "" {
		t.Fatalf("data = %+v", envelope.Data)
	}
	if authorizer.calls != 1 {
		t.Fatalf("Control was asked %d time(s), want once", authorizer.calls)
	}
}

// A ZDR principal never reaches the route: the router refuses every non-read request
// from it before any handler runs, so Run now writes nothing under ZDR.
func TestCronRunRefusedForZDRPrincipal(t *testing.T) {
	request := httptest.NewRequest(http.MethodPost, "/api/v1/cron/sched-1/run", nil)
	for name, principal := range map[string]authctx.Principal{
		"zdr":                 {OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user", RetentionPolicyPresent: true, ZeroDataRetention: true, Scopes: []string{authz.WriteScope}},
		"no retention policy": {OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user", Scopes: []string{authz.WriteScope}},
	} {
		if err := authz.AuthorizeHTTP(principal, request); err == nil {
			t.Fatalf("%s: the router let a Run now through", name)
		}
	}
}

// listStubDB answers the schedule list with one row and records the arguments it was
// asked with.
type listStubDB struct {
	unreachableDatabase
	args []any
	sql  string
}

func (db *listStubDB) Query(_ context.Context, sql string, args ...any) (pgx.Rows, error) {
	db.sql, db.args = sql, args
	return &scheduleListRows{}, nil
}

type scheduleListRows struct {
	pgx.Rows
	done bool
}

func (rows *scheduleListRows) Next() bool {
	if rows.done {
		return false
	}
	rows.done = true
	return true
}

func (rows *scheduleListRows) Scan(dest ...any) error {
	now := time.Date(2026, 11, 4, 9, 0, 0, 0, time.UTC)
	for i, value := range []string{"sched-1", "org-1", "Nightly", "", "0 9 * * *", "UTC"} {
		*dest[i].(*string) = value
	}
	*dest[6].(*any) = map[string]any{"kind": "cron"}
	*dest[7].(*bool) = false
	*dest[10].(*time.Time) = now
	*dest[11].(*time.Time) = now
	*dest[12].(*string) = "space-1"
	*dest[13].(*string) = "user-1"
	*dest[14].(*string) = "paused"
	*dest[15].(*string) = "paused_failures"
	return nil
}

func (rows *scheduleListRows) Err() error { return nil }

func (rows *scheduleListRows) Close() {}

func listRequest(target string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, target, nil)
	return request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	}))
}

func TestCronListAddsLastOutcomeAndFeatures(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		db := &listStubDB{unreachableDatabase: unreachableDatabase{t}}
		handler := NewCronHandler(db).WithRunNow(runNowStubDB{creator: "user-1"}, &allowingFireAuthorizer{}, enabled)
		recorder := httptest.NewRecorder()
		handler.listOrCreate(recorder, listRequest("/api/v1/cron"))
		if recorder.Code != http.StatusOK {
			t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
		}
		var body struct {
			Schedules []map[string]any `json:"schedules"`
			Features  map[string]bool  `json:"features"`
		}
		if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
			t.Fatal(err)
		}
		if body.Features["run_now"] != enabled {
			t.Fatalf("features.run_now = %v, want %v", body.Features["run_now"], enabled)
		}
		if len(body.Schedules) != 1 || body.Schedules[0]["last_fire_status"] != "paused" ||
			body.Schedules[0]["last_outcome"] != "paused_failures" {
			t.Fatalf("schedules = %+v, want the last fire's status and outcome", body.Schedules)
		}
	}
}

func TestCronListCreatorSelfFilter(t *testing.T) {
	cases := map[string]string{
		"/api/v1/cron":                      "",
		"/api/v1/cron?creator=self":         "user-1",
		"/api/v1/cron?creator=someone-else": "",
	}
	for target, wantCreator := range cases {
		db := &listStubDB{unreachableDatabase: unreachableDatabase{t}}
		recorder := httptest.NewRecorder()
		NewCronHandler(db).listOrCreate(recorder, listRequest(target))
		if recorder.Code != http.StatusOK {
			t.Fatalf("%s: status %d", target, recorder.Code)
		}
		if len(db.args) != 3 || db.args[0] != "org-1" || db.args[2] != wantCreator {
			t.Fatalf("%s: list asked with %v, want creator %q", target, db.args, wantCreator)
		}
		if !strings.Contains(db.sql, "($3 = '' OR s.creator_subject_id = $3)") {
			t.Fatalf("%s: the creator filter is missing from the list query", target)
		}
	}
}
