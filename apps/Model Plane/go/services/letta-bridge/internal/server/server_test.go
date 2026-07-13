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

type semanticTestStore struct{ searchErr error }

func (s semanticTestStore) Put(_ context.Context, orgID, threadID, topic, memoryID, content string) (*memstore.Record, error) {
	return &memstore.Record{OrgID: orgID, ThreadID: threadID, Topic: topic, MemoryID: memoryID, Content: content}, nil
}

func (s semanticTestStore) Search(_ context.Context, _, _, _ string, _ []string, _ time.Time, _ int32) ([]memstore.Hit, error) {
	return nil, s.searchErr
}

func TestSemanticReadinessTracksObservedSearchOutcome(t *testing.T) {
	srv := NewServerWithBackend(semanticTestStore{}, "agent-memory", true)
	if ready, status := srv.ReadyStatus(); ready || status != "DEGRADED_SEMANTIC_UNVERIFIED" {
		t.Fatalf("initial readiness=%v status=%q", ready, status)
	}
	resp, err := srv.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "safe query", Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if resp.Degraded || resp.DegradationReason != "" {
		t.Fatalf("successful semantic response marked degraded: %+v", resp)
	}
	if ready, status := srv.ReadyStatus(); !ready || status != "OK" {
		t.Fatalf("successful semantic search readiness=%v status=%q", ready, status)
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
