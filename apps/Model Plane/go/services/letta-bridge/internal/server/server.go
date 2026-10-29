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
	Put(ctx context.Context, orgID, threadID, topic, memoryID, userID, content string) (*memstore.Record, error)
	Search(ctx context.Context, orgID, threadID, userID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error)
	// List enumerates memories owned by userID, across every thread, scoped
	// to orgID. Backs ListMemory -- see MemoryService in memory.proto. A
	// backend with no per-user ownership tracking may legitimately return an
	// empty result rather than risk a cross-user leak; see each
	// implementation's own doc comment.
	List(ctx context.Context, orgID, userID string, topK int32) ([]memstore.Hit, error)
	// Delete removes a single memory by id. Returns whether a record was
	// found and removed. Never returns an error for "not found" -- that is a
	// valid, informational false, not a failure.
	Delete(ctx context.Context, orgID, userID, memoryID string) (bool, error)
}

// inMemoryStore adapts the goroutine-safe in-memory memstore.Store to the
// context-aware Store interface. The context is unused — the store is local.
type inMemoryStore struct{ s *memstore.Store }

func (m *inMemoryStore) Put(_ context.Context, orgID, threadID, topic, memoryID, userID, content string) (*memstore.Record, error) {
	return m.s.Put(orgID, threadID, topic, memoryID, userID, content)
}

func (m *inMemoryStore) Search(_ context.Context, orgID, threadID, userID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error) {
	return m.s.Search(orgID, threadID, userID, query, topicFilter, updatedAfter, topK), nil
}

// List always returns an empty result. memstore has no per-user ownership
// column (see Put), so there is no safe way to answer "list this user's
// memories" without either ignoring the user filter (a cross-user leak
// within the org) or scanning everything (an org-wide leak). Neither is
// acceptable, so this tier reports nothing rather than risk either. The
// durable, correctly user-scoped source of truth is session-core's own
// `agent_memory` table; this tier is a supplementary lexical layer only.
func (m *inMemoryStore) List(_ context.Context, _, _ string, _ int32) ([]memstore.Hit, error) {
	return nil, nil
}

// Delete removes a single memory owned by (orgID, userID). See
// memstore.Store.Delete for the scoping rationale.
func (m *inMemoryStore) Delete(_ context.Context, orgID, userID, memoryID string) (bool, error) {
	return m.s.Delete(orgID, userID, memoryID), nil
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
	// semanticReturnedHit records that the semantic backend has ever returned
	// at least one row.
	//
	// A vector index that answers every query with an empty result set is not
	// working, and until now it reported OK: both call sites set
	// semanticSearchOkay from (err == nil), and a 0-document index answers 200
	// with zero rows. Live proof of the lie — /readyz returned
	// {"memory_status":"OK","ready":true} against an index whose FT.INFO showed
	// num_docs 0 and 16 hash_indexing_failures from a 3072-vs-1536 dimension
	// mismatch. Nothing in the fleet could tell that recall was dead.
	semanticReturnedHit atomic.Bool
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
	// "It answered" is not "it works". Readiness requires evidence that the
	// index has actually produced a row. Reported as its own status rather than
	// folded into UNAVAILABLE, because the operator action differs: UNAVAILABLE
	// means the backend is erroring, EMPTY means it is healthy and holding
	// nothing retrievable — usually an index that needs rebuilding at the right
	// dimension.
	//
	// This is deliberately sticky-on-first-hit rather than per-query: a
	// legitimately empty result for a specific query must not flap readiness.
	if !s.semanticReturnedHit.Load() {
		return false, "DEGRADED_SEMANTIC_EMPTY_INDEX"
	}
	return true, "OK"
}

// observeSemantic records the outcome of one semantic backend call.
//
// Both callers used to inline the two Store calls and neither recorded whether
// anything came back, which is how the false green survived in two places at
// once. One helper, so a third call site cannot reintroduce it.
func (s *Server) observeSemantic(err error, hits int) {
	if !s.semantic {
		return
	}
	s.semanticObserved.Store(true)
	s.semanticSearchOkay.Store(err == nil)
	if err == nil && hits > 0 {
		s.semanticReturnedHit.Store(true)
	}
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
	raw, err := s.store.Search(ctx, req.OrgId, req.ThreadId, req.UserId, req.Query, req.TopicFilter, updatedAfter, int32(req.Limit))
	s.observeSemantic(err, len(raw))
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

// IndexMemory upserts a record into the in-memory store. When the caller
// supplies MemoryId (session-core does, for entries it can also delete
// later), it is reused verbatim so the same logical memory shares one id
// across the durable index and this backend; otherwise a fresh id is
// generated, matching the RPC's documented default in memory.proto.
func (s *Server) IndexMemory(ctx context.Context, req *mpv1.IndexMemoryRequest) (*mpv1.IndexMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "IndexMemory")))
	memoryID := req.MemoryId
	if memoryID == "" {
		memoryID = fmt.Sprintf("%s:%s:%d", req.ThreadId, req.Topic, time.Now().UnixNano())
	}
	rec, err := s.store.Put(ctx, req.OrgId, req.ThreadId, req.Topic, memoryID, req.UserId, req.Content)
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

// ListMemory returns memories owned by a user, across every thread. Unlike
// SearchMemory this is never thread-scoped -- see ListMemoryRequest in
// memory.proto.
func (s *Server) ListMemory(ctx context.Context, req *mpv1.ListMemoryRequest) (*mpv1.ListMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListMemory")))
	limit := req.Limit
	if limit == 0 {
		limit = 100
	}
	raw, err := s.store.List(ctx, req.OrgId, req.UserId, int32(limit))
	s.observeSemantic(err, len(raw))
	if err != nil {
		telemetry.MemoryListedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "error")))
		return nil, mapErr(fmt.Errorf("list backend: %w", err))
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
			UserId:    req.UserId,
		}
	}
	telemetry.MemoryListedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "ok")))
	degraded, degradationReason := s.degradation()
	return &mpv1.ListMemoryResponse{
		Entries:           entries,
		Degraded:          degraded,
		DegradationReason: degradationReason,
	}, nil
}

// DeleteMemory removes a single memory by id. A "not found" outcome is
// reported as Deleted=false, never as an error -- see the Store.Delete doc
// comment.
func (s *Server) DeleteMemory(ctx context.Context, req *mpv1.DeleteMemoryRequest) (*mpv1.DeleteMemoryResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "DeleteMemory")))
	deleted, err := s.store.Delete(ctx, req.OrgId, req.UserId, req.MemoryId)
	if err != nil {
		telemetry.MemoryDeletedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "error")))
		return nil, mapErr(fmt.Errorf("delete backend: %w", err))
	}
	telemetry.MemoryDeletedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", deleteOutcome(deleted))))
	degraded, degradationReason := s.degradation()
	return &mpv1.DeleteMemoryResponse{
		Deleted:           deleted,
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
