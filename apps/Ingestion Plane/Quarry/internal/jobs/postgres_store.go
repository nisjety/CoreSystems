package jobs

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresExtractionStore provides a PostgreSQL-backed extraction store.
// The store owns the pool passed to it in Quarry's current wiring, so Close()
// releases the pool during API shutdown.
type PostgresExtractionStore struct {
	pool      *pgxpool.Pool
	ttl       time.Duration
	closeOnce sync.Once
}

// NewPostgresExtractionStore creates a new PostgreSQL-backed extraction store.
func NewPostgresExtractionStore(pool *pgxpool.Pool, ttl time.Duration) ExtractionStore {
	if ttl <= 0 {
		ttl = time.Hour
	}
	return &PostgresExtractionStore{pool: pool, ttl: ttl}
}

// NewPostgresJobStore preserves the previous constructor name used by handler setup.
func NewPostgresJobStore(pool *pgxpool.Pool, ttl time.Duration) ExtractionStore {
	return NewPostgresExtractionStore(pool, ttl)
}

type extractionPayload struct {
	URL      string                 `json:"url"`
	Schema   string                 `json:"schema,omitempty"`
	Prompt   string                 `json:"prompt,omitempty"`
	Result   map[string]interface{} `json:"result,omitempty"`
	Error    string                 `json:"error,omitempty"`
	Duration int64                  `json:"duration_ms,omitempty"`
}

// Create stores a new extraction job in the database.
func (s *PostgresExtractionStore) Create(ctx context.Context, job *ExtractionJob) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if job == nil || job.ID == "" || strings.TrimSpace(job.URL) == "" {
		return ErrInvalidID
	}
	if s.pool == nil {
		return fmt.Errorf("postgres extraction store is not initialized")
	}

	now := time.Now()
	stored := cloneExtractionJob(job)
	if stored.Status == "" {
		stored.Status = ExtractionQueued
	}
	if stored.CreatedAt.IsZero() {
		stored.CreatedAt = now
	}
	stored.UpdatedAt = now
	if stored.ExpiresAt.IsZero() || !stored.ExpiresAt.After(now) {
		stored.ExpiresAt = now.Add(s.ttl)
	}

	payloadJSON, err := json.Marshal(extractionPayload{
		URL:      stored.URL,
		Schema:   stored.Schema,
		Prompt:   stored.Prompt,
		Result:   stored.Result,
		Error:    stored.Error,
		Duration: stored.Duration,
	})
	if err != nil {
		return fmt.Errorf("marshal payload: %w", err)
	}

	query := `
		INSERT INTO quarry_jobs (id, status, payload, created_at, updated_at, expires_at)
		VALUES ($1, $2, $3, $4, $5, $6)
		ON CONFLICT (id) DO NOTHING
	`

	tag, err := s.pool.Exec(ctx, query,
		stored.ID,
		string(stored.Status),
		payloadJSON,
		stored.CreatedAt,
		stored.UpdatedAt,
		stored.ExpiresAt,
	)
	if err != nil {
		return fmt.Errorf("insert job: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return ErrJobExists
	}

	return nil
}

// Get retrieves an extraction job by ID.
func (s *PostgresExtractionStore) Get(ctx context.Context, jobID string) (*ExtractionJob, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if s.pool == nil {
		return nil, fmt.Errorf("postgres extraction store is not initialized")
	}

	query := `
		SELECT id, status, payload, created_at, updated_at, expires_at
		FROM quarry_jobs
		WHERE id = $1
	`

	var id, status string
	var payload []byte
	var createdAt, updatedAt, expiresAt time.Time

	err := s.pool.QueryRow(ctx, query, jobID).
		Scan(&id, &status, &payload, &createdAt, &updatedAt, &expiresAt)
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, ErrJobNotFound
		}
		return nil, fmt.Errorf("query job: %w", err)
	}

	if !expiresAt.After(time.Now()) {
		_ = s.Delete(context.Background(), jobID)
		return nil, ErrJobNotFound
	}

	var stored extractionPayload
	if err := json.Unmarshal(payload, &stored); err != nil {
		return nil, fmt.Errorf("unmarshal payload: %w", err)
	}

	job := &ExtractionJob{
		ID:        id,
		Status:    ExtractionStatus(status),
		CreatedAt: createdAt,
		UpdatedAt: updatedAt,
		ExpiresAt: expiresAt,
		URL:       stored.URL,
		Schema:    stored.Schema,
		Prompt:    stored.Prompt,
		Result:    cloneJSONMap(stored.Result),
		Error:     stored.Error,
		Duration:  stored.Duration,
	}

	return job, nil
}

// Update updates an existing extraction job.
func (s *PostgresExtractionStore) Update(ctx context.Context, job *ExtractionJob) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if job == nil || job.ID == "" {
		return ErrInvalidID
	}
	if s.pool == nil {
		return fmt.Errorf("postgres extraction store is not initialized")
	}

	current, err := s.Get(ctx, job.ID)
	if err != nil {
		return err
	}

	updated := cloneExtractionJob(job)
	updated.CreatedAt = current.CreatedAt
	updated.UpdatedAt = time.Now()
	updated.ExpiresAt = updated.UpdatedAt.Add(s.ttl)
	if updated.Status == ExtractionCompleted || updated.Status == ExtractionFailed || updated.Status == ExtractionCancelled {
		updated.Duration = updated.UpdatedAt.Sub(updated.CreatedAt).Milliseconds()
	}

	payload := extractionPayload{
		URL:      updated.URL,
		Schema:   updated.Schema,
		Prompt:   updated.Prompt,
		Result:   updated.Result,
		Error:    updated.Error,
		Duration: updated.Duration,
	}
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal payload: %w", err)
	}

	query := `
		UPDATE quarry_jobs
		SET status = $1, payload = $2, updated_at = $3, expires_at = $4
		WHERE id = $5
	`

	cmdTag, err := s.pool.Exec(ctx, query,
		string(updated.Status),
		payloadJSON,
		updated.UpdatedAt,
		updated.ExpiresAt,
		updated.ID,
	)
	if err != nil {
		return fmt.Errorf("update job: %w", err)
	}

	if cmdTag.RowsAffected() == 0 {
		return ErrJobNotFound
	}

	return nil
}

// Delete removes an extraction job.
func (s *PostgresExtractionStore) Delete(ctx context.Context, jobID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if s.pool == nil {
		return fmt.Errorf("postgres extraction store is not initialized")
	}
	query := `DELETE FROM quarry_jobs WHERE id = $1`

	cmdTag, err := s.pool.Exec(ctx, query, jobID)
	if err != nil {
		return fmt.Errorf("delete job: %w", err)
	}

	if cmdTag.RowsAffected() == 0 {
		return ErrJobNotFound
	}

	return nil
}

// List returns recent extraction jobs (up to limit).
func (s *PostgresExtractionStore) List(ctx context.Context, limit int) ([]*ExtractionJob, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if s.pool == nil {
		return nil, fmt.Errorf("postgres extraction store is not initialized")
	}
	if limit <= 0 || limit > 100 {
		limit = 10
	}

	query := `
		SELECT id, status, payload, created_at, updated_at, expires_at
		FROM quarry_jobs
		WHERE id LIKE 'extract_%'
		ORDER BY updated_at DESC
		LIMIT $1
	`

	rows, err := s.pool.Query(ctx, query, limit)
	if err != nil {
		return nil, fmt.Errorf("query jobs: %w", err)
	}
	defer rows.Close()

	var jobs []*ExtractionJob
	for rows.Next() {
		var id, status string
		var payload []byte
		var createdAt, updatedAt, expiresAt time.Time

		if err := rows.Scan(&id, &status, &payload, &createdAt, &updatedAt, &expiresAt); err != nil {
			return nil, fmt.Errorf("scan row: %w", err)
		}
		if !expiresAt.After(time.Now()) {
			_ = s.Delete(context.Background(), id)
			continue
		}

		var stored extractionPayload
		if err := json.Unmarshal(payload, &stored); err != nil {
			return nil, fmt.Errorf("unmarshal payload: %w", err)
		}

		job := &ExtractionJob{
			ID:        id,
			Status:    ExtractionStatus(status),
			CreatedAt: createdAt,
			UpdatedAt: updatedAt,
			ExpiresAt: expiresAt,
			URL:       stored.URL,
			Schema:    stored.Schema,
			Prompt:    stored.Prompt,
			Result:    cloneJSONMap(stored.Result),
			Error:     stored.Error,
			Duration:  stored.Duration,
		}

		jobs = append(jobs, job)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("rows error: %w", err)
	}

	return jobs, nil
}

// Close releases the owned PostgreSQL pool.
func (s *PostgresExtractionStore) Close() error {
	s.closeOnce.Do(func() {
		if s.pool != nil {
			s.pool.Close()
			s.pool = nil
		}
	})
	return nil
}
