//go:build integration

package watch

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// The create/cancel surface, against a real store.
//
// Run with: go test -tags integration ./internal/watch/

type recordingAuthorizer struct {
	calls  int
	err    error
	issued Authority
}

func (a *recordingAuthorizer) AuthorizeCreate(context.Context, Watch) (Authority, error) {
	a.calls++
	if a.err != nil {
		return Authority{}, a.err
	}
	if a.issued.RecipientAudienceRevision == 0 {
		a.issued = Authority{RecipientAudienceRef: "space:space-1:audience:4", RecipientAudienceRevision: 4}
	}
	return a.issued, nil
}

type recordingResolver struct {
	calls  int
	cursor int64
	err    error
}

func (r *recordingResolver) Resolve(context.Context, Watch) (int64, error) {
	r.calls++
	return r.cursor, r.err
}

func handlerOver(t *testing.T, store *Store, authz CreateAuthorizer, resolver SourceResolver) *Handler {
	t.Helper()
	resolvers := map[string]SourceResolver{}
	if resolver != nil {
		resolvers[SourceKindProcessOutput] = resolver
	}
	n := 0
	h, err := NewHandler(store, authz, resolvers, func() string {
		n++
		return fmt.Sprintf("wch_%d", n)
	})
	if err != nil {
		t.Fatalf("NewHandler: %v", err)
	}
	return h
}

func asUser(r *http.Request) *http.Request {
	return r.WithContext(authctx.ContextWithPrincipal(r.Context(), authctx.Principal{
		PrincipalType: "user", ActorID: "user-1", OrganizationID: "org-1",
	}))
}

func createBody(predicate map[string]any) *bytes.Reader {
	body, _ := json.Marshal(map[string]any{
		"space_ref": "space-1", "source_kind": SourceKindProcessOutput,
		"source_ref": "proc-1", "predicate": predicate,
	})
	return bytes.NewReader(body)
}

func postCreate(t *testing.T, h *Handler, predicate map[string]any) *httptest.ResponseRecorder {
	t.Helper()
	recorder := httptest.NewRecorder()
	request := asUser(httptest.NewRequest(http.MethodPost, "/api/v1/watches", createBody(predicate)))
	h.listOrCreate(recorder, request)
	return recorder
}

func TestCreateRecordsAWatchWithItsAuthorityAndStartCursor(t *testing.T) {
	store, _ := setupWatchStore(t)
	authz := &recordingAuthorizer{}
	resolver := &recordingResolver{cursor: 42}

	recorder := postCreate(t, handlerOver(t, store, authz, resolver),
		map[string]any{"kind": PredicateContains, "value": "ERROR", "stream": "stderr"})
	if recorder.Code != http.StatusCreated {
		t.Fatalf("status = %d: %s", recorder.Code, recorder.Body.String())
	}

	got, err := store.Get(context.Background(), "org-1", "wch_1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	// The authority binding is what the sweeper will later compare against; a
	// watch stored without one has nothing to re-derive from.
	if got.Authority.RecipientAudienceRevision != 4 {
		t.Fatalf("authority did not reach the row: %+v", got.Authority)
	}
	// A watch on a process running for an hour must not replay that hour: the
	// person asked what happens NEXT.
	if got.CursorValue != 42 {
		t.Fatalf("cursor = %d, want the resolver's current position", got.CursorValue)
	}
	if got.Predicate.Value != "ERROR" || got.Predicate.Stream != "stderr" {
		t.Fatalf("predicate did not round-trip: %+v", got.Predicate)
	}
}

// Authority FIRST, then the source. A caller who may not watch this Space at
// all must not be able to use the source check as a probe for which resources
// exist in it.
func TestCreateChecksAuthorityBeforeTouchingTheSource(t *testing.T) {
	store, _ := setupWatchStore(t)
	authz := &recordingAuthorizer{err: fmt.Errorf("Control refused this watch")}
	resolver := &recordingResolver{cursor: 1}

	recorder := postCreate(t, handlerOver(t, store, authz, resolver), map[string]any{"kind": PredicateAny})
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403", recorder.Code)
	}
	if resolver.calls != 0 {
		t.Fatal("the source was resolved for a caller Control refused; that is an existence oracle")
	}
}

// A missing source is refused at CREATE, not left to fail silently forever. The
// adapter would refuse it on every poll anyway, so nothing leaks — but the
// person would be told their watch exists and never hear from it.
func TestCreateRefusesAnUnresolvableSource(t *testing.T) {
	store, _ := setupWatchStore(t)
	resolver := &recordingResolver{err: ErrSourceGone}

	recorder := postCreate(t, handlerOver(t, store, &recordingAuthorizer{}, resolver), map[string]any{"kind": PredicateAny})
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", recorder.Code)
	}
	if _, err := store.Get(context.Background(), "org-1", "wch_1"); err == nil {
		t.Fatal("a watch was recorded for a source that does not exist")
	}
}

// A kind this deployment cannot poll must not be creatable: it would occupy a
// slot and never report.
func TestCreateRefusesAnUnservableKind(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, nil) // no resolvers at all

	recorder := postCreate(t, h, map[string]any{"kind": PredicateAny})
	if recorder.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 for a kind nothing serves", recorder.Code)
	}
}

// The closed grammar is only useful if a caller can tell which rule they broke.
func TestCreateRefusesAnInvalidPredicateWithItsReason(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})

	for _, predicate := range []map[string]any{
		{"kind": "regex", "value": "^(a+)+$"},
		{"kind": PredicateContains},
		{"kind": PredicateAny, "value": "ERROR"},
	} {
		recorder := postCreate(t, h, predicate)
		if recorder.Code != http.StatusBadRequest {
			t.Fatalf("predicate %v returned %d, want 400", predicate, recorder.Code)
		}
		if !bytes.Contains(recorder.Body.Bytes(), []byte("predicate")) {
			t.Fatalf("the refusal does not say what was wrong: %s", recorder.Body.String())
		}
	}
}

func TestCreateEnforcesThePerSpaceLimit(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})

	// Distinct predicates so the duplicate index is not what refuses them.
	for i := 0; i < MaxActiveWatchesPerSpace; i++ {
		recorder := postCreate(t, h, map[string]any{"kind": PredicateContains, "value": fmt.Sprintf("E%d", i)})
		if recorder.Code != http.StatusCreated {
			t.Fatalf("watch %d returned %d: %s", i, recorder.Code, recorder.Body.String())
		}
	}
	recorder := postCreate(t, h, map[string]any{"kind": PredicateContains, "value": "one-too-many"})
	if recorder.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409 past the per-Space limit", recorder.Code)
	}
}

// Watching the same thing for the same reason twice is a duplicate, not two
// answers — and the person is told so rather than given a second stream of the
// same events.
func TestCreateRefusesADuplicateWithAUsefulStatus(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})
	predicate := map[string]any{"kind": PredicateContains, "value": "ERROR"}

	if recorder := postCreate(t, h, predicate); recorder.Code != http.StatusCreated {
		t.Fatalf("first create = %d", recorder.Code)
	}
	recorder := postCreate(t, h, predicate)
	if recorder.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409 for an identical active watch", recorder.Code)
	}
}

// Only the person who asked to be told may stop being told. Reported as
// not-found rather than forbidden: the id belongs to someone, and saying which
// is a disclosure of its own.
func TestOnlyTheCreatorMayCancel(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})
	if recorder := postCreate(t, h, map[string]any{"kind": PredicateAny}); recorder.Code != http.StatusCreated {
		t.Fatalf("create = %d", recorder.Code)
	}

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodDelete, "/api/v1/watches/wch_1", nil)
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		PrincipalType: "user", ActorID: "user-2", OrganizationID: "org-1",
	}))
	h.Cancel(recorder, request, "org-1", "user-2", "wch_1")
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for someone else's watch", recorder.Code)
	}
	got, err := store.Get(context.Background(), "org-1", "wch_1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateActive {
		t.Fatalf("another member cancelled a watch: state = %s", got.State)
	}
}

// Cancelling is idempotent: the caller's intent is already satisfied, and
// reporting a failure would invite a retry that can never succeed.
func TestCancelIsIdempotent(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})
	if recorder := postCreate(t, h, map[string]any{"kind": PredicateAny}); recorder.Code != http.StatusCreated {
		t.Fatalf("create = %d", recorder.Code)
	}

	for attempt := 1; attempt <= 2; attempt++ {
		recorder := httptest.NewRecorder()
		request := asUser(httptest.NewRequest(http.MethodDelete, "/api/v1/watches/wch_1", nil))
		h.Cancel(recorder, request, "org-1", "user-1", "wch_1")
		if recorder.Code != http.StatusOK {
			t.Fatalf("cancel attempt %d = %d", attempt, recorder.Code)
		}
	}
	got, err := store.Get(context.Background(), "org-1", "wch_1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.State != StateCancelled {
		t.Fatalf("state = %s, want CANCELLED", got.State)
	}
}

// A watch belongs to a person. An empty subject would make the creator check
// vacuous, and a watch nobody owns is a watch nobody can stop.
func TestAServiceIdentityCannotHoldAWatch(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/watches", createBody(map[string]any{"kind": PredicateAny}))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		PrincipalType: "service", ActorID: "capability-core", OrganizationID: "org-1",
	}))
	h.listOrCreate(recorder, request)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403 for a service identity", recorder.Code)
	}
}

// "What am I being told about" is the question a person opens the list with,
// and a month of finished watches buries the answer.
func TestListHidesTerminalWatchesUnlessAsked(t *testing.T) {
	store, _ := setupWatchStore(t)
	h := handlerOver(t, store, &recordingAuthorizer{}, &recordingResolver{})
	for _, value := range []string{"A", "B"} {
		if recorder := postCreate(t, h, map[string]any{"kind": PredicateContains, "value": value}); recorder.Code != http.StatusCreated {
			t.Fatalf("create %s = %d", value, recorder.Code)
		}
	}
	if err := store.Terminate(context.Background(), "org-1", "wch_1", StateCancelled, ""); err != nil {
		t.Fatalf("Terminate: %v", err)
	}

	count := func(includeFinished bool) int {
		recorder := httptest.NewRecorder()
		url := "/api/v1/watches?space_ref=space-1"
		if includeFinished {
			url += "&include_finished=true"
		}
		h.listOrCreate(recorder, asUser(httptest.NewRequest(http.MethodGet, url, nil)))
		if recorder.Code != http.StatusOK {
			t.Fatalf("list = %d: %s", recorder.Code, recorder.Body.String())
		}
		var envelope struct {
			Data struct {
				Watches []map[string]any `json:"watches"`
			} `json:"data"`
		}
		if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
			t.Fatalf("decode: %v", err)
		}
		return len(envelope.Data.Watches)
	}
	if got := count(false); got != 1 {
		t.Fatalf("default list returned %d watches, want only the active one", got)
	}
	if got := count(true); got != 2 {
		t.Fatalf("include_finished returned %d watches, want both", got)
	}
}
