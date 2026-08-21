package api

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
)

func TestRunIDFromWatchersPath(t *testing.T) {
	for _, test := range []struct {
		path   string
		wantID string
		wantOK bool
	}{
		{"/api/v1/runs/run-1/watchers", "run-1", true},
		{"/api/v1/runs/run_with-mixed.chars/watchers", "run_with-mixed.chars", true},
		{"/api/v1/runs//watchers", "", false},          // empty run id
		{"/api/v1/runs/run-1/other", "", false},        // wrong suffix
		{"/api/v1/runs/run-1", "", false},              // missing suffix entirely
		{"/api/v1/runs/run-1/sub/watchers", "", false}, // extra path segment
		{"/api/v1/other/run-1/watchers", "", false},    // wrong prefix
	} {
		gotID, gotOK := runIDFromWatchersPath(test.path)
		if gotID != test.wantID || gotOK != test.wantOK {
			t.Errorf("runIDFromWatchersPath(%q) = (%q, %v), want (%q, %v)", test.path, gotID, gotOK, test.wantID, test.wantOK)
		}
	}
}

func TestRunWatchers_CreateBindsOwnerToVerifiedPrincipal(t *testing.T) {
	database := &recordingDatabase{}
	database.expectUpsertReturningID(t, "pending")
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPost, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 1 {
		t.Fatalf("queryRows = %d, want exactly 1 (the upsert)", len(database.queryRows))
	}
	call := database.queryRows[0]
	if !strings.Contains(call.query, "INSERT INTO run_watch_subscriptions") {
		t.Fatalf("query = %q, want the upsert insert", call.query)
	}
	// args: id, org_id, run_id, user_id, now
	if len(call.args) != 5 || call.args[1] != "org-a" || call.args[2] != "run-1" || call.args[3] != "user-a" {
		t.Fatalf("insert args = %#v, want org-a/run-1/user-a bound from the verified principal", call.args)
	}
	if !strings.Contains(response.Body.String(), `"watching":true`) {
		t.Fatalf("response = %s, want watching:true", response.Body.String())
	}
}

func TestRunWatchers_CreateRejectsUnauthenticatedDirectCall(t *testing.T) {
	database := &recordingDatabase{}
	handler := NewRunWatchersHandler(database)
	request := httptest.NewRequest(http.MethodPost, "/api/v1/runs/run-1/watchers", nil)
	response := httptest.NewRecorder()
	handler.create(response, request, "run-1")

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", response.Code)
	}
	if database.databaseCalls() != 0 {
		t.Fatal("unauthenticated create must never reach the database")
	}
}

func TestRunWatchers_CreateIsIdempotentOnConflict(t *testing.T) {
	database := &recordingDatabase{}
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, _, writeToken := mcpAuthenticatedHandler(t, mux)

	// First QueryRow (the INSERT ... ON CONFLICT DO NOTHING RETURNING status)
	// answers pgx.ErrNoRows — simulating an already-active watch, exactly
	// what Postgres itself returns from DO NOTHING + RETURNING on a
	// conflict — so the handler must fall back to a plain SELECT (the
	// second queued answer) and report the row's real, already-fired
	// status rather than assuming 'pending'.
	database.nextRows = []pgx.Row{errorRow{err: pgx.ErrNoRows}, stringRow{value: "notified"}}

	request := httptest.NewRequest(http.MethodPost, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"status":"notified"`) {
		t.Fatalf("response = %s, want the existing row's real status (notified), not an assumed pending", response.Body.String())
	}
	if len(database.queryRows) != 2 {
		t.Fatalf("queryRows = %d, want 2 (insert attempt + fallback select)", len(database.queryRows))
	}
}

func TestRunWatchers_GetReturnsOnlyCallersOwnWatch(t *testing.T) {
	// A fresh recordingDatabase with no queued row answers a QueryRow with
	// pgx.ErrNoRows by default (see its QueryRow method) — exactly the "no
	// active watch" case this test wants.
	database := &recordingDatabase{}
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, readToken, _ := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodGet, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"watching":false`) {
		t.Fatalf("response = %s, want watching:false when no row exists", response.Body.String())
	}
	call := database.queryRows[len(database.queryRows)-1]
	if len(call.args) != 3 || call.args[0] != "org-a" || call.args[2] != "user-a" {
		t.Fatalf("lookup args = %#v, want scoped to the verified org-a/user-a — a GET must never accept a caller-chosen user", call.args)
	}
}

func TestRunWatchers_GetReportsExistingWatch(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, stringRow{value: "pending"})
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, readToken, _ := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodGet, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"watching":true`) || !strings.Contains(response.Body.String(), `"status":"pending"`) {
		t.Fatalf("response = %s, want watching:true status:pending", response.Body.String())
	}
}

func TestRunWatchers_DeleteOnlyAffectsCallersOwnRow(t *testing.T) {
	database := &recordingDatabase{}
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodDelete, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, body=%s, want 204", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 {
		t.Fatalf("execs = %d, want exactly 1", len(database.execs))
	}
	call := database.execs[0]
	if !strings.Contains(call.query, "org_id=$2") || !strings.Contains(call.query, "user_id=$4") {
		t.Fatalf("delete query = %q, want scoped by both org_id and user_id", call.query)
	}
	if len(call.args) != 4 || call.args[1] != "org-a" || call.args[2] != "run-1" || call.args[3] != "user-a" {
		t.Fatalf("delete args = %#v, want scoped to org-a/run-1/user-a from the verified principal, never a caller-supplied user", call.args)
	}
}

func TestRunWatchers_DeleteWithNoMatchingRowReturnsNotFound(t *testing.T) {
	database := &recordingDatabase{}
	database.execTag = "UPDATE 0"
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	authenticated, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodDelete, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 when the caller has no active watch on this run", response.Code)
	}
}

func TestRunWatchers_UnsupportedMethod(t *testing.T) {
	database := &recordingDatabase{}
	handler := NewRunWatchersHandler(database)
	mux := http.NewServeMux()
	handler.Register(mux)
	// PUT is a mutating method under authz.AuthorizeHTTP, so it needs the
	// write-scoped token to reach this handler's own method switch at all —
	// the read token would 403 at the authorization layer first.
	authenticated, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPut, "/api/v1/runs/run-1/watchers", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	authenticated.ServeHTTP(response, request)

	if response.Code != http.StatusMethodNotAllowed {
		t.Fatalf("status = %d, want 405", response.Code)
	}
}
