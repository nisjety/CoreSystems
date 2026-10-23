package api

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

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
