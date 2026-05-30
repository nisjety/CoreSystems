package pipeline

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// JobStorePersister writes pipeline results into the job store (PostgreSQL or in-memory).
type JobStorePersister struct {
	pool *pgxpool.Pool
}

// NewJobStorePersister creates a persister backed by PostgreSQL.
// If pool is nil, PersistResult is a no-op (graceful degradation).
func NewJobStorePersister(pool *pgxpool.Pool) *JobStorePersister {
	return &JobStorePersister{pool: pool}
}

func (p *JobStorePersister) PersistResult(ctx context.Context, jobID string, result map[string]any, meta map[string]string) error {
	if p.pool == nil {
		return nil // graceful degradation — no persistent store configured
	}

	resultJSON, err := json.Marshal(result)
	if err != nil {
		return fmt.Errorf("marshal result: %w", err)
	}
	metaJSON, err := json.Marshal(meta)
	if err != nil {
		return fmt.Errorf("marshal meta: %w", err)
	}

	const upsert = `
		INSERT INTO job_results (job_id, result, meta, updated_at)
		VALUES ($1, $2, $3, $4)
		ON CONFLICT (job_id) DO UPDATE SET
			result = EXCLUDED.result,
			meta = EXCLUDED.meta,
			updated_at = EXCLUDED.updated_at`

	_, err = p.pool.Exec(ctx, upsert, jobID, resultJSON, metaJSON, time.Now().UTC())
	if err != nil {
		return fmt.Errorf("upsert job_results: %w", err)
	}
	return nil
}
