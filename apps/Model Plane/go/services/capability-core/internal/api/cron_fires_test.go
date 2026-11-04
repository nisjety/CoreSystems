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
)

// unreachableDatabase fails the test if a request that must be refused before
// its query reaches the database.
type unreachableDatabase struct{ t *testing.T }

func (d unreachableDatabase) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Fatal("database reached before the request was refused")
	return pgconn.CommandTag{}, nil
}

func (d unreachableDatabase) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Fatal("database reached before the request was refused")
	return nil, nil
}

func (d unreachableDatabase) QueryRow(context.Context, string, ...any) pgx.Row {
	d.t.Fatal("database reached before the request was refused")
	return nil
}

func TestCronFiresRefusesBadLimitBeforeTouchingTheDatabase(t *testing.T) {
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).Register(mux)
	for _, target := range []string{
		"/api/v1/cron/sched-1/fires?limit=0",
		"/api/v1/cron/sched-1/fires?limit=101",
		"/api/v1/cron/sched-1/fires?limit=many",
	} {
		rec := httptest.NewRecorder()
		mux.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, target, nil))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: status %d, want %d", target, rec.Code, http.StatusBadRequest)
		}
	}
}

func TestCronFiresRouteIsReadOnly(t *testing.T) {
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).Register(mux)
	rec := httptest.NewRecorder()
	mux.ServeHTTP(rec, httptest.NewRequest(http.MethodPost, "/api/v1/cron/sched-1/fires", nil))
	if rec.Code != http.StatusNotFound {
		t.Fatalf("POST to the fires listing: status %d, want %d", rec.Code, http.StatusNotFound)
	}
}

// firesStubDB answers the owner check with a schedule that exists, and the fires query
// with two fires: a Run now and a schedule's own. The listing must carry each fire's key.
type firesStubDB struct {
	unreachableDatabase
	sql string
}

func (db *firesStubDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return runNowStubRow(func(dest ...any) error {
		*dest[0].(*bool) = true
		return nil
	})
}

func (db *firesStubDB) Query(_ context.Context, sql string, _ ...any) (pgx.Rows, error) {
	db.sql = sql
	return &firesStubRows{keys: []string{"manual:6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10", "2026-11-04T09:00:00Z"}}, nil
}

// firesStubRows yields one fire per key, in the order the listing's query returns them.
type firesStubRows struct {
	pgx.Rows
	keys []string
	next int
}

func (rows *firesStubRows) Next() bool {
	rows.next++
	return rows.next <= len(rows.keys)
}

func (rows *firesStubRows) Scan(dest ...any) error {
	*dest[0].(*string) = "fire"
	*dest[1].(*time.Time) = time.Date(2026, 11, 4, 9, 0, 0, 0, time.UTC)
	*dest[2].(*string) = "completed"
	*dest[3].(*string) = ""
	*dest[4].(**time.Time) = nil
	*dest[5].(*string) = ""
	*dest[6].(*string) = ""
	*dest[7].(*int) = 0
	*dest[8].(*string) = rows.keys[rows.next-1]
	return nil
}

func (rows *firesStubRows) Err() error { return nil }

func (rows *firesStubRows) Close() {}

func TestListFiresIncludesFireKey(t *testing.T) {
	db := &firesStubDB{unreachableDatabase: unreachableDatabase{t}}
	mux := http.NewServeMux()
	NewCronHandler(db).Register(mux)
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, listRequest("/api/v1/cron/sched-1/fires?limit=5"))
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if !strings.Contains(db.sql, "fire_key") {
		t.Fatalf("the fires query does not read fire_key: %s", db.sql)
	}
	var body struct {
		Data []struct {
			FireKey string `json:"fire_key"`
		} `json:"data"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	want := []string{"manual:6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10", "2026-11-04T09:00:00Z"}
	if len(body.Data) != len(want) {
		t.Fatalf("fires = %s, want %d fires", recorder.Body.String(), len(want))
	}
	for i, fire := range body.Data {
		if fire.FireKey != want[i] {
			t.Fatalf("fire %d fire_key = %q, want %q", i, fire.FireKey, want[i])
		}
	}
}
