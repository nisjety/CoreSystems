package search

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"github.com/triodelab/quarry/internal/config"
)

type AsyncRun struct {
	ID        string    `json:"id"`
	Query     string    `json:"query"`
	CreatedAt time.Time `json:"createdAt"`
	UpdatedAt time.Time `json:"updatedAt"`
	ExpiresAt time.Time `json:"expiresAt"`
	Completed int       `json:"completed"`
	Total     int       `json:"total"`
}

type StoredResult struct {
	Title   string  `json:"title"`
	URL     string  `json:"url"`
	Snippet string  `json:"snippet"`
	Source  string  `json:"source"`
	Type    string  `json:"type"`
	Content string  `json:"content,omitempty"`
	Score   float64 `json:"score,omitempty"`
}

type AsyncStore interface {
	CreateRun(context.Context, *AsyncRun) error
	GetRun(context.Context, string) (*AsyncRun, error)
	SetRun(context.Context, *AsyncRun) error
	ReplaceResults(context.Context, string, []StoredResult) error
	ListResults(context.Context, string, int, int) ([]StoredResult, int, error)
	Close() error
}

type asyncStoreBackend string

const (
	asyncBackendMemory   asyncStoreBackend = "memory"
	asyncBackendRedis    asyncStoreBackend = "redis"
	asyncBackendPostgres asyncStoreBackend = "postgres"
)

func NewAsyncStore(cfg *config.Config, ttl time.Duration) AsyncStore {
	if ttl <= 0 {
		ttl = time.Hour
	}

	selected := strings.ToLower(strings.TrimSpace(os.Getenv("JOB_STORE_BACKEND")))
	if cfg != nil && strings.TrimSpace(cfg.JobStoreBackend) != "" {
		selected = strings.ToLower(strings.TrimSpace(cfg.JobStoreBackend))
	}

	switch asyncStoreBackend(selected) {
	case asyncBackendRedis:
		redisURL := ""
		if cfg != nil {
			redisURL = cfg.RedisURL
		}
		if redisURL == "" {
			redisURL = os.Getenv("REDIS_URL")
		}
		if redisURL != "" {
			opts, err := redis.ParseURL(redisURL)
			if err == nil {
				client := redis.NewClient(opts)
				if pingErr := client.Ping(context.Background()).Err(); pingErr == nil {
					return &redisAsyncStore{client: client, ttl: ttl}
				}
				_ = client.Close()
			}
		}
	case asyncBackendPostgres:
		dsn := ""
		if cfg != nil {
			dsn = strings.TrimSpace(cfg.PostgresDSN)
		}
		if dsn == "" {
			dsn = strings.TrimSpace(os.Getenv("QUARRY_POSTGRES_DSN"))
		}
		if dsn == "" {
			dsn = strings.TrimSpace(os.Getenv("DATABASE_URL"))
		}
		if dsn != "" {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			pool, err := pgxpool.New(ctx, dsn)
			if err == nil {
				if pingErr := pool.Ping(ctx); pingErr == nil {
					if ensureErr := ensureAsyncStoreSchema(ctx, pool); ensureErr == nil {
						return &postgresAsyncStore{pool: pool, ttl: ttl}
					}
				}
				pool.Close()
			}
		}
	}

	return NewMemoryAsyncStore(ttl)
}

type memoryAsyncStore struct {
	mu      sync.RWMutex
	ttl     time.Duration
	runs    map[string]*AsyncRun
	results map[string][]StoredResult
	done    chan struct{}
	once    sync.Once
}

func NewMemoryAsyncStore(ttl time.Duration) AsyncStore {
	if ttl <= 0 {
		ttl = time.Hour
	}
	store := &memoryAsyncStore{
		ttl:     ttl,
		runs:    make(map[string]*AsyncRun),
		results: make(map[string][]StoredResult),
		done:    make(chan struct{}),
	}
	go store.cleanupExpired()
	return store
}

func (s *memoryAsyncStore) CreateRun(_ context.Context, run *AsyncRun) error {
	if run == nil || run.ID == "" {
		return fmt.Errorf("run id is required")
	}
	now := time.Now().UTC()
	stored := cloneAsyncRun(run)
	if stored.CreatedAt.IsZero() {
		stored.CreatedAt = now
	}
	stored.UpdatedAt = now
	if stored.ExpiresAt.IsZero() || !stored.ExpiresAt.After(now) {
		stored.ExpiresAt = now.Add(s.ttl)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runs[stored.ID] = stored
	return nil
}

func (s *memoryAsyncStore) GetRun(_ context.Context, id string) (*AsyncRun, error) {
	now := time.Now().UTC()
	s.mu.Lock()
	defer s.mu.Unlock()
	s.cleanupExpiredLocked(now)
	run, ok := s.runs[id]
	if !ok {
		return nil, fmt.Errorf("run not found")
	}
	return cloneAsyncRun(run), nil
}

func (s *memoryAsyncStore) SetRun(_ context.Context, run *AsyncRun) error {
	if run == nil || run.ID == "" {
		return fmt.Errorf("run id is required")
	}
	now := time.Now().UTC()
	stored := cloneAsyncRun(run)
	stored.UpdatedAt = now
	if stored.ExpiresAt.IsZero() || !stored.ExpiresAt.After(now) {
		stored.ExpiresAt = now.Add(s.ttl)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runs[stored.ID] = stored
	return nil
}

func (s *memoryAsyncStore) ReplaceResults(_ context.Context, runID string, results []StoredResult) error {
	if runID == "" {
		return fmt.Errorf("run id is required")
	}
	cloned := cloneStoredResults(results)
	s.mu.Lock()
	defer s.mu.Unlock()
	s.results[runID] = cloned
	return nil
}

func (s *memoryAsyncStore) ListResults(_ context.Context, runID string, skip, limit int) ([]StoredResult, int, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	items := s.results[runID]
	return paginateStoredResults(items, skip, limit), len(items), nil
}

func (s *memoryAsyncStore) Close() error {
	s.once.Do(func() {
		close(s.done)
	})
	return nil
}

func (s *memoryAsyncStore) cleanupExpired() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			s.mu.Lock()
			s.cleanupExpiredLocked(time.Now().UTC())
			s.mu.Unlock()
		case <-s.done:
			return
		}
	}
}

func (s *memoryAsyncStore) cleanupExpiredLocked(now time.Time) {
	for id, run := range s.runs {
		if !run.ExpiresAt.After(now) {
			delete(s.runs, id)
			delete(s.results, id)
		}
	}
}

type redisAsyncStore struct {
	client *redis.Client
	ttl    time.Duration
}

func (s *redisAsyncStore) CreateRun(ctx context.Context, run *AsyncRun) error {
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run id is required")
	}
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	pipe := s.client.TxPipeline()
	pipe.Set(ctx, asyncRunKey(run.ID), payload, s.ttl)
	pipe.Del(ctx, asyncResultsKey(run.ID))
	pipe.Expire(ctx, asyncResultsKey(run.ID), s.ttl)
	_, err = pipe.Exec(ctx)
	return err
}

func (s *redisAsyncStore) GetRun(ctx context.Context, id string) (*AsyncRun, error) {
	payload, err := s.client.Get(ctx, asyncRunKey(id)).Bytes()
	if err != nil {
		return nil, fmt.Errorf("run not found")
	}
	var run AsyncRun
	if err := json.Unmarshal(payload, &run); err != nil {
		return nil, err
	}
	return cloneAsyncRun(&run), nil
}

func (s *redisAsyncStore) SetRun(ctx context.Context, run *AsyncRun) error {
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run id is required")
	}
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	return s.client.Set(ctx, asyncRunKey(run.ID), payload, s.ttl).Err()
}

func (s *redisAsyncStore) ReplaceResults(ctx context.Context, runID string, results []StoredResult) error {
	if strings.TrimSpace(runID) == "" {
		return fmt.Errorf("run id is required")
	}
	pipe := s.client.TxPipeline()
	pipe.Del(ctx, asyncResultsKey(runID))
	if len(results) > 0 {
		values := make([]interface{}, 0, len(results))
		for _, result := range results {
			payload, err := json.Marshal(result)
			if err != nil {
				return err
			}
			values = append(values, payload)
		}
		pipe.RPush(ctx, asyncResultsKey(runID), values...)
	}
	pipe.Expire(ctx, asyncResultsKey(runID), s.ttl)
	_, err := pipe.Exec(ctx)
	return err
}

func (s *redisAsyncStore) ListResults(ctx context.Context, runID string, skip, limit int) ([]StoredResult, int, error) {
	total, err := s.client.LLen(ctx, asyncResultsKey(runID)).Result()
	if err != nil {
		return nil, 0, err
	}
	start, end := redisRangeBounds(skip, limit, int(total))
	if start >= int(total) {
		return []StoredResult{}, int(total), nil
	}
	items, err := s.client.LRange(ctx, asyncResultsKey(runID), int64(start), int64(end)).Result()
	if err != nil {
		return nil, 0, err
	}
	results := make([]StoredResult, 0, len(items))
	for _, item := range items {
		var decoded StoredResult
		if err := json.Unmarshal([]byte(item), &decoded); err != nil {
			return nil, 0, err
		}
		results = append(results, decoded)
	}
	return results, int(total), nil
}

func (s *redisAsyncStore) Close() error {
	if s.client == nil {
		return nil
	}
	return s.client.Close()
}

type postgresAsyncStore struct {
	pool *pgxpool.Pool
	ttl  time.Duration
}

func (s *postgresAsyncStore) CreateRun(ctx context.Context, run *AsyncRun) error {
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run id is required")
	}
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO quarry_search_runs (id, query, payload, expires_at, updated_at)
		VALUES ($1, $2, $3, $4, NOW())
		ON CONFLICT (id)
		DO UPDATE SET
			query = EXCLUDED.query,
			payload = EXCLUDED.payload,
			expires_at = EXCLUDED.expires_at,
			updated_at = NOW()
	`, run.ID, run.Query, payload, run.ExpiresAt)
	return err
}

func (s *postgresAsyncStore) GetRun(ctx context.Context, id string) (*AsyncRun, error) {
	var payload []byte
	err := s.pool.QueryRow(ctx, `
		SELECT payload
		FROM quarry_search_runs
		WHERE id = $1 AND expires_at > NOW()
	`, id).Scan(&payload)
	if err != nil {
		return nil, fmt.Errorf("run not found")
	}
	var run AsyncRun
	if err := json.Unmarshal(payload, &run); err != nil {
		return nil, err
	}
	return cloneAsyncRun(&run), nil
}

func (s *postgresAsyncStore) SetRun(ctx context.Context, run *AsyncRun) error {
	return s.CreateRun(ctx, run)
}

func (s *postgresAsyncStore) ReplaceResults(ctx context.Context, runID string, results []StoredResult) error {
	if strings.TrimSpace(runID) == "" {
		return fmt.Errorf("run id is required")
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() {
		_ = tx.Rollback(ctx)
	}()

	if _, err := tx.Exec(ctx, `DELETE FROM quarry_search_results WHERE run_id = $1`, runID); err != nil {
		return err
	}
	for index, result := range results {
		payload, err := json.Marshal(result)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `
			INSERT INTO quarry_search_results (run_id, position, payload)
			VALUES ($1, $2, $3)
		`, runID, index, payload); err != nil {
			return err
		}
	}
	return tx.Commit(ctx)
}

func (s *postgresAsyncStore) ListResults(ctx context.Context, runID string, skip, limit int) ([]StoredResult, int, error) {
	var total int
	if err := s.pool.QueryRow(ctx, `SELECT COUNT(*) FROM quarry_search_results WHERE run_id = $1`, runID).Scan(&total); err != nil {
		return nil, 0, err
	}
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = total
	}
	rows, err := s.pool.Query(ctx, `
		SELECT payload
		FROM quarry_search_results
		WHERE run_id = $1
		ORDER BY position ASC
		OFFSET $2 LIMIT $3
	`, runID, skip, limit)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()

	results := make([]StoredResult, 0)
	for rows.Next() {
		var payload []byte
		if err := rows.Scan(&payload); err != nil {
			return nil, 0, err
		}
		var decoded StoredResult
		if err := json.Unmarshal(payload, &decoded); err != nil {
			return nil, 0, err
		}
		results = append(results, decoded)
	}
	if err := rows.Err(); err != nil {
		return nil, 0, err
	}
	return results, total, nil
}

func (s *postgresAsyncStore) Close() error {
	if s.pool != nil {
		s.pool.Close()
	}
	return nil
}

func ensureAsyncStoreSchema(ctx context.Context, pool *pgxpool.Pool) error {
	_, err := pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS quarry_search_runs (
			id TEXT PRIMARY KEY,
			query TEXT NOT NULL,
			payload JSONB NOT NULL,
			expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
			updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
		);
		CREATE TABLE IF NOT EXISTS quarry_search_results (
			run_id TEXT NOT NULL,
			position INTEGER NOT NULL,
			payload JSONB NOT NULL,
			PRIMARY KEY (run_id, position)
		);
		CREATE INDEX IF NOT EXISTS idx_quarry_search_runs_expires_at ON quarry_search_runs(expires_at);
		CREATE INDEX IF NOT EXISTS idx_quarry_search_results_run_id ON quarry_search_results(run_id, position);
	`)
	return err
}

func asyncRunKey(id string) string {
	return "search:run:" + id
}

func asyncResultsKey(id string) string {
	return "search:results:" + id
}

func redisRangeBounds(skip, limit, total int) (int, int) {
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = total
	}
	if total <= 0 || skip >= total {
		return total, total - 1
	}
	end := skip + limit - 1
	if end >= total {
		end = total - 1
	}
	return skip, end
}

func paginateStoredResults(items []StoredResult, skip, limit int) []StoredResult {
	total := len(items)
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = total
	}
	if skip > total {
		skip = total
	}
	end := skip + limit
	if end > total {
		end = total
	}
	return cloneStoredResults(items[skip:end])
}

func cloneAsyncRun(run *AsyncRun) *AsyncRun {
	if run == nil {
		return nil
	}
	clone := *run
	return &clone
}

func cloneStoredResults(results []StoredResult) []StoredResult {
	if len(results) == 0 {
		return []StoredResult{}
	}
	encoded, err := json.Marshal(results)
	if err != nil {
		out := make([]StoredResult, len(results))
		copy(out, results)
		return out
	}
	var out []StoredResult
	if err := json.Unmarshal(encoded, &out); err != nil {
		out = make([]StoredResult, len(results))
		copy(out, results)
	}
	return out
}
