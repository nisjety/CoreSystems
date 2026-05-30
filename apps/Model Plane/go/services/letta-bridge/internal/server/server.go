package server

import (
	"context"
	"fmt"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/triodelab/model-plane/services/letta-bridge/internal/memstore"
	"github.com/triodelab/model-plane/services/letta-bridge/internal/telemetry"
)

// Store is the memory backend the server reads and writes through. The
// in-memory implementation is the default; a Redis Agent Memory backend
// (vector long-term recall) can be swapped in without touching the gRPC
// layer. Methods take a context so network-backed stores can honor
// cancellation and timeouts.
type Store interface {
	Put(ctx context.Context, orgID, threadID, topic, memoryID, content string) (*memstore.Record, error)
	Search(ctx context.Context, orgID, threadID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error)
}

// inMemoryStore adapts the goroutine-safe in-memory memstore.Store to the
// context-aware Store interface. The context is unused — the store is local.
type inMemoryStore struct{ s *memstore.Store }

func (m *inMemoryStore) Put(_ context.Context, orgID, threadID, topic, memoryID, content string) (*memstore.Record, error) {
	return m.s.Put(orgID, threadID, topic, memoryID, content)
}

func (m *inMemoryStore) Search(_ context.Context, orgID, threadID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error) {
	return m.s.Search(orgID, threadID, query, topicFilter, updatedAfter, topK), nil
}

// Server is the MemoryService implementation. It is backed by a Store, which
// defaults to the in-memory stub but can be any backend (e.g. Redis Agent
// Memory).
type Server struct {
	mpv1.UnimplementedMemoryServiceServer
	store Store
}

// NewServer constructs a Server with a fresh in-memory store.
func NewServer() *Server { return &Server{store: &inMemoryStore{s: memstore.NewStore()}} }

// NewServerWithStore constructs a Server backed by the provided Store.
func NewServerWithStore(store Store) *Server { return &Server{store: store} }

// SearchMemory returns hits from the in-memory store.
func (s *Server) SearchMemory(ctx context.Context, req *mpv1.SearchMemoryRequest) (*mpv1.SearchMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "SearchMemory")))
	updatedAfter := time.Time{}
	if req.UpdatedAfter != nil {
		updatedAfter = req.UpdatedAfter.AsTime()
	}
	raw, err := s.store.Search(ctx, req.OrgId, req.ThreadId, req.Query, req.TopicFilter, updatedAfter, int32(req.Limit))
	if err != nil {
		telemetry.MemorySearchesTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "error")))
		return nil, mapErr(fmt.Errorf("search backend: %w", err))
	}
	entries := make([]*mpv1.MemoryEntry, len(raw))
	for i, h := range raw {
		entries[i] = &mpv1.MemoryEntry{
			MemoryId:  h.MemoryID,
			ThreadId:  h.ThreadID,
			Topic:     h.Topic,
			Content:   h.Content,
			Score:     h.Score,
			UpdatedAt: timestamppb.New(h.UpdatedAt),
		}
	}
	telemetry.MemorySearchesTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "ok")))
	return &mpv1.SearchMemoryResponse{Entries: entries}, nil
}

// IndexMemory upserts a record into the in-memory store.
func (s *Server) IndexMemory(ctx context.Context, req *mpv1.IndexMemoryRequest) (*mpv1.IndexMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "IndexMemory")))
	memoryID := fmt.Sprintf("%s:%s:%d", req.ThreadId, req.Topic, time.Now().UnixNano())
	rec, err := s.store.Put(ctx, req.OrgId, req.ThreadId, req.Topic, memoryID, req.Content)
	if err != nil {
		// memstore validation errors are surfaced as ErrInvalidArgument so
		// they are classified consistently across the service boundary.
		wrapped := fmt.Errorf("%w: %s", ErrInvalidArgument, err.Error())
		telemetry.MemoryIndexedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", indexOutcome(wrapped))))
		return nil, mapErr(wrapped)
	}
	telemetry.MemoryIndexedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "ok")))
	return &mpv1.IndexMemoryResponse{MemoryId: rec.MemoryID}, nil
}

// Health reports OK.
func (s *Server) Health(ctx context.Context, _ *mpv1.MemoryHealthRequest) (*mpv1.MemoryHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	return &mpv1.MemoryHealthResponse{Status: "OK"}, nil
}

// Register wires the MemoryService onto the provided gRPC server.
func Register(g grpc.ServiceRegistrar, impl mpv1.MemoryServiceServer) {
	mpv1.RegisterMemoryServiceServer(g, impl)
}
