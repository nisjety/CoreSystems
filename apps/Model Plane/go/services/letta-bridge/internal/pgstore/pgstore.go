// Package pgstore provides a Postgres-backed durable store for agent memory
// blocks. It satisfies the server.Store interface, upgrading the in-memory
// substring stub to a store that survives process restarts.
//
// This tier exists because orchestrator-core's MemoryConsolidationWorkflow
// writes consolidated memory back through IndexMemory and later reads it via
// SearchMemory. With the in-memory store those writes are lost on restart,
// which breaks consolidation continuity. The semantic agent-memory-server
// backend (internal/agentmemory) remains the preferred tier when configured;
// pgstore is the durable middle ground when only a Postgres DSN is available.
//
// Search is a case-insensitive substring match (ILIKE) ranked by recency,
// mirroring the in-memory store's semantics so callers see identical behavior
// regardless of which non-semantic tier is active.
package pgstore

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/letta-bridge/internal/memstore"
)

// schemaDDL is applied idempotently at construction so the store works in local
// (non-Docker) runs and tests without external migration tooling. The Docker
// image additionally applies migrations/ via the entrypoint, matching the
// pattern used by capability-core. The two are kept in sync.
const schemaDDL = `
CREATE TABLE IF NOT EXISTS letta_memory_blocks (
    org_id     TEXT        NOT NULL,
    thread_id  TEXT        NOT NULL,
    memory_id  TEXT        NOT NULL,
    topic      TEXT        NOT NULL,
    content    TEXT        NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, thread_id, memory_id)
);
-- Added after the table's initial release (see migrations/0002); a plain
-- CREATE TABLE IF NOT EXISTS above is a no-op against a pre-existing table,
-- so the column needs its own idempotent statement to reach installs that
-- already have the table.
ALTER TABLE letta_memory_blocks ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_letta_memory_org_thread
    ON letta_memory_blocks (org_id, thread_id);
CREATE INDEX IF NOT EXISTS idx_letta_memory_org_updated
    ON letta_memory_blocks (org_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_letta_memory_org_user
    ON letta_memory_blocks (org_id, user_id);
`

// Store is a Postgres-backed durable memory store.
type Store struct {
	pool *pgxpool.Pool
}

// New constructs a Store and ensures the schema exists. The pool must be
// non-nil. The provided context bounds the schema-ensure step only.
func New(ctx context.Context, pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgstore: pgx pool required")
	}
	if _, err := pool.Exec(ctx, schemaDDL); err != nil {
		return nil, fmt.Errorf("pgstore: ensure schema: %w", err)
	}
	return &Store{pool: pool}, nil
}

// Put inserts or replaces a record. It returns an error if required
// identifiers are empty, matching the in-memory store's validation so the
// gRPC layer classifies the error identically.
func (s *Store) Put(ctx context.Context, orgID, threadID, topic, memoryID, userID, content string) (*memstore.Record, error) {
	if orgID == "" || threadID == "" || topic == "" || memoryID == "" {
		return nil, errors.New("orgID, threadID, topic, and memoryID are required")
	}
	now := time.Now().UTC()
	const q = `
INSERT INTO letta_memory_blocks (org_id, thread_id, memory_id, topic, user_id, content, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT (org_id, thread_id, memory_id)
DO UPDATE SET topic = EXCLUDED.topic,
              user_id = EXCLUDED.user_id,
              content = EXCLUDED.content,
              updated_at = EXCLUDED.updated_at`
	if _, err := s.pool.Exec(ctx, q, orgID, threadID, memoryID, topic, userID, content, now); err != nil {
		return nil, fmt.Errorf("pgstore put: %w", err)
	}
	return &memstore.Record{
		OrgID:     orgID,
		ThreadID:  threadID,
		Topic:     topic,
		MemoryID:  memoryID,
		UserID:    userID,
		Content:   content,
		UpdatedAt: now,
	}, nil
}

// Search returns up to topK hits whose content contains the query substring
// (case-insensitive), scoped to the org and optional thread/topic/updatedAfter
// filters, ordered most-recent-first. An empty query matches all records in
// scope. topK <= 0 means no limit.
//
// Scoring mirrors the in-memory store: a prefix match scores 1.0, any other
// substring match scores 0.5, and an empty query scores 1.0.
func (s *Store) Search(ctx context.Context, orgID, threadID, userID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error) {
	var (
		clauses = []string{"org_id = $1"}
		args    = []any{orgID}
	)
	add := func(clause string, arg any) {
		args = append(args, arg)
		clauses = append(clauses, fmt.Sprintf(clause, len(args)))
	}
	if userID != "" {
		// Own user-scoped memories plus everything owned by nobody
		// (org/workspace/policy). Mirrors session-core's durable rule
		// `scope = 'user' AND owner = $3` so the two tiers agree on visibility
		// instead of the semantic one being wider.
		add("(user_id = $%d OR user_id = '')", userID)
	}
	if threadID != "" {
		add("thread_id = $%d", threadID)
	}
	if len(topicFilter) > 0 {
		add("topic = ANY($%d)", topicFilter)
	}
	if !updatedAfter.IsZero() {
		add("updated_at >= $%d", updatedAfter.UTC())
	}
	q := strings.TrimSpace(query)
	if q != "" {
		// Escape ILIKE wildcards so the query is treated as a literal substring.
		esc := escapeLike(q)
		add("content ILIKE $%d ESCAPE '\\'", "%"+esc+"%")
	}

	sql := "SELECT memory_id, thread_id, topic, content, updated_at FROM letta_memory_blocks WHERE " +
		strings.Join(clauses, " AND ") + " ORDER BY updated_at DESC"
	if topK > 0 {
		args = append(args, topK)
		sql += fmt.Sprintf(" LIMIT $%d", len(args))
	}

	rows, err := s.pool.Query(ctx, sql, args...)
	if err != nil {
		return nil, fmt.Errorf("pgstore search: %w", err)
	}
	defer rows.Close()

	lowerQ := strings.ToLower(q)
	hits := make([]memstore.Hit, 0)
	for rows.Next() {
		var (
			memoryID, threadOut, topic, content string
			updatedAt                           time.Time
		)
		if err := rows.Scan(&memoryID, &threadOut, &topic, &content, &updatedAt); err != nil {
			return nil, fmt.Errorf("pgstore scan: %w", err)
		}
		score := float32(1.0)
		if lowerQ != "" && !strings.HasPrefix(strings.ToLower(content), lowerQ) {
			score = 0.5
		}
		hits = append(hits, memstore.Hit{
			MemoryID:  memoryID,
			ThreadID:  threadOut,
			Topic:     topic,
			Score:     score,
			Content:   content,
			UpdatedAt: updatedAt.UTC(),
		})
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("pgstore rows: %w", err)
	}
	return hits, nil
}

// escapeLike escapes the ILIKE metacharacters (% _ \) so a user query is
// matched as a literal substring rather than a pattern. The backslash is the
// ESCAPE character declared in the query.
func escapeLike(s string) string {
	r := strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`)
	return r.Replace(s)
}

// List always returns an empty result. The table now has a user_id column
// (see Put), which is what makes Delete below safe to implement for real,
// but List remains conservatively unimplemented rather than adding a real
// enumeration query as a side effect of the DSAR erasure fix -- broadening
// List to a proper per-user query is a separate, lower-stakes enhancement.
// The durable, correctly user-scoped source of truth is session-core's own
// `agent_memory` table; this tier is a supplementary semantic/lexical layer.
func (s *Store) List(_ context.Context, _, _ string, _ int32) ([]memstore.Hit, error) {
	return nil, nil
}

// Delete removes the record matching orgID + memoryID whose user_id equals
// userID, returning whether a row was actually removed. There is no
// threadID parameter (the Store interface's Delete doesn't take one), so
// this matches any thread within the org. A wrong userID or a nonexistent
// memoryID both report (false, nil) -- see memstore.Store.Delete's doc
// comment for why that ambiguity is intentional, not a shortcut.
func (s *Store) Delete(ctx context.Context, orgID, userID, memoryID string) (bool, error) {
	if orgID == "" || userID == "" || memoryID == "" {
		return false, nil
	}
	const q = `DELETE FROM letta_memory_blocks WHERE org_id = $1 AND memory_id = $2 AND user_id = $3`
	tag, err := s.pool.Exec(ctx, q, orgID, memoryID, userID)
	if err != nil {
		return false, fmt.Errorf("pgstore delete: %w", err)
	}
	return tag.RowsAffected() > 0, nil
}
