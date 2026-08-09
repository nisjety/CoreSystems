package agentmemory

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
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
	hits, err := c.Search(context.Background(), "org1", "thread1", "brown", []string{"MEMORY"}, time.Time{}, 5)
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

func TestDelete_SendsMemoryIDsAsQueryParam(t *testing.T) {
	var gotMethod, gotQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotQuery = r.URL.RawQuery
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"ok"}`))
	}))
	defer srv.Close()

	c, _ := New(Config{BaseURL: srv.URL})
	deleted, err := c.Delete(context.Background(), "org1", "user1", "m1")
	if err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if !deleted {
		t.Error("expected deleted=true on a 200 response")
	}
	if gotMethod != http.MethodDelete {
		t.Errorf("method = %q, want DELETE", gotMethod)
	}
	if gotQuery != "memory_ids=m1" {
		t.Errorf("query = %q, want memory_ids=m1", gotQuery)
	}
}

func TestDelete_RequiresMemoryID(t *testing.T) {
	c, _ := New(Config{BaseURL: "http://unused.invalid"})
	if _, err := c.Delete(context.Background(), "org1", "user1", ""); err == nil {
		t.Fatal("expected error without memoryID")
	}
}
