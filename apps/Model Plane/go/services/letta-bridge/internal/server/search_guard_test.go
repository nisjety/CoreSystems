package server

import (
	"context"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/letta-bridge/internal/memstore"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// countingSearchStore counts Search calls, so a refused search can be shown to stop
// before the store is asked anything.
type countingSearchStore struct {
	semanticTestStore
	searches int
}

func (s *countingSearchStore) Search(ctx context.Context, orgID, threadID, userID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error) {
	s.searches++
	return s.semanticTestStore.Search(ctx, orgID, threadID, userID, query, topicFilter, updatedAfter, topK)
}

// WS17-S3: a search must be scoped to a thread or a user. With neither, the search
// would reach every memory in the tenant, so it is refused as InvalidArgument before
// the store is asked. Either scope alone is still allowed.
func TestSearchMemoryRefusesNeitherThreadNorUserBeforeTheStore(t *testing.T) {
	store := &countingSearchStore{}
	srv := NewServerWithBackend(store, "agent-memory", true)

	_, err := srv.SearchMemory(context.Background(), &mpv1.SearchMemoryRequest{OrgId: "org-a", Query: "anything", Limit: 1})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("unscoped SearchMemory code = %v (err=%v), want InvalidArgument", got, err)
	}
	if store.searches != 0 {
		t.Fatalf("store was asked %d time(s) for an unscoped search", store.searches)
	}

	for _, request := range []*mpv1.SearchMemoryRequest{
		{OrgId: "org-a", ThreadId: "thread-1", Query: "anything", Limit: 1},
		{OrgId: "org-a", UserId: "user-a", Query: "anything", Limit: 1},
	} {
		if _, err := srv.SearchMemory(context.Background(), request); err != nil {
			t.Fatalf("scoped search (thread=%q user=%q) refused: %v", request.GetThreadId(), request.GetUserId(), err)
		}
	}
	if store.searches != 2 {
		t.Fatalf("store searches = %d, want 2 for the two scoped requests", store.searches)
	}
}
