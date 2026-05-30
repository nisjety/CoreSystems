package server

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func TestHealth_ReturnsOK(t *testing.T) {
	srv := NewServer()
	resp, err := srv.Health(context.Background(), &mpv1.MemoryHealthRequest{})
	if err != nil {
		t.Fatalf("Health: unexpected error: %v", err)
	}
	if resp.Status != "OK" {
		t.Errorf("Health: expected status=OK, got %q", resp.Status)
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
}
