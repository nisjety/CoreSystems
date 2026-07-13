package server

import (
	"context"
	"fmt"
	"sync/atomic"
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
	store              Store
	backendKind        string
	semantic           bool
	semanticObserved   atomic.Bool
	semanticSearchOkay atomic.Bool
}

// NewServer constructs a Server with a fresh in-memory store.
func NewServer() *Server {
	return NewServerWithBackend(&inMemoryStore{s: memstore.NewStore()}, "in-memory", false)
}

// NewServerWithStore constructs a Server backed by a conservative non-semantic
// store. Call NewServerWithBackend when the backend provides semantic search.
func NewServerWithStore(store Store) *Server {
	return NewServerWithBackend(store, "configured", false)
}

// NewServerWithBackend constructs a server with explicit backend capability
// metadata. A semantic backend does not report ready until an actual search has
// succeeded, preventing a green process health check from masquerading as
// working semantic retrieval.
func NewServerWithBackend(store Store, backendKind string, semantic bool) *Server {
	return &Server{store: store, backendKind: backendKind, semantic: semantic}
}

// ReadyStatus reports whether semantic retrieval has been observed working.
// Lexical stores remain intentionally degraded even when they can return hits.
func (s *Server) ReadyStatus() (bool, string) {
	if !s.semantic {
		return false, "DEGRADED_LEXICAL_FALLBACK"
	}
	if !s.semanticObserved.Load() {
		return false, "DEGRADED_SEMANTIC_UNVERIFIED"
	}
	if !s.semanticSearchOkay.Load() {
		return false, "DEGRADED_SEMANTIC_UNAVAILABLE"
	}
	return true, "OK"
}

func (s *Server) degradation() (bool, string) {
	ready, memoryStatus := s.ReadyStatus()
	if ready {
		return false, ""
	}
	return true, memoryStatus
}

// SearchMemory returns hits from the in-memory store.
func (s *Server) SearchMemory(ctx context.Context, req *mpv1.SearchMemoryRequest) (*mpv1.SearchMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "SearchMemory")))
	updatedAfter := time.Time{}
	if req.UpdatedAfter != nil {
		updatedAfter = req.UpdatedAfter.AsTime()
	}
	raw, err := s.store.Search(ctx, req.OrgId, req.ThreadId, req.Query, req.TopicFilter, updatedAfter, int32(req.Limit))
	if s.semantic {
		s.semanticObserved.Store(true)
		s.semanticSearchOkay.Store(err == nil)
	}
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
	degraded, degradationReason := s.degradation()
	return &mpv1.SearchMemoryResponse{
		Entries:           entries,
		Degraded:          degraded,
		DegradationReason: degradationReason,
	}, nil
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
	degraded, degradationReason := s.degradation()
	return &mpv1.IndexMemoryResponse{
		MemoryId:          rec.MemoryID,
		Degraded:          degraded,
		DegradationReason: degradationReason,
	}, nil
}

// Health reports semantic capability rather than process liveness.
func (s *Server) Health(ctx context.Context, _ *mpv1.MemoryHealthRequest) (*mpv1.MemoryHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	ready, readiness := s.ReadyStatus()
	return &mpv1.MemoryHealthResponse{
		Status:       readiness,
		Ready:        ready,
		MemoryStatus: readiness,
	}, nil
}

// Register wires the MemoryService onto the provided gRPC server.
func Register(g grpc.ServiceRegistrar, impl mpv1.MemoryServiceServer) {
	mpv1.RegisterMemoryServiceServer(g, impl)
}
