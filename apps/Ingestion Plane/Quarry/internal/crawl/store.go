package crawl

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
	zlog "github.com/rs/zerolog/log"
	"github.com/triodelab/quarry/internal/config"
)

type Store interface {
	CreateRun(context.Context, *Run) error
	GetRun(context.Context, string) (*Run, error)
	SetRun(context.Context, *Run) error
	AppendDocument(context.Context, string, *Document) error
	ListDocuments(context.Context, string, int, int) ([]*Document, int, error)
	AppendError(context.Context, string, *PageError) error
	ListErrors(context.Context, string) ([]*PageError, error)
	AddRobotsBlocked(context.Context, string, []string) error
	ListRobotsBlocked(context.Context, string) ([]string, error)
	Close() error
}

type backendType string

const (
	backendMemory   backendType = "memory"
	backendRedis    backendType = "redis"
	backendPostgres backendType = "postgres"
)

func NewStore(cfg *config.Config, ttl time.Duration) Store {
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}

	selected := strings.ToLower(strings.TrimSpace(os.Getenv("JOB_STORE_BACKEND")))
	if cfg != nil && strings.TrimSpace(cfg.JobStoreBackend) != "" {
		selected = strings.ToLower(strings.TrimSpace(cfg.JobStoreBackend))
	}

	switch selected {
	case string(backendRedis):
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
					return &redisStore{client: client, ttl: ttl}
				}
				_ = client.Close()
			}
		}
	case string(backendPostgres):
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
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			pool, err := pgxpool.New(ctx, dsn)
			if err == nil {
				if pingErr := pool.Ping(ctx); pingErr == nil {
					if ensureErr := ensurePostgresSchema(ctx, pool); ensureErr == nil {
						zlog.Info().Msg("crawl store: using PostgreSQL backend")
						return &postgresStore{pool: pool, ttl: ttl}
					} else {
						zlog.Error().Err(ensureErr).Msg("crawl store: ensurePostgresSchema failed, falling back to memory")
					}
				} else {
					zlog.Error().Err(pingErr).Msg("crawl store: postgres ping failed, falling back to memory")
				}
				pool.Close()
			} else {
				zlog.Error().Err(err).Msg("crawl store: pgxpool.New failed, falling back to memory")
			}
		} else {
			zlog.Warn().Msg("crawl store: JOB_STORE_BACKEND=postgres but no DSN configured, falling back to memory")
		}
	}

	return NewMemoryStore(ttl)
}

type memoryStore struct {
	mu      sync.RWMutex
	ttl     time.Duration
	runs    map[string]*Run
	docs    map[string][]*Document
	errors  map[string][]*PageError
	blocked map[string]map[string]struct{}
}

func NewMemoryStore(ttl time.Duration) Store {
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}
	return &memoryStore{
		ttl:     ttl,
		runs:    make(map[string]*Run),
		docs:    make(map[string][]*Document),
		errors:  make(map[string][]*PageError),
		blocked: make(map[string]map[string]struct{}),
	}
}

func (s *memoryStore) CreateRun(_ context.Context, run *Run) error {
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run id is required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runs[run.ID] = cloneRun(run)
	return nil
}

func (s *memoryStore) GetRun(_ context.Context, id string) (*Run, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	run, ok := s.runs[id]
	if !ok {
		return nil, fmt.Errorf("run not found")
	}
	return cloneRun(run), nil
}

func (s *memoryStore) SetRun(_ context.Context, run *Run) error {
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run id is required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runs[run.ID] = cloneRun(run)
	return nil
}

func (s *memoryStore) AppendDocument(_ context.Context, runID string, doc *Document) error {
	if doc == nil {
		return fmt.Errorf("document is nil")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.docs[runID] = append(s.docs[runID], cloneDocument(doc))
	return nil
}

func (s *memoryStore) ListDocuments(_ context.Context, runID string, skip, limit int) ([]*Document, int, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	items := s.docs[runID]
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
	result := make([]*Document, 0, end-skip)
	for _, item := range items[skip:end] {
		result = append(result, cloneDocument(item))
	}
	return result, total, nil
}

func (s *memoryStore) AppendError(_ context.Context, runID string, pageErr *PageError) error {
	if pageErr == nil {
		return fmt.Errorf("page error is nil")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.errors[runID] = append(s.errors[runID], clonePageError(pageErr))
	return nil
}

func (s *memoryStore) ListErrors(_ context.Context, runID string) ([]*PageError, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	items := s.errors[runID]
	result := make([]*PageError, 0, len(items))
	for _, item := range items {
		result = append(result, clonePageError(item))
	}
	return result, nil
}

func (s *memoryStore) AddRobotsBlocked(_ context.Context, runID string, urls []string) error {
	if len(urls) == 0 {
		return nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	current := s.blocked[runID]
	if current == nil {
		current = make(map[string]struct{}, len(urls))
	}
	for _, item := range urls {
		trimmed := strings.TrimSpace(item)
		if trimmed == "" {
			continue
		}
		current[trimmed] = struct{}{}
	}
	s.blocked[runID] = current
	return nil
}

func (s *memoryStore) ListRobotsBlocked(_ context.Context, runID string) ([]string, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	current := s.blocked[runID]
	result := make([]string, 0, len(current))
	for item := range current {
		result = append(result, item)
	}
	sort.Strings(result)
	return result, nil
}

func (s *memoryStore) Close() error {
	return nil
}

type redisStore struct {
	client *redis.Client
	ttl    time.Duration
}

func (s *redisStore) CreateRun(ctx context.Context, run *Run) error {
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	pipe := s.client.TxPipeline()
	pipe.Set(ctx, runKey(run.ID), payload, s.ttl)
	pipe.Expire(ctx, docsKey(run.ID), s.ttl)
	pipe.Expire(ctx, errorsKey(run.ID), s.ttl)
	pipe.Expire(ctx, blockedKey(run.ID), s.ttl)
	_, err = pipe.Exec(ctx)
	return err
}

func (s *redisStore) GetRun(ctx context.Context, id string) (*Run, error) {
	payload, err := s.client.Get(ctx, runKey(id)).Bytes()
	if err != nil {
		return nil, err
	}
	var run Run
	if err := json.Unmarshal(payload, &run); err != nil {
		return nil, err
	}
	return &run, nil
}

func (s *redisStore) SetRun(ctx context.Context, run *Run) error {
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	return s.client.Set(ctx, runKey(run.ID), payload, s.ttl).Err()
}

func (s *redisStore) AppendDocument(ctx context.Context, runID string, doc *Document) error {
	payload, err := json.Marshal(doc)
	if err != nil {
		return err
	}
	pipe := s.client.TxPipeline()
	pipe.RPush(ctx, docsKey(runID), payload)
	pipe.Expire(ctx, docsKey(runID), s.ttl)
	_, err = pipe.Exec(ctx)
	return err
}

func (s *redisStore) ListDocuments(ctx context.Context, runID string, skip, limit int) ([]*Document, int, error) {
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = 100
	}
	total64, err := s.client.LLen(ctx, docsKey(runID)).Result()
	if err != nil {
		return nil, 0, err
	}
	total := int(total64)
	end := skip + limit - 1
	values, err := s.client.LRange(ctx, docsKey(runID), int64(skip), int64(end)).Result()
	if err != nil {
		return nil, 0, err
	}
	result := make([]*Document, 0, len(values))
	for _, value := range values {
		var doc Document
		if err := json.Unmarshal([]byte(value), &doc); err != nil {
			return nil, 0, err
		}
		result = append(result, &doc)
	}
	return result, total, nil
}

func (s *redisStore) AppendError(ctx context.Context, runID string, pageErr *PageError) error {
	payload, err := json.Marshal(pageErr)
	if err != nil {
		return err
	}
	pipe := s.client.TxPipeline()
	pipe.RPush(ctx, errorsKey(runID), payload)
	pipe.Expire(ctx, errorsKey(runID), s.ttl)
	_, err = pipe.Exec(ctx)
	return err
}

func (s *redisStore) ListErrors(ctx context.Context, runID string) ([]*PageError, error) {
	values, err := s.client.LRange(ctx, errorsKey(runID), 0, -1).Result()
	if err != nil {
		return nil, err
	}
	result := make([]*PageError, 0, len(values))
	for _, value := range values {
		var pageErr PageError
		if err := json.Unmarshal([]byte(value), &pageErr); err != nil {
			return nil, err
		}
		result = append(result, &pageErr)
	}
	return result, nil
}

func (s *redisStore) AddRobotsBlocked(ctx context.Context, runID string, urls []string) error {
	if len(urls) == 0 {
		return nil
	}
	values := make([]interface{}, 0, len(urls))
	for _, item := range urls {
		values = append(values, item)
	}
	pipe := s.client.TxPipeline()
	pipe.SAdd(ctx, blockedKey(runID), values...)
	pipe.Expire(ctx, blockedKey(runID), s.ttl)
	_, err := pipe.Exec(ctx)
	return err
}

func (s *redisStore) ListRobotsBlocked(ctx context.Context, runID string) ([]string, error) {
	values, err := s.client.SMembers(ctx, blockedKey(runID)).Result()
	if err != nil {
		return nil, err
	}
	sort.Strings(values)
	return values, nil
}

func (s *redisStore) Close() error {
	if s.client == nil {
		return nil
	}
	return s.client.Close()
}

type postgresStore struct {
	pool *pgxpool.Pool
	ttl  time.Duration
}

func (s *postgresStore) CreateRun(ctx context.Context, run *Run) error {
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO crawl_runs (id, status, payload, created_at, updated_at, expires_at)
		VALUES ($1, $2, $3, $4, $5, $6)
		ON CONFLICT (id) DO UPDATE SET
			status = EXCLUDED.status,
			payload = EXCLUDED.payload,
			updated_at = EXCLUDED.updated_at,
			expires_at = EXCLUDED.expires_at
	`, run.ID, string(run.Status), payload, run.CreatedAt, run.UpdatedAt, run.ExpiresAt)
	return err
}

func (s *postgresStore) GetRun(ctx context.Context, id string) (*Run, error) {
	var payload []byte
	if err := s.pool.QueryRow(ctx, `SELECT payload FROM crawl_runs WHERE id = $1`, id).Scan(&payload); err != nil {
		return nil, err
	}
	var run Run
	if err := json.Unmarshal(payload, &run); err != nil {
		return nil, err
	}
	return &run, nil
}

func (s *postgresStore) SetRun(ctx context.Context, run *Run) error {
	payload, err := json.Marshal(run)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `
		UPDATE crawl_runs
		SET status = $1, payload = $2, updated_at = $3, expires_at = $4
		WHERE id = $5
	`, string(run.Status), payload, run.UpdatedAt, run.ExpiresAt, run.ID)
	return err
}

func (s *postgresStore) AppendDocument(ctx context.Context, runID string, doc *Document) error {
	payload, err := json.Marshal(doc)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO crawl_documents (crawl_id, payload, created_at)
		VALUES ($1, $2, NOW())
	`, runID, payload)
	return err
}

func (s *postgresStore) ListDocuments(ctx context.Context, runID string, skip, limit int) ([]*Document, int, error) {
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = 100
	}

	var total int
	if err := s.pool.QueryRow(ctx, `SELECT COUNT(*) FROM crawl_documents WHERE crawl_id = $1`, runID).Scan(&total); err != nil {
		return nil, 0, err
	}

	rows, err := s.pool.Query(ctx, `
		SELECT payload
		FROM crawl_documents
		WHERE crawl_id = $1
		ORDER BY id ASC
		OFFSET $2
		LIMIT $3
	`, runID, skip, limit)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()

	result := make([]*Document, 0, limit)
	for rows.Next() {
		var payload []byte
		if err := rows.Scan(&payload); err != nil {
			return nil, 0, err
		}
		var doc Document
		if err := json.Unmarshal(payload, &doc); err != nil {
			return nil, 0, err
		}
		result = append(result, &doc)
	}
	if err := rows.Err(); err != nil {
		return nil, 0, err
	}
	return result, total, nil
}

func (s *postgresStore) AppendError(ctx context.Context, runID string, pageErr *PageError) error {
	payload, err := json.Marshal(pageErr)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO crawl_errors (crawl_id, payload, created_at)
		VALUES ($1, $2, NOW())
	`, runID, payload)
	return err
}

func (s *postgresStore) ListErrors(ctx context.Context, runID string) ([]*PageError, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT payload
		FROM crawl_errors
		WHERE crawl_id = $1
		ORDER BY id ASC
	`, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	result := make([]*PageError, 0)
	for rows.Next() {
		var payload []byte
		if err := rows.Scan(&payload); err != nil {
			return nil, err
		}
		var pageErr PageError
		if err := json.Unmarshal(payload, &pageErr); err != nil {
			return nil, err
		}
		result = append(result, &pageErr)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

func (s *postgresStore) AddRobotsBlocked(ctx context.Context, runID string, urls []string) error {
	for _, item := range urls {
		trimmed := strings.TrimSpace(item)
		if trimmed == "" {
			continue
		}
		if _, err := s.pool.Exec(ctx, `
			INSERT INTO crawl_blocked_urls (crawl_id, url, created_at)
			VALUES ($1, $2, NOW())
			ON CONFLICT (crawl_id, url) DO NOTHING
		`, runID, trimmed); err != nil {
			return err
		}
	}
	return nil
}

func (s *postgresStore) ListRobotsBlocked(ctx context.Context, runID string) ([]string, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT url
		FROM crawl_blocked_urls
		WHERE crawl_id = $1
		ORDER BY url ASC
	`, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	result := make([]string, 0)
	for rows.Next() {
		var url string
		if err := rows.Scan(&url); err != nil {
			return nil, err
		}
		result = append(result, url)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

func (s *postgresStore) Close() error {
	if s.pool != nil {
		s.pool.Close()
	}
	return nil
}

func ensurePostgresSchema(ctx context.Context, pool *pgxpool.Pool) error {
	_, err := pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS crawl_runs (
			id TEXT PRIMARY KEY,
			status TEXT NOT NULL,
			payload JSONB NOT NULL,
			created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
			updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
			expires_at TIMESTAMP WITH TIME ZONE NOT NULL
		);
		CREATE TABLE IF NOT EXISTS crawl_documents (
			id BIGSERIAL PRIMARY KEY,
			crawl_id TEXT NOT NULL,
			payload JSONB NOT NULL,
			created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
		);
		CREATE TABLE IF NOT EXISTS crawl_errors (
			id BIGSERIAL PRIMARY KEY,
			crawl_id TEXT NOT NULL,
			payload JSONB NOT NULL,
			created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
		);
		CREATE TABLE IF NOT EXISTS crawl_blocked_urls (
			crawl_id TEXT NOT NULL,
			url TEXT NOT NULL,
			created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
			PRIMARY KEY (crawl_id, url)
		);
		CREATE INDEX IF NOT EXISTS idx_crawl_runs_status ON crawl_runs(status);
		CREATE INDEX IF NOT EXISTS idx_crawl_documents_crawl_id ON crawl_documents(crawl_id, id);
		CREATE INDEX IF NOT EXISTS idx_crawl_errors_crawl_id ON crawl_errors(crawl_id, id);
	`)
	return err
}

func runKey(id string) string     { return "crawl:runs:" + id }
func docsKey(id string) string    { return "crawl:docs:" + id }
func errorsKey(id string) string  { return "crawl:errors:" + id }
func blockedKey(id string) string { return "crawl:blocked:" + id }

func cloneRun(run *Run) *Run {
	if run == nil {
		return nil
	}
	clone := *run
	if run.Meta != nil {
		payload, _ := json.Marshal(run.Meta)
		var meta map[string]interface{}
		_ = json.Unmarshal(payload, &meta)
		clone.Meta = meta
	}
	return &clone
}

func cloneDocument(doc *Document) *Document {
	if doc == nil {
		return nil
	}
	clone := *doc
	if doc.Metadata != nil {
		payload, _ := json.Marshal(doc.Metadata)
		var metadata map[string]interface{}
		_ = json.Unmarshal(payload, &metadata)
		clone.Metadata = metadata
	}
	if doc.Outputs != nil {
		payload, _ := json.Marshal(doc.Outputs)
		var outputs map[string]interface{}
		_ = json.Unmarshal(payload, &outputs)
		clone.Outputs = outputs
	}
	return &clone
}

func clonePageError(pageErr *PageError) *PageError {
	if pageErr == nil {
		return nil
	}
	clone := *pageErr
	return &clone
}
