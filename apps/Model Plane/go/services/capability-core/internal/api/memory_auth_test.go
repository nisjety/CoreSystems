package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

type memoryRows struct {
	entries []memoryEntry
	index   int
	err     error
}

func (rows *memoryRows) Close()                        {}
func (rows *memoryRows) Err() error                    { return rows.err }
func (rows *memoryRows) CommandTag() pgconn.CommandTag { return pgconn.CommandTag{} }
func (rows *memoryRows) FieldDescriptions() []pgconn.FieldDescription {
	return nil
}
func (rows *memoryRows) Next() bool {
	if rows.index >= len(rows.entries) {
		return false
	}
	rows.index++
	return true
}
func (rows *memoryRows) Scan(dest ...any) error {
	entry := rows.entries[rows.index-1]
	*dest[0].(*string) = entry.ID
	*dest[1].(*string) = entry.OrgID
	*dest[2].(**string) = entry.SessionID
	*dest[3].(*string) = entry.Scope
	*dest[4].(*string) = entry.Key
	*dest[5].(*string) = entry.Content
	*dest[6].(*string) = entry.Kind
	*dest[7].(*float64) = entry.Confidence
	*dest[8].(*string) = entry.Owner
	*dest[9].(*[]string) = append([]string(nil), entry.SourceLinks...)
	*dest[10].(*string) = entry.ReviewState
	*dest[11].(*string) = entry.Classification
	*dest[12].(**time.Time) = entry.ExpiresAt
	*dest[13].(*time.Time) = entry.CreatedAt
	*dest[14].(*time.Time) = entry.UpdatedAt
	return nil
}
func (rows *memoryRows) Values() ([]any, error) { return nil, nil }
func (rows *memoryRows) RawValues() [][]byte    { return nil }
func (rows *memoryRows) Conn() *pgx.Conn        { return nil }

func memoryAuthenticatedHandler(t *testing.T, database registryDatabase) (http.Handler, string, string) {
	t.Helper()
	memory := NewMemoryHandler(nil)
	memory.pool = database
	mux := http.NewServeMux()
	memory.Register(mux)
	return mcpAuthenticatedHandler(t, mux)
}

func TestMemoryHandlerFailsClosedWithoutVerifiedIdentityContext(t *testing.T) {
	database := &recordingDatabase{}
	memory := NewMemoryHandler(nil)
	memory.pool = database
	mux := http.NewServeMux()
	memory.Register(mux)
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "/api/v1/memory", nil))

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, body=%s, want 401", response.Code, response.Body.String())
	}
	if len(database.queries) != 0 {
		t.Fatal("request without verified identity reached durable storage")
	}
}

func assertPrivateMemoryQueryPinned(t *testing.T, call databaseCall, actorIndex int) {
	t.Helper()
	query := strings.Join(strings.Fields(call.query), " ")
	for _, fragment := range []string{
		"scope IN ('org','global')",
		"scope IN ('run','thread','workspace','session','user')",
		"owner <> ''",
	} {
		if !strings.Contains(query, fragment) {
			t.Fatalf("memory query %q does not contain %q", query, fragment)
		}
	}
	if actorIndex >= len(call.args) || call.args[actorIndex] != "user-a" {
		t.Fatalf("memory query args = %v, want verified actor user-a at index %d", call.args, actorIndex)
	}
}

func TestMemoryReadsPinPrivateScopesToVerifiedActor(t *testing.T) {
	database := &recordingDatabase{}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)

	t.Run("same tenant cross user query is denied before storage", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory?user_id=user-b", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusForbidden {
			t.Fatalf("status = %d, want 403", response.Code)
		}
		if len(database.queries) != 0 {
			t.Fatal("cross-user request reached durable storage")
		}
	})

	t.Run("owned and shared list reaches storage with actor predicate", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.queries[len(database.queries)-1]
		if call.args[0] != "org-a" {
			t.Fatalf("list args = %v, want verified org first", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, 1)
	})

	t.Run("resolve reaches storage with actor predicate", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.queries[len(database.queries)-1]
		if len(call.args) != 4 || call.args[0] != "org-a" || call.args[2] != "" || call.args[3] != 201 {
			t.Fatalf("resolve args = %v, want verified org, actor, empty key, and bound", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, 1)
	})

	t.Run("session filter remains owner scoped", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory?session_id=session-a&limit=25", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.queries[len(database.queries)-1]
		if len(call.args) != 4 || call.args[2] != "session-a" || call.args[3] != 25 {
			t.Fatalf("session list args = %v", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, 1)
	})

	t.Run("invalid scope is rejected before storage", func(t *testing.T) {
		before := len(database.queries)
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory?scope=legacy-private", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusBadRequest {
			t.Fatalf("status = %d, body=%s, want 400", response.Code, response.Body.String())
		}
		if len(database.queries) != before {
			t.Fatal("invalid scope reached durable storage")
		}
	})

	t.Run("owned row is returned", func(t *testing.T) {
		now := time.Now().UTC()
		database.rows = &memoryRows{entries: []memoryEntry{{
			ID: "owned", OrgID: "org-a", Scope: "user", Key: "preference",
			Content: "private", Owner: "user-a", CreatedAt: now, UpdatedAt: now,
		}}}
		defer func() { database.rows = nil }()
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory?scope=user", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		var body struct {
			Entries []memoryEntry `json:"entries"`
			Count   int           `json:"count"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
			t.Fatal(err)
		}
		if body.Count != 1 || len(body.Entries) != 1 || body.Entries[0].Owner != "user-a" {
			t.Fatalf("list body = %+v", body)
		}
	})

	t.Run("direct id lookup cannot escape actor predicate", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/mem-foreign", nil)
		request.Header.Set("Authorization", "Bearer "+readToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, want 404", response.Code)
		}
		call := database.queryRows[len(database.queryRows)-1]
		if len(call.args) != 3 || call.args[0] != "mem-foreign" || call.args[1] != "org-a" {
			t.Fatalf("get args = %v, want id, verified org, verified actor", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, 2)
	})
}

func TestMemoryPrivateCreatePinsOwnerAndMutationsUseVerifiedActor(t *testing.T) {
	database := &recordingDatabase{}
	handler, _, writeToken := memoryAuthenticatedHandler(t, database)

	t.Run("user scope owner is derived from signed identity", func(t *testing.T) {
		body := `{"scope":"user","key":"preference","content":"safe","owner":"user-b"}`
		request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusCreated {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.execs[len(database.execs)-1]
		if len(call.args) < 9 || call.args[1] != "org-a" || call.args[8] != "user-a" {
			t.Fatalf("insert args = %v, want verified org and actor owner", call.args)
		}
	})

	t.Run("update is actor scoped", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/memory/mem-a", strings.NewReader(`{"content":"new"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.execs[len(database.execs)-1]
		if call.args[len(call.args)-2] != "org-a" || call.args[len(call.args)-1] != "user-a" {
			t.Fatalf("update args = %v, want verified org and actor", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, len(call.args)-1)
	})

	t.Run("delete is actor scoped", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodDelete, "/api/v1/memory/mem-a", nil)
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)

		if response.Code != http.StatusNoContent {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		call := database.execs[len(database.execs)-1]
		if len(call.args) != 3 || call.args[0] != "mem-a" || call.args[1] != "org-a" || call.args[2] != "user-a" {
			t.Fatalf("delete args = %v, want id, verified org, verified actor", call.args)
		}
		assertPrivateMemoryQueryPinned(t, call, 2)
	})

	t.Run("invalid create scope is rejected", func(t *testing.T) {
		body := `{"scope":"legacy-private","key":"unsafe","content":"unsafe"}`
		request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("status = %d, body=%s, want 400", response.Code, response.Body.String())
		}
	})

	for _, scope := range []string{"run", "thread", "workspace", "session"} {
		t.Run(scope+" write is quarantined without ownership contract", func(t *testing.T) {
			before := len(database.execs)
			body := `{"scope":"` + scope + `","key":"unsafe","content":"unsafe"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
			}
			if len(database.execs) != before {
				t.Fatal("unverified resource-scoped write reached durable storage")
			}
		})
	}

	t.Run("caller supplied session binding is quarantined for user scope", func(t *testing.T) {
		before := len(database.execs)
		body := `{"scope":"user","session_id":"session-b","key":"unsafe","content":"unsafe"}`
		request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
		}
		if len(database.execs) != before {
			t.Fatal("unverified session binding reached durable storage")
		}
	})
}

func TestMemoryForeignPrivateMutationsFailClosed(t *testing.T) {
	database := &recordingDatabase{execTag: "UPDATE 0"}
	handler, _, writeToken := memoryAuthenticatedHandler(t, database)

	for _, test := range []struct {
		name   string
		method string
		body   string
	}{
		{name: "update", method: http.MethodPatch, body: `{"content":"new"}`},
		{name: "delete", method: http.MethodDelete},
	} {
		t.Run(test.name, func(t *testing.T) {
			request := httptest.NewRequest(test.method, "/api/v1/memory/mem-foreign", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
			}
		})
	}
}

func TestMemoryMutationFailuresAreExplicit(t *testing.T) {
	t.Run("unsupported item method", func(t *testing.T) {
		database := &recordingDatabase{}
		handler, _, writeToken := memoryAuthenticatedHandler(t, database)
		request := httptest.NewRequest(http.MethodPut, "/api/v1/memory/mem-a", strings.NewReader(`{}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
		}
	})

	t.Run("empty patch", func(t *testing.T) {
		database := &recordingDatabase{}
		handler, _, writeToken := memoryAuthenticatedHandler(t, database)
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/memory/mem-a", strings.NewReader(`{}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("status = %d, body=%s, want 400", response.Code, response.Body.String())
		}
	})

	t.Run("review update reaches actor-scoped storage", func(t *testing.T) {
		database := &recordingDatabase{}
		handler, _, writeToken := memoryAuthenticatedHandler(t, database)
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/memory/mem-a", strings.NewReader(`{"review_state":"accepted"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		assertPrivateMemoryQueryPinned(t, database.execs[0], len(database.execs[0].args)-1)
	})

	t.Run("multi-field patch is one atomic statement", func(t *testing.T) {
		database := &recordingDatabase{}
		handler, _, writeToken := memoryAuthenticatedHandler(t, database)
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/memory/mem-a", strings.NewReader(`{"content":"new","review_state":"accepted"}`))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		if len(database.execs) != 1 {
			t.Fatalf("exec count = %d, want one atomic update", len(database.execs))
		}
		query := strings.Join(strings.Fields(database.execs[0].query), " ")
		if !strings.Contains(query, "content=CASE") || !strings.Contains(query, "review_state=CASE") {
			t.Fatalf("atomic update query = %q", query)
		}
	})

	for _, test := range []struct {
		name   string
		method string
		body   string
	}{
		{name: "update store unavailable", method: http.MethodPatch, body: `{"content":"new"}`},
		{name: "delete store unavailable", method: http.MethodDelete},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execErr: errors.New("database unavailable")}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database)
			request := httptest.NewRequest(test.method, "/api/v1/memory/mem-a", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
			}
		})
	}
}

func TestMemoryResolveAppliesPrivatePrecedence(t *testing.T) {
	now := time.Now().UTC()
	sessionID := "session-a"
	foreignSessionID := "session-b"
	database := &recordingDatabase{rows: &memoryRows{entries: []memoryEntry{
		{ID: "org", OrgID: "org-a", Scope: "org", Key: "preference", Content: "shared", Owner: "user-z", CreatedAt: now, UpdatedAt: now},
		{ID: "user", OrgID: "org-a", Scope: "user", Key: "preference", Content: "owned", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "session", OrgID: "org-a", SessionID: &sessionID, Scope: "session", Key: "session-key", Content: "private", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "other-session", OrgID: "org-a", SessionID: &foreignSessionID, Scope: "session", Key: "other-session", Content: "must-not-mix", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
	}}}
	handler, readToken, writeToken := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?session_id=session-a", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	var body struct {
		Entries []memoryEntry `json:"entries"`
		Count   int           `json:"count"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body.Count != 2 || len(body.Entries) != 2 || body.Entries[0].Content != "private" || body.Entries[1].Content != "owned" {
		t.Fatalf("resolved body = %+v, want session then owned user override", body)
	}
	call := database.queries[0]
	if len(call.args) != 5 || call.args[2] != "session-a" || call.args[3] != "" || call.args[4] != 201 || !strings.Contains(call.query, "session_id=$3") {
		t.Fatalf("session resolve query = %q args=%v", call.query, call.args)
	}

	methodRequest := httptest.NewRequest(http.MethodPost, "/api/v1/memory/resolve", nil)
	methodRequest.Header.Set("Authorization", "Bearer "+writeToken)
	methodResponse := httptest.NewRecorder()
	handler.ServeHTTP(methodResponse, methodRequest)
	if methodResponse.Code != http.StatusMethodNotAllowed {
		t.Fatalf("method status = %d, want 405", methodResponse.Code)
	}
}

func TestMemoryResolveDoesNotMixUnselectedPrivateResources(t *testing.T) {
	now := time.Now().UTC()
	sessionID := "session-a"
	database := &recordingDatabase{rows: &memoryRows{entries: []memoryEntry{
		{ID: "session", OrgID: "org-a", SessionID: &sessionID, Scope: "session", Key: "private", Content: "private", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "user", OrgID: "org-a", Scope: "user", Key: "preference", Content: "owned", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "legacy", OrgID: "org-a", Scope: "legacy-private", Key: "legacy", Content: "must-not-leak", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
	}}}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	var body struct {
		Entries []memoryEntry `json:"entries"`
		Count   int           `json:"count"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body.Count != 1 || len(body.Entries) != 1 || body.Entries[0].Scope != "user" {
		t.Fatalf("unselected private resources leaked into resolve: %+v", body)
	}
}

func TestMemoryResolveRejectsUnsupportedResourceSelectors(t *testing.T) {
	database := &recordingDatabase{}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?run_id=run-a", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusNotImplemented {
		t.Fatalf("status = %d, body=%s, want 501", response.Code, response.Body.String())
	}
	if len(database.queries) != 0 {
		t.Fatal("unsupported resource selector reached durable storage")
	}
}

func TestMemoryResolveBoundsDurableRows(t *testing.T) {
	now := time.Now().UTC()
	entries := make([]memoryEntry, 201)
	for index := range entries {
		entries[index] = memoryEntry{
			ID: "owned", OrgID: "org-a", Scope: "user", Key: "preference",
			Content: "private", Owner: "user-a", CreatedAt: now, UpdatedAt: now,
		}
	}
	database := &recordingDatabase{rows: &memoryRows{entries: entries}}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?key=preference", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, body=%s, want 422", response.Code, response.Body.String())
	}
	call := database.queries[0]
	query := strings.Join(strings.Fields(call.query), " ")
	if !strings.Contains(query, "key=$3") || !strings.Contains(query, "LIMIT $4") {
		t.Fatalf("bounded resolve query = %q", query)
	}
	if len(call.args) != 4 || call.args[2] != "preference" || call.args[3] != 201 {
		t.Fatalf("bounded resolve args = %v", call.args)
	}
}

func TestMemoryValidOwnerFailsHonestlyWhenDurableStoreUnavailable(t *testing.T) {
	database := &recordingDatabase{queryErr: errors.New("database unavailable")}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory?scope=user", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
	}
	if len(database.queries) != 1 {
		t.Fatalf("valid owner query count = %d, want 1", len(database.queries))
	}
	assertPrivateMemoryQueryPinned(t, database.queries[0], 1)

	t.Run("direct lookup", func(t *testing.T) {
		directDatabase := &recordingDatabase{rowErr: errors.New("database unavailable")}
		directHandler, directToken, _ := memoryAuthenticatedHandler(t, directDatabase)
		directRequest := httptest.NewRequest(http.MethodGet, "/api/v1/memory/mem-a", nil)
		directRequest.Header.Set("Authorization", "Bearer "+directToken)
		directResponse := httptest.NewRecorder()
		directHandler.ServeHTTP(directResponse, directRequest)
		if directResponse.Code != http.StatusServiceUnavailable {
			t.Fatalf("status = %d, body=%s, want 503", directResponse.Code, directResponse.Body.String())
		}
	})
}
