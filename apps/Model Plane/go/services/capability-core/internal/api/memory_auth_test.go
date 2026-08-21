package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// fakeRunServiceClient is a scripted mpv1.RunServiceClient double for
// MEM-2's resource-ownership authorization tests. It embeds the interface
// itself (nil) so every method other than the two overridden below panics if
// a test ever calls it by mistake, rather than silently returning a zero
// value.
type fakeRunServiceClient struct {
	mpv1.RunServiceClient
	calls      int
	authorized bool
	err        error
}

func (f *fakeRunServiceClient) ResolveRunOwner(_ context.Context, _ *mpv1.ResolveRunOwnerRequest, _ ...grpc.CallOption) (*mpv1.ResolveRunOwnerResponse, error) {
	f.calls++
	if f.err != nil {
		return nil, f.err
	}
	return &mpv1.ResolveRunOwnerResponse{Authorized: f.authorized}, nil
}

func (f *fakeRunServiceClient) ResolveThreadOwner(_ context.Context, _ *mpv1.ResolveThreadOwnerRequest, _ ...grpc.CallOption) (*mpv1.ResolveThreadOwnerResponse, error) {
	f.calls++
	if f.err != nil {
		return nil, f.err
	}
	return &mpv1.ResolveThreadOwnerResponse{Authorized: f.authorized}, nil
}

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

// memoryAuthenticatedHandler wires a MemoryHandler over the given database
// fake. The optional trailing argument supplies a RunServiceClient double for
// the run/thread/session resource-authorization tests; every other test in
// this file omits it and gets a nil client (the "session-core dial disabled"
// fail-closed path).
func memoryAuthenticatedHandler(t *testing.T, database registryDatabase, runs ...mpv1.RunServiceClient) (http.Handler, string, string) {
	t.Helper()
	var runClient mpv1.RunServiceClient
	if len(runs) > 0 {
		runClient = runs[0]
	}
	memory := NewMemoryHandler(nil, runClient)
	memory.pool = database
	mux := http.NewServeMux()
	memory.Register(mux)
	return mcpAuthenticatedHandler(t, mux)
}

func TestMemoryHandlerFailsClosedWithoutVerifiedIdentityContext(t *testing.T) {
	database := &recordingDatabase{}
	memory := NewMemoryHandler(nil, nil)
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

	// workspace is the one scope MEM-2 deliberately did NOT build an
	// authorization check for: no table, owner column, or RPC anywhere in
	// Model Plane resolves "who owns workspace X" (session-core's
	// workspace_id is a content-selection hint only; capability-core's own
	// ScopeKindWorkspace is rejected at capability-invocation time). Building
	// a check here would mean fabricating an authorization concept that
	// doesn't exist in the data model, so this stays quarantined
	// unconditionally, independent of whether a session_id is supplied or a
	// RunServiceClient is wired — unlike run/thread/session below, which now
	// have their own dedicated authorization tests in
	// TestMemoryResourceScopedWriteAuthorization.
	t.Run("workspace write is quarantined without an ownership model", func(t *testing.T) {
		before := len(database.execs)
		body := `{"scope":"workspace","key":"unsafe","content":"unsafe"}`
		request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
		}
		if len(database.execs) != before {
			t.Fatal("workspace-scoped write reached durable storage")
		}
	})

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

// TestMemoryResourceScopedWriteAuthorization covers MEM-2: run/thread/session
// scoped memory writes now call Session Core's RunService instead of being
// unconditionally quarantined (workspace, which has no ownership contract
// anywhere in Model Plane, is covered separately in
// TestMemoryPrivateCreatePinsOwnerAndMutationsUseVerifiedActor and stays
// quarantined unconditionally).
func TestMemoryResourceScopedWriteAuthorization(t *testing.T) {
	for _, scope := range []string{"run", "thread", "session"} {
		t.Run(scope+" authorized owner write succeeds", func(t *testing.T) {
			database := &recordingDatabase{}
			runs := &fakeRunServiceClient{authorized: true}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database, runs)

			body := `{"scope":"` + scope + `","session_id":"resource-a","key":"k","content":"v"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusCreated {
				t.Fatalf("status = %d, body=%s, want 201", response.Code, response.Body.String())
			}
			if runs.calls != 1 {
				t.Fatalf("session core lookup calls = %d, want 1", runs.calls)
			}
			call := database.execs[len(database.execs)-1]
			sessionIDArg, _ := call.args[2].(*string)
			if len(call.args) < 9 || sessionIDArg == nil || *sessionIDArg != "resource-a" || call.args[3] != scope {
				t.Fatalf("insert args = %v, want resource id threaded into session_id and scope=%s", call.args, scope)
			}
		})

		t.Run(scope+" unauthorized write is rejected with 403 not 503", func(t *testing.T) {
			database := &recordingDatabase{}
			runs := &fakeRunServiceClient{authorized: false}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database, runs)

			body := `{"scope":"` + scope + `","session_id":"resource-a","key":"k","content":"v"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusForbidden {
				t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
			}
			if len(database.execs) != 0 {
				t.Fatal("denied resource-scoped write reached durable storage")
			}
		})

		t.Run(scope+" missing resource id is 400 and never calls session core", func(t *testing.T) {
			database := &recordingDatabase{}
			runs := &fakeRunServiceClient{authorized: true}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database, runs)

			body := `{"scope":"` + scope + `","key":"k","content":"v"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, body=%s, want 400", response.Code, response.Body.String())
			}
			if runs.calls != 0 {
				t.Fatal("missing resource id must not reach session core")
			}
			if len(database.execs) != 0 {
				t.Fatal("missing resource id reached durable storage")
			}
		})

		t.Run(scope+" session core unreachable fails closed as 503 not 403", func(t *testing.T) {
			database := &recordingDatabase{}
			runs := &fakeRunServiceClient{err: errors.New("transport error")}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database, runs)

			body := `{"scope":"` + scope + `","session_id":"resource-a","key":"k","content":"v"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, body=%s, want 503 (unreachable is not the same as denied)", response.Code, response.Body.String())
			}
			if len(database.execs) != 0 {
				t.Fatal("write reached durable storage despite an unreachable authority")
			}
		})

		t.Run(scope+" nil session core client fails closed as 503", func(t *testing.T) {
			// A nil RunServiceClient means the session-core dial is disabled
			// or failed at startup — an availability problem, not a "no"
			// answer, so this is deliberately 503 (resourceAuthUnavailable),
			// not 403, matching the "unreachable" case above rather than the
			// "denied" case.
			database := &recordingDatabase{}
			handler, _, writeToken := memoryAuthenticatedHandler(t, database)

			body := `{"scope":"` + scope + `","session_id":"resource-a","key":"k","content":"v"}`
			request := httptest.NewRequest(http.MethodPost, "/api/v1/memory", strings.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, body=%s, want 503", response.Code, response.Body.String())
			}
			if len(database.execs) != 0 {
				t.Fatal("write reached durable storage with no session core client configured")
			}
		})
	}
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

// TestMemoryResolveRejectsWorkspaceSelector: workspace is deferred, not
// merely "not yet implemented across the board" — MEM-2 gave run_id and
// thread_id real support (see TestMemoryResolveAppliesRunAndThreadPrecedence
// below); workspace_id alone still 501s because no ownership model exists
// for it anywhere in Model Plane.
func TestMemoryResolveRejectsWorkspaceSelector(t *testing.T) {
	database := &recordingDatabase{}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?workspace_id=workspace-a", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusNotImplemented {
		t.Fatalf("status = %d, body=%s, want 501", response.Code, response.Body.String())
	}
	if len(database.queries) != 0 {
		t.Fatal("workspace resource selector reached durable storage")
	}
}

// TestMemoryResolveAppliesRunAndThreadPrecedence: MEM-2 unblocked run_id and
// thread_id resolution. Combined with session_id, the merge picks the
// narrowest scope first: run > thread > session > user > org > global.
func TestMemoryResolveAppliesRunAndThreadPrecedence(t *testing.T) {
	now := time.Now().UTC()
	runID := "run-a"
	threadID := "thread-a"
	sessionID := "session-a"
	database := &recordingDatabase{rows: &memoryRows{entries: []memoryEntry{
		{ID: "org", OrgID: "org-a", Scope: "org", Key: "preference", Content: "shared", Owner: "user-z", CreatedAt: now, UpdatedAt: now},
		{ID: "user", OrgID: "org-a", Scope: "user", Key: "preference", Content: "owned", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "session", OrgID: "org-a", SessionID: &sessionID, Scope: "session", Key: "preference", Content: "session-level", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "thread", OrgID: "org-a", SessionID: &threadID, Scope: "thread", Key: "preference", Content: "thread-level", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "run", OrgID: "org-a", SessionID: &runID, Scope: "run", Key: "preference", Content: "run-level", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
	}}}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?run_id=run-a&thread_id=thread-a&session_id=session-a", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	var body struct {
		Entries    []memoryEntry `json:"entries"`
		Count      int           `json:"count"`
		Precedence []string      `json:"precedence"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	wantPrecedence := []string{"run", "thread", "session", "user", "org", "global"}
	if len(body.Precedence) != len(wantPrecedence) {
		t.Fatalf("precedence = %v, want %v", body.Precedence, wantPrecedence)
	}
	for i, scope := range wantPrecedence {
		if body.Precedence[i] != scope {
			t.Fatalf("precedence = %v, want %v", body.Precedence, wantPrecedence)
		}
	}
	// All entries share the key "preference"; the narrowest scope (run) must
	// win the merge over thread/session/user/org.
	if body.Count != 1 || len(body.Entries) != 1 || body.Entries[0].Content != "run-level" {
		t.Fatalf("resolved body = %+v, want only the run-scoped entry to win", body)
	}
}

// TestMemoryResolveDoesNotMixUnselectedRunOrThreadResources mirrors
// TestMemoryResolveDoesNotMixUnselectedPrivateResources for the two new
// selectors: a run/thread row must not leak into a resolve call that never
// asked for that run_id/thread_id.
func TestMemoryResolveDoesNotMixUnselectedRunOrThreadResources(t *testing.T) {
	now := time.Now().UTC()
	runID := "run-a"
	threadID := "thread-a"
	database := &recordingDatabase{rows: &memoryRows{entries: []memoryEntry{
		{ID: "run", OrgID: "org-a", SessionID: &runID, Scope: "run", Key: "preference", Content: "must-not-leak", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "thread", OrgID: "org-a", SessionID: &threadID, Scope: "thread", Key: "preference", Content: "must-not-leak", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
		{ID: "user", OrgID: "org-a", Scope: "user", Key: "preference", Content: "owned", Owner: "user-a", CreatedAt: now, UpdatedAt: now},
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
		t.Fatalf("unselected run/thread resources leaked into resolve: %+v", body)
	}
}

// TestMemoryResolveRunAndThreadSelectorsRemainOwnerPinned is the regression
// check for MEM-2 §0(b)'s design decision: reads never call Session Core
// again on the resolve path — memoryVisibilitySQL's owner=actor filter,
// already proven at write time (authorizeResourceOwner), is what keeps a
// run/thread row private. This asserts the generated SQL still ANDs that
// filter in for the new run_id/thread_id clauses; get this wrong and a
// stale-owner memory row becomes readable by whoever currently owns the run
// or thread.
func TestMemoryResolveRunAndThreadSelectorsRemainOwnerPinned(t *testing.T) {
	database := &recordingDatabase{}
	handler, readToken, _ := memoryAuthenticatedHandler(t, database)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/memory/resolve?run_id=run-a&thread_id=thread-a", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	call := database.queries[0]
	assertPrivateMemoryQueryPinned(t, call, 1)
	if !strings.Contains(call.query, "scope='run' AND session_id=$") {
		t.Fatalf("resolve query = %q, want a run clause", call.query)
	}
	if !strings.Contains(call.query, "scope='thread' AND session_id=$") {
		t.Fatalf("resolve query = %q, want a thread clause", call.query)
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
