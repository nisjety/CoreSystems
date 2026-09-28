package agentmemory

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestPut_PostsCreateRequest(t *testing.T) {
	var gotPath, gotAuth string
	var gotBody createRequest
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotAuth = r.Header.Get("Authorization")
		_ = json.NewDecoder(r.Body).Decode(&gotBody)
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"ok"}`))
	}))
	defer srv.Close()

	c, ok := New(Config{BaseURL: srv.URL, APIKey: "tok"})
	if !ok {
		t.Fatal("New returned ok=false")
	}
	rec, err := c.Put(context.Background(), "org1", "thread1", "MEMORY", "m1", "user1", "hello world")
	if err != nil {
		t.Fatalf("Put: %v", err)
	}
	if gotPath != "/v1/long-term-memory/" {
		t.Errorf("path = %q", gotPath)
	}
	if gotAuth != "Bearer tok" {
		t.Errorf("auth = %q, want Bearer tok", gotAuth)
	}
	if len(gotBody.Memories) != 1 {
		t.Fatalf("memories = %d, want 1", len(gotBody.Memories))
	}
	m := gotBody.Memories[0]
	if m.DiscreteMemoryExtracted != "t" {
		t.Fatal("canonical memories must not spawn untracked inferred copies")
	}
	if m.ID != "m1" || m.Text != "hello world" || m.Namespace != "org1" ||
		m.SessionID != "thread1" || m.MemoryType != "semantic" || m.UserID != "user1" {
		t.Errorf("memory = %+v", m)
	}
	if len(m.Topics) != 1 || m.Topics[0] != "MEMORY" {
		t.Errorf("topics = %v", m.Topics)
	}
	if rec.MemoryID != "m1" || rec.Content != "hello world" {
		t.Errorf("rec = %+v", rec)
	}
}

func TestSearch_MapsResultsToHits(t *testing.T) {
	var gotReq searchRequest
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/long-term-memory/search" {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		_ = json.NewDecoder(r.Body).Decode(&gotReq)
		_, _ = io.WriteString(w, `{"memories":[{"id":"m1","text":"the quick brown fox","session_id":"thread1","topics":["MEMORY"],"dist":0.2,"updated_at":"2026-05-28T10:00:00Z"}],"total":1}`)
	}))
	defer srv.Close()

	c, _ := New(Config{BaseURL: srv.URL})
	hits, err := c.Search(context.Background(), "org1", "thread1", "", "brown", []string{"MEMORY"}, time.Time{}, 5)
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if gotReq.Text != "brown" || gotReq.Namespace == nil || gotReq.Namespace.Eq != "org1" {
		t.Errorf("request = %+v", gotReq)
	}
	if gotReq.Limit != 5 || gotReq.SessionID == nil || gotReq.SessionID.Eq != "thread1" {
		t.Errorf("request scope = %+v", gotReq)
	}
	if len(hits) != 1 {
		t.Fatalf("hits = %d, want 1", len(hits))
	}
	h := hits[0]
	if h.MemoryID != "m1" || h.ThreadID != "thread1" || h.Topic != "MEMORY" ||
		h.Content != "the quick brown fox" {
		t.Errorf("hit = %+v", h)
	}
	if h.Score < 0.79 || h.Score > 0.81 { // dist 0.2 -> score ~0.8
		t.Errorf("score = %v, want ~0.8", h.Score)
	}
}

func TestNew_RequiresBaseURL(t *testing.T) {
	if _, ok := New(Config{}); ok {
		t.Error("expected ok=false without BaseURL")
	}
}

func TestList_FiltersByNamespaceAndUserWithoutEmbeddingRoundTrip(t *testing.T) {
	var gotReq searchRequest
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/long-term-memory/search" {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		_ = json.NewDecoder(r.Body).Decode(&gotReq)
		_, _ = io.WriteString(w, `{"memories":[{"id":"m1","text":"prefers dark mode","session_id":"thread1","topics":["USER"],"updated_at":"2026-05-28T10:00:00Z"}],"total":1}`)
	}))
	defer srv.Close()

	c, _ := New(Config{BaseURL: srv.URL})
	hits, err := c.List(context.Background(), "org1", "user1", 50)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if gotReq.Text != "" {
		t.Errorf("List must not send query text, got %q", gotReq.Text)
	}
	if gotReq.SearchMode != "keyword" {
		t.Errorf("List must use keyword mode to skip embedding, got %q", gotReq.SearchMode)
	}
	if gotReq.Namespace == nil || gotReq.Namespace.Eq != "org1" {
		t.Errorf("namespace filter = %+v", gotReq.Namespace)
	}
	if gotReq.UserID == nil || gotReq.UserID.Eq != "user1" {
		t.Errorf("user_id filter = %+v", gotReq.UserID)
	}
	if gotReq.Limit != 50 {
		t.Errorf("limit = %d, want 50", gotReq.Limit)
	}
	if len(hits) != 1 || hits[0].MemoryID != "m1" || hits[0].Content != "prefers dark mode" {
		t.Fatalf("hits = %+v", hits)
	}
}

func TestList_RequiresUserID(t *testing.T) {
	c, _ := New(Config{BaseURL: "http://unused.invalid"})
	if _, err := c.List(context.Background(), "org1", "", 10); err == nil {
		t.Fatal("expected error without userID")
	}
}

// fakeMemoryServer emulates the agent-memory-server 0.15.1 routes Delete
// uses. GET /v1/long-term-memory/{id} returns the record or 404. DELETE
// /v1/long-term-memory?memory_ids=... removes by id alone, in every
// namespace, and acknowledges "ok, deleted N memories".
type fakeMemoryServer struct {
	mu      sync.Mutex
	records map[string]storedMemory
	// deletes holds the raw query of every DELETE the server received.
	deletes []string
	gets    int
	auth    []string
	// ack, when set, replaces the acknowledgement's status text.
	ack string
	// keepOnDelete makes DELETE acknowledge without removing anything.
	keepOnDelete bool
}

func newFakeMemoryServer(t *testing.T, records ...storedMemory) (*fakeMemoryServer, *Client) {
	t.Helper()
	fake := &fakeMemoryServer{records: map[string]storedMemory{}}
	for _, record := range records {
		fake.records[record.ID] = record
	}
	srv := httptest.NewServer(fake)
	t.Cleanup(srv.Close)
	client, ok := New(Config{BaseURL: srv.URL, APIKey: "tok"})
	if !ok {
		t.Fatal("New returned ok=false")
	}
	return fake, client
}

func (f *fakeMemoryServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.auth = append(f.auth, r.Header.Get("Authorization"))
	nullable := func(value string) any {
		if value == "" {
			return nil
		}
		return value
	}
	switch {
	case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/v1/long-term-memory/"):
		f.gets++
		record, ok := f.records[strings.TrimPrefix(r.URL.Path, "/v1/long-term-memory/")]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = io.WriteString(w, `{"detail":"Memory not found"}`)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"id": record.ID, "text": "remembered", "memory_type": "semantic",
			"namespace": nullable(record.Namespace), "user_id": nullable(record.UserID),
		})
	case r.Method == http.MethodDelete && r.URL.Path == "/v1/long-term-memory":
		f.deletes = append(f.deletes, r.URL.RawQuery)
		removed := 0
		for _, id := range r.URL.Query()["memory_ids"] {
			if _, ok := f.records[id]; ok && !f.keepOnDelete {
				delete(f.records, id)
				removed++
			}
		}
		status := fmt.Sprintf("ok, deleted %d memories", removed)
		if f.ack != "" {
			status = f.ack
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"status": status})
	default:
		w.WriteHeader(http.StatusNotFound)
	}
}

func (f *fakeMemoryServer) has(id string) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	_, ok := f.records[id]
	return ok
}

// calls returns what the server has received so far.
func (f *fakeMemoryServer) calls() (gets int, deletes, auth []string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.gets, append([]string(nil), f.deletes...), append([]string(nil), f.auth...)
}

func (f *fakeMemoryServer) configure(ack string, keepOnDelete bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.ack, f.keepOnDelete = ack, keepOnDelete
}

func TestDelete_SendsMemoryIDsAsQueryParam(t *testing.T) {
	fake, c := newFakeMemoryServer(t, storedMemory{ID: "m1", Namespace: "org1", UserID: "user1"})
	deleted, err := c.Delete(context.Background(), "org1", "user1", "m1")
	if err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if !deleted {
		t.Error("expected deleted=true for the caller's own memory")
	}
	_, deletes, auths := fake.calls()
	if len(deletes) != 1 || deletes[0] != "memory_ids=m1" {
		t.Errorf("DELETE queries = %q, want [memory_ids=m1]", deletes)
	}
	for _, auth := range auths {
		if auth != "Bearer tok" {
			t.Errorf("auth = %q, want Bearer tok on every call", auth)
		}
	}
	if fake.has("m1") {
		t.Error("m1 should be gone")
	}
}

// The upstream delete is by id across every namespace, so without the scope
// check any caller could erase any memory whose id it knew, and every 2xx
// reported deleted=true even when nothing matched.
func TestDelete_RemovesOnlyTheCallersOwnMemory(t *testing.T) {
	fake, c := newFakeMemoryServer(t,
		storedMemory{ID: "mine", Namespace: "org1", UserID: "user1"},
		storedMemory{ID: "other-user", Namespace: "org1", UserID: "user2"},
		storedMemory{ID: "other-org", Namespace: "org2", UserID: "user1"},
		storedMemory{ID: "org-level", Namespace: "org1"},
		storedMemory{ID: "thread1:MEMORY:1759312345", Namespace: "org1", UserID: "user1"},
	)
	for _, test := range []struct {
		id   string
		want bool
	}{
		{"other-user", false},
		{"other-org", false},
		{"org-level", false},
		{"never-existed", false},
		{"mine", true},
		{"mine", false}, // already gone
		{"thread1:MEMORY:1759312345", true},
	} {
		deleted, err := c.Delete(context.Background(), "org1", "user1", test.id)
		if err != nil {
			t.Fatalf("Delete(%q): %v", test.id, err)
		}
		if deleted != test.want {
			t.Errorf("Delete(%q) = %v, want %v", test.id, deleted, test.want)
		}
	}
	for _, id := range []string{"other-user", "other-org", "org-level"} {
		if !fake.has(id) {
			t.Errorf("%s was deleted by a caller outside its scope", id)
		}
	}
	if _, deletes, _ := fake.calls(); len(deletes) != 2 {
		t.Errorf("DELETE calls = %q, want only the two owned memories", deletes)
	}
}

func TestDelete_ReportsWhatTheServerActuallyRemoved(t *testing.T) {
	fake, c := newFakeMemoryServer(t, storedMemory{ID: "m1", Namespace: "org1", UserID: "user1"})
	// The memory passes the scope check but is gone by the time the delete
	// runs, so the server removes nothing.
	fake.configure("ok, deleted 0 memories", false)
	deleted, err := c.Delete(context.Background(), "org1", "user1", "m1")
	if err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if deleted {
		t.Error("expected deleted=false when the server deleted 0 memories")
	}
}

func TestDelete_ConfirmsAnUnreadableAcknowledgementByReadingBack(t *testing.T) {
	fake, c := newFakeMemoryServer(t,
		storedMemory{ID: "m1", Namespace: "org1", UserID: "user1"},
		storedMemory{ID: "m2", Namespace: "org1", UserID: "user1"},
	)
	fake.configure("ok", false)
	deleted, err := c.Delete(context.Background(), "org1", "user1", "m1")
	if err != nil || !deleted {
		t.Fatalf("Delete(m1) = %v, %v; want true once the read-back finds it gone", deleted, err)
	}

	fake.configure("ok", true)
	deleted, err = c.Delete(context.Background(), "org1", "user1", "m2")
	if err != nil || deleted {
		t.Fatalf("Delete(m2) = %v, %v; want false while the memory is still there", deleted, err)
	}
}

func TestDelete_WithoutOrgOrUserMatchesNothing(t *testing.T) {
	fake, c := newFakeMemoryServer(t, storedMemory{ID: "m1", Namespace: "org1"})
	for _, scope := range [][2]string{{"", "user1"}, {"org1", ""}} {
		deleted, err := c.Delete(context.Background(), scope[0], scope[1], "m1")
		if err != nil || deleted {
			t.Fatalf("Delete(%q, %q) = %v, %v; want false, nil", scope[0], scope[1], deleted, err)
		}
	}
	if gets, deletes, _ := fake.calls(); gets != 0 || len(deletes) != 0 || !fake.has("m1") {
		t.Fatalf("an unscoped delete reached the server: gets=%d deletes=%q", gets, deletes)
	}
}

func TestDelete_ServerErrorIsReportedNotSwallowed(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer srv.Close()
	c, _ := New(Config{BaseURL: srv.URL})
	if deleted, err := c.Delete(context.Background(), "org1", "user1", "m1"); err == nil || deleted {
		t.Fatalf("Delete = %v, %v; want an error", deleted, err)
	}
}

func TestDeletedCount(t *testing.T) {
	for raw, want := range map[string]struct {
		count int
		ok    bool
	}{
		`{"status":"ok, deleted 1 memories"}`:  {1, true},
		`{"status":"ok, deleted 0 memories"}`:  {0, true},
		`{"status":"ok, deleted 12 memories"}`: {12, true},
		`{"status":"ok"}`:                      {0, false},
		`not json`:                             {0, false},
	} {
		count, ok := deletedCount([]byte(raw))
		if count != want.count || ok != want.ok {
			t.Errorf("deletedCount(%s) = (%d, %v), want (%d, %v)", raw, count, ok, want.count, want.ok)
		}
	}
}

func TestDelete_RequiresMemoryID(t *testing.T) {
	c, _ := New(Config{BaseURL: "http://unused.invalid"})
	if _, err := c.Delete(context.Background(), "org1", "user1", ""); err == nil {
		t.Fatal("expected error without memoryID")
	}
}
