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
	rec, err := c.Put(context.Background(), "org1", "thread1", "MEMORY", "m1", "hello world")
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
		m.SessionID != "thread1" || m.MemoryType != "semantic" {
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
