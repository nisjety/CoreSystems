package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// The /api/v1/memory proxy writes agent_memory directly, so it is the one
// memory path that does not pass through Session Core's own gate. These tests
// pin that it asks Session Core for the decision on every verb that reads or
// writes, refuses with 403 memory_not_enabled when the answer is no, and fails
// closed (never open) when the answer cannot be had.

var (
	memoryDecisionOn      = memoryDecision{UseAllowed: true, GenerationAllowed: true, Reason: "on"}
	memoryDecisionPaused  = memoryDecision{Reason: "user_paused"}
	memoryDecisionOff     = memoryDecision{Reason: "user_off"}
	memoryDecisionOrgOff  = memoryDecision{Reason: "org_disabled"}
	memoryDecisionZDR     = memoryDecision{Reason: "zdr"}
	memoryDecisionUnknown = memoryDecision{Reason: "policy_unavailable"}
)

type fakeMemoryControls struct {
	decision memoryDecision
	err      error
	calls    int
	orgID    string
	userID   string
}

func (f *fakeMemoryControls) Decision(_ context.Context, orgID, userID string) (memoryDecision, error) {
	f.calls++
	f.orgID, f.userID = orgID, userID
	return f.decision, f.err
}

func memoryGateHandler(t *testing.T, controls memoryControlsReader) (http.Handler, *recordingDatabase, string, string) {
	t.Helper()
	database := &recordingDatabase{}
	memory := NewMemoryHandler(nil, nil)
	memory.pool = database
	memory.controls = controls
	mux := http.NewServeMux()
	memory.Register(mux)
	handler, readToken, writeToken := mcpAuthenticatedHandler(t, mux)
	return handler, database, readToken, writeToken
}

func memoryGateDo(handler http.Handler, method, path, token, body string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func assertMemoryNotEnabled(t *testing.T, response *httptest.ResponseRecorder, reason string) {
	t.Helper()
	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
	}
	var body map[string]string
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["error"] != "memory_not_enabled" || body["reason"] != reason {
		t.Fatalf("body = %v, want memory_not_enabled with reason %q", body, reason)
	}
}

func TestMemoryDecisionReadAccess(t *testing.T) {
	for _, test := range []struct {
		name     string
		decision memoryDecision
		want     memoryReadAccess
	}{
		{"on", memoryDecisionOn, memoryReadFull},
		{"paused keeps shared rows", memoryDecisionPaused, memoryReadOrgOnly},
		{"off keeps shared rows", memoryDecisionOff, memoryReadOrgOnly},
		{"org disabled suppresses everything", memoryDecisionOrgOff, memoryReadNone},
		{"zdr suppresses everything", memoryDecisionZDR, memoryReadNone},
		{"policy unavailable suppresses everything", memoryDecisionUnknown, memoryReadNone},
		{"empty verdict suppresses everything", memoryDecision{}, memoryReadNone},
		{"unknown reason suppresses everything", memoryDecision{Reason: "something_new"}, memoryReadNone},
	} {
		if got := test.decision.readAccess(); got != test.want {
			t.Errorf("%s: readAccess = %d, want %d", test.name, got, test.want)
		}
	}
}

func TestMemoryReadVisibilitySQLNarrowsOnlyForOrgOnly(t *testing.T) {
	if got, want := memoryReadVisibilitySQL(memoryReadFull, "$2"), memoryVisibilitySQL("$2"); got != want {
		t.Fatalf("full access changed the predicate: %q", got)
	}
	narrowed := memoryReadVisibilitySQL(memoryReadOrgOnly, "$2")
	if !strings.HasPrefix(narrowed, memoryVisibilitySQL("$2")) || !strings.HasSuffix(narrowed, ` AND scope IN ('org','global')`) {
		t.Fatalf("org-only predicate = %q", narrowed)
	}
}

func TestMemoryWritesRefusedUnlessSessionCoreAllowsGeneration(t *testing.T) {
	for _, denied := range []memoryDecision{memoryDecisionPaused, memoryDecisionOff, memoryDecisionOrgOff, memoryDecisionZDR, memoryDecisionUnknown} {
		controls := &fakeMemoryControls{decision: denied}
		handler, database, _, writeToken := memoryGateHandler(t, controls)

		create := memoryGateDo(handler, http.MethodPost, "/api/v1/memory", writeToken, `{"scope":"user","key":"k","content":"c"}`)
		assertMemoryNotEnabled(t, create, denied.Reason)
		patch := memoryGateDo(handler, http.MethodPatch, "/api/v1/memory/mem-a", writeToken, `{"content":"new"}`)
		assertMemoryNotEnabled(t, patch, denied.Reason)

		if database.databaseCalls() != 0 {
			t.Fatalf("%s: a refused write reached durable storage", denied.Reason)
		}
		if controls.orgID != "org-a" || controls.userID != "user-a" {
			t.Fatalf("decision asked for (%q, %q), want the verified org and actor", controls.orgID, controls.userID)
		}
	}

	controls := &fakeMemoryControls{decision: memoryDecisionOn}
	handler, database, _, writeToken := memoryGateHandler(t, controls)
	create := memoryGateDo(handler, http.MethodPost, "/api/v1/memory", writeToken, `{"scope":"user","key":"k","content":"c"}`)
	if create.Code != http.StatusCreated {
		t.Fatalf("allowed write status = %d, body=%s", create.Code, create.Body.String())
	}
	if len(database.execs) != 1 {
		t.Fatalf("allowed write executed %d statements, want 1", len(database.execs))
	}
}

func TestMemoryWriteNeedsGenerationNotJustUse(t *testing.T) {
	controls := &fakeMemoryControls{decision: memoryDecision{UseAllowed: true, GenerationAllowed: false, Reason: "user_paused"}}
	handler, database, _, writeToken := memoryGateHandler(t, controls)
	response := memoryGateDo(handler, http.MethodPost, "/api/v1/memory", writeToken, `{"scope":"user","key":"k","content":"c"}`)
	assertMemoryNotEnabled(t, response, "user_paused")
	if database.databaseCalls() != 0 {
		t.Fatal("write without generation reached durable storage")
	}
}

func TestMemoryReadsWhenPausedKeepOnlySharedRows(t *testing.T) {
	for _, paused := range []memoryDecision{memoryDecisionPaused, memoryDecisionOff} {
		controls := &fakeMemoryControls{decision: paused}
		handler, database, readToken, _ := memoryGateHandler(t, controls)

		list := memoryGateDo(handler, http.MethodGet, "/api/v1/memory", readToken, "")
		if list.Code != http.StatusOK {
			t.Fatalf("%s: list status = %d, body=%s", paused.Reason, list.Code, list.Body.String())
		}
		resolve := memoryGateDo(handler, http.MethodGet, "/api/v1/memory/resolve?thread_id=t1", readToken, "")
		if resolve.Code != http.StatusOK {
			t.Fatalf("%s: resolve status = %d, body=%s", paused.Reason, resolve.Code, resolve.Body.String())
		}
		get := memoryGateDo(handler, http.MethodGet, "/api/v1/memory/mem-a", readToken, "")
		if get.Code != http.StatusNotFound {
			t.Fatalf("%s: get status = %d, want the empty store's 404", paused.Reason, get.Code)
		}
		calls := append(append([]databaseCall{}, database.queries...), database.queryRows...)
		if len(calls) != 3 {
			t.Fatalf("%s: %d reads reached storage, want 3", paused.Reason, len(calls))
		}
		for _, call := range calls {
			query := strings.Join(strings.Fields(call.query), " ")
			if !strings.Contains(query, ") AND scope IN ('org','global')") {
				t.Fatalf("%s: read is not narrowed to shared scopes: %q", paused.Reason, query)
			}
		}
	}
}

func TestMemoryPausedUserCannotReadOwnScopesExplicitly(t *testing.T) {
	controls := &fakeMemoryControls{decision: memoryDecisionPaused}
	handler, database, readToken, _ := memoryGateHandler(t, controls)
	for _, scope := range []string{"user", "session", "thread", "run", "workspace"} {
		assertMemoryNotEnabled(t, memoryGateDo(handler, http.MethodGet, "/api/v1/memory?scope="+scope, readToken, ""), "user_paused")
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a refused user-scoped read reached durable storage")
	}
	for _, scope := range []string{"org", "global"} {
		if response := memoryGateDo(handler, http.MethodGet, "/api/v1/memory?scope="+scope, readToken, ""); response.Code != http.StatusOK {
			t.Fatalf("shared scope %s status = %d, body=%s", scope, response.Code, response.Body.String())
		}
	}
}

func TestMemoryReadsRefusedWhenOrgOrZDRSuppressMemory(t *testing.T) {
	for _, denied := range []memoryDecision{memoryDecisionOrgOff, memoryDecisionZDR, memoryDecisionUnknown} {
		controls := &fakeMemoryControls{decision: denied}
		handler, database, readToken, _ := memoryGateHandler(t, controls)
		for _, path := range []string{"/api/v1/memory", "/api/v1/memory?scope=org", "/api/v1/memory/resolve", "/api/v1/memory/mem-a"} {
			assertMemoryNotEnabled(t, memoryGateDo(handler, http.MethodGet, path, readToken, ""), denied.Reason)
		}
		if database.databaseCalls() != 0 {
			t.Fatalf("%s: a refused read reached durable storage", denied.Reason)
		}
	}
}

func TestMemoryDeleteIsNeverGated(t *testing.T) {
	controls := &fakeMemoryControls{decision: memoryDecisionOrgOff, err: errors.New("session core down")}
	handler, database, _, writeToken := memoryGateHandler(t, controls)
	database.execTag = "DELETE 1"
	response := memoryGateDo(handler, http.MethodDelete, "/api/v1/memory/mem-a", writeToken, "")
	if response.Code != http.StatusNoContent {
		t.Fatalf("delete status = %d, body=%s, want 204", response.Code, response.Body.String())
	}
	if controls.calls != 0 {
		t.Fatalf("delete asked for the decision %d times; erasure must not depend on it", controls.calls)
	}
}

func TestMemoryGateFailsClosedWhenDecisionUnavailable(t *testing.T) {
	for name, controls := range map[string]memoryControlsReader{
		"lookup error":  &fakeMemoryControls{err: errors.New("session core down")},
		"not wired":     nil,
		"allow ignored": &fakeMemoryControls{decision: memoryDecisionOn, err: errors.New("deadline exceeded")},
	} {
		handler, database, readToken, writeToken := memoryGateHandler(t, controls)
		for _, request := range []struct{ method, path, token, body string }{
			{http.MethodGet, "/api/v1/memory", readToken, ""},
			{http.MethodGet, "/api/v1/memory/resolve", readToken, ""},
			{http.MethodGet, "/api/v1/memory/mem-a", readToken, ""},
			{http.MethodPost, "/api/v1/memory", writeToken, `{"scope":"org","key":"k","content":"c"}`},
			{http.MethodPatch, "/api/v1/memory/mem-a", writeToken, `{"content":"new"}`},
		} {
			response := memoryGateDo(handler, request.method, request.path, request.token, request.body)
			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("%s: %s %s status = %d, body=%s, want 503", name, request.method, request.path, response.Code, response.Body.String())
			}
		}
		if database.databaseCalls() != 0 {
			t.Fatalf("%s: a request without a decision reached durable storage", name)
		}
	}
}

type fakeMemoryServiceClient struct {
	mpv1.MemoryServiceClient
	response *mpv1.GetMemoryControlsResponse
	err      error
	request  *mpv1.GetMemoryControlsRequest
}

func (f *fakeMemoryServiceClient) GetMemoryControls(_ context.Context, request *mpv1.GetMemoryControlsRequest, _ ...grpc.CallOption) (*mpv1.GetMemoryControlsResponse, error) {
	f.request = request
	return f.response, f.err
}

func TestSessionCoreMemoryControlsMapsTheDecision(t *testing.T) {
	client := &fakeMemoryServiceClient{response: &mpv1.GetMemoryControlsResponse{Controls: &mpv1.MemoryControls{
		UseAllowed: false, GenerationAllowed: false, Reason: "user_paused",
	}}}
	decision, err := sessionCoreMemoryControls{client: client}.Decision(context.Background(), "org-a", "user-a")
	if err != nil {
		t.Fatal(err)
	}
	if decision != memoryDecisionPaused {
		t.Fatalf("decision = %+v, want %+v", decision, memoryDecisionPaused)
	}
	if client.request.GetOrgId() != "org-a" || client.request.GetUserId() != "user-a" {
		t.Fatalf("request = %v", client.request)
	}

	client.response = &mpv1.GetMemoryControlsResponse{}
	if _, err := (sessionCoreMemoryControls{client: client}).Decision(context.Background(), "org-a", "user-a"); err == nil {
		t.Fatal("a response without controls was read as an answer")
	}
	client.response, client.err = nil, errors.New("unavailable")
	if _, err := (sessionCoreMemoryControls{client: client}).Decision(context.Background(), "org-a", "user-a"); err == nil {
		t.Fatal("a transport error was read as an answer")
	}
}

func TestWithMemoryControlsNilClientStaysUnwired(t *testing.T) {
	if handler := NewMemoryHandler(nil, nil).WithMemoryControls(nil); handler.controls != nil {
		t.Fatal("a nil MemoryService client produced a decision source")
	}
}
