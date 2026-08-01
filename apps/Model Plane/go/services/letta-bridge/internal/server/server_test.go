package server

import (
	"context"
	"errors"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/letta-bridge/internal/memstore"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func TestHealth_ReportsExplicitLexicalDegradedMode(t *testing.T) {
	srv := NewServer()
	resp, err := srv.Health(context.Background(), &mpv1.MemoryHealthRequest{})
	if err != nil {
		t.Fatalf("Health: unexpected error: %v", err)
	}
	if resp.Status != "DEGRADED_LEXICAL_FALLBACK" {
		t.Errorf("Health: expected degraded lexical status, got %q", resp.Status)
	}
	if resp.Ready || resp.MemoryStatus != "DEGRADED_LEXICAL_FALLBACK" {
		t.Fatalf("Health: ready=%v memory_status=%q", resp.Ready, resp.MemoryStatus)
	}
	if ready, _ := srv.ReadyStatus(); ready {
		t.Error("lexical fallback must not claim semantic-search readiness")
	}
}

type semanticTestStore struct {
	searchErr error
	listErr   error
	listHits   []memstore.Hit
	searchHits []memstore.Hit
	deleted    bool
	deleteErr  error
}

func (s semanticTestStore) Put(_ context.Context, orgID, threadID, topic, memoryID, _, content string) (*memstore.Record, error) {
	return &memstore.Record{OrgID: orgID, ThreadID: threadID, Topic: topic, MemoryID: memoryID, Content: content}, nil
}

func (s semanticTestStore) Search(_ context.Context, _, _, _ string, _ []string, _ time.Time, _ int32) ([]memstore.Hit, error) {
	return s.searchHits, s.searchErr
}

func (s semanticTestStore) List(_ context.Context, _, _ string, _ int32) ([]memstore.Hit, error) {
	return s.listHits, s.listErr
}

func (s semanticTestStore) Delete(_ context.Context, _, _, _ string) (bool, error) {
	return s.deleted, s.deleteErr
}

// A backend that answers 200 with zero rows is NOT ready.
//
// This test previously asserted the opposite: its store returned no hits and it
// required status "OK". That is the false green exactly as it shipped — live
// /readyz reported {"memory_status":"OK","ready":true} against an index with
// num_docs 0 and 16 hash_indexing_failures, so nothing in the fleet could tell
// that semantic recall was dead. The assertion is inverted here on purpose.
func TestSemanticReadinessRequiresAnActualHitNotJustA200(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{}, "agent-memory", true)
	if ready, status := srv.ReadyStatus(); ready || status != "DEGRADED_SEMANTIC_UNVERIFIED" {
		t.Fatalf("initial readiness=%v status=%q", ready, status)
	}

	// Succeeds, returns nothing.
	if _, err := srv.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "safe query", Limit: 1}); err != nil {
		t.Fatal(err)
	}
	if ready, status := srv.ReadyStatus(); ready || status != "DEGRADED_SEMANTIC_EMPTY_INDEX" {
		t.Fatalf("a zero-hit backend must not be ready: readiness=%v status=%q", ready, status)
	}

	// One real hit is the evidence readiness requires.
	hitting := NewServerWithBackend(
		semanticTestStore{searchHits: []memstore.Hit{{MemoryID: "m1", Content: "remembered"}}},
		"agent-memory",
		true,
	)
	resp, err := hitting.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "safe query", Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if resp.Degraded || resp.DegradationReason != "" {
		t.Fatalf("a backend that returned a hit must not be degraded: %+v", resp)
	}
	if ready, status := hitting.ReadyStatus(); !ready || status != "OK" {
		t.Fatalf("hit readiness=%v status=%q", ready, status)
	}

	// And readiness is sticky once earned: a later legitimately-empty query for
	// some unrelated term must not flap it back to degraded.
	if _, err := hitting.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "no match", Limit: 1}); err != nil {
		t.Fatal(err)
	}
	if ready, _ := hitting.ReadyStatus(); !ready {
		t.Fatal("readiness must not flap on a legitimately empty query")
	}

	failing := NewServerWithBackend(semanticTestStore{searchErr: errors.New("backend down")}, "agent-memory", true)
	_, _ = failing.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "safe query", Limit: 1})
	if ready, status := failing.ReadyStatus(); ready || status != "DEGRADED_SEMANTIC_UNAVAILABLE" {
		t.Fatalf("failed semantic search readiness=%v status=%q", ready, status)
	}
}

func TestIndexMemory_Success(t *testing.T) {
	srv := NewServer()
	resp, err := srv.IndexMemory(context.Background(), &mpv1.IndexMemoryRequest{
		OrgId:    "org1",
		ThreadId: "thread1",
		Topic:    "MEMORY",
		Content:  "hello world",
	})
	if err != nil {
		t.Fatalf("IndexMemory: unexpected error: %v", err)
	}
	if resp.MemoryId == "" {
		t.Error("expected non-empty MemoryId")
	}
	if !resp.Degraded || resp.DegradationReason != "DEGRADED_LEXICAL_FALLBACK" {
		t.Fatalf("lexical index must expose degradation: %+v", resp)
	}
}

func TestIndexMemory_MissingIdentifiers(t *testing.T) {
	srv := NewServer()
	_, err := srv.IndexMemory(context.Background(), &mpv1.IndexMemoryRequest{
		OrgId:    "",
		ThreadId: "thread1",
		Topic:    "MEMORY",
		Content:  "x",
	})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Errorf("expected InvalidArgument, got %v (err=%v)", got, err)
	}
}

func TestSearchMemory_FindsIndexedBlock(t *testing.T) {
	srv := NewServer()
	ctx := context.Background()
	_, err := srv.IndexMemory(ctx, &mpv1.IndexMemoryRequest{
		OrgId:    "org1",
		ThreadId: "thread1",
		Topic:    "MEMORY",
		Content:  "the quick brown fox",
	})
	if err != nil {
		t.Fatalf("IndexMemory: %v", err)
	}
	resp, err := srv.SearchMemory(ctx, &mpv1.SearchMemoryRequest{
		OrgId:    "org1",
		ThreadId: "thread1",
		Query:    "brown",
		Limit:    5,
	})
	if err != nil {
		t.Fatalf("SearchMemory: %v", err)
	}
	if len(resp.Entries) != 1 {
		t.Fatalf("expected 1 hit, got %d", len(resp.Entries))
	}
	if resp.Entries[0].ThreadId != "thread1" {
		t.Errorf("expected ThreadId=thread1, got %q", resp.Entries[0].ThreadId)
	}
}

func TestSearchMemory_NoMatches(t *testing.T) {
	srv := NewServer()
	resp, err := srv.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{
		OrgId:    "org1",
		ThreadId: "thread1",
		Query:    "anything",
		Limit:    5,
	})
	if err != nil {
		t.Fatalf("SearchMemory: %v", err)
	}
	if len(resp.Entries) != 0 {
		t.Errorf("expected 0 hits, got %d", len(resp.Entries))
	}
	if !resp.Degraded || resp.DegradationReason != "DEGRADED_LEXICAL_FALLBACK" {
		t.Fatalf("empty lexical response must remain distinguishable: %+v", resp)
	}
}

func TestListMemory_TagsEntriesWithTheRequestedUser(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{
		listHits: []memstore.Hit{{MemoryID: "m1", Content: "prefers dark mode", Score: 1}},
	}, "agent-memory", true)
	resp, err := srv.ListMemory(context.Background(), &mpv1.ListMemoryRequest{OrgId: "org-a", UserId: "user-a"})
	if err != nil {
		t.Fatalf("ListMemory: %v", err)
	}
	if len(resp.Entries) != 1 || resp.Entries[0].UserId != "user-a" || resp.Entries[0].MemoryId != "m1" {
		t.Fatalf("entries = %+v", resp.Entries)
	}
}

func TestListMemory_BackendErrorIsNotSilentlyEmpty(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{listErr: errors.New("backend down")}, "agent-memory", true)
	_, err := srv.ListMemory(context.Background(), &mpv1.ListMemoryRequest{OrgId: "org-a", UserId: "user-a"})
	if err == nil {
		t.Fatal("expected an error, not a silently empty list")
	}
}

func TestDeleteMemory_ReportsNotFoundAsFalseNotError(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{deleted: false}, "agent-memory", true)
	resp, err := srv.DeleteMemory(context.Background(), &mpv1.DeleteMemoryRequest{
		OrgId: "org-a", UserId: "user-a", MemoryId: "missing",
	})
	if err != nil {
		t.Fatalf("DeleteMemory: unexpected error for a well-formed miss: %v", err)
	}
	if resp.Deleted {
		t.Fatal("expected Deleted=false")
	}
}

func TestDeleteMemory_Success(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{deleted: true}, "agent-memory", true)
	resp, err := srv.DeleteMemory(context.Background(), &mpv1.DeleteMemoryRequest{
		OrgId: "org-a", UserId: "user-a", MemoryId: "m1",
	})
	if err != nil {
		t.Fatalf("DeleteMemory: %v", err)
	}
	if !resp.Deleted {
		t.Fatal("expected Deleted=true")
	}
}
