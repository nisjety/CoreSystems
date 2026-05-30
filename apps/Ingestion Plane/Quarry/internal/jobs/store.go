package jobs

import (
	"context"
	"encoding/json"
	"os"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
)

type Status string

const (
	StatusPending   Status = "pending"
	StatusRunning   Status = "running"
	StatusReady     Status = "ready"
	StatusFailed    Status = "failed"
	StatusCancelled Status = "cancelled"
)

type Job struct {
	ID        string            `json:"id"`
	Status    Status            `json:"status"`
	CreatedAt time.Time         `json:"created_at"`
	UpdatedAt time.Time         `json:"updated_at"`
	ExpiresAt time.Time         `json:"expires_at"`
	Progress  int               `json:"progress"`
	Result    map[string]any    `json:"result,omitempty"`
	Error     string            `json:"error,omitempty"`
	Meta      map[string]string `json:"meta,omitempty"`
}

type Store struct {
	mu           sync.RWMutex
	jobs         map[string]*Job
	ttl          time.Duration
	backend      string
	redisClient  *redis.Client
	postgresPool *pgxpool.Pool
	done         chan struct{}
	closeOnce    sync.Once
}

func NewStore(ttl time.Duration) *Store {
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}

	store := &Store{
		jobs:    make(map[string]*Job),
		ttl:     ttl,
		backend: "memory",
		done:    make(chan struct{}),
	}

	if strings.EqualFold(os.Getenv("JOB_STORE_BACKEND"), "redis") {
		// Redis initialization
		redisURL := os.Getenv("REDIS_URL")
		if redisURL != "" {
			opts, err := redis.ParseURL(redisURL)
			if err == nil {
				store.redisClient = redis.NewClient(opts)
				if pingErr := store.redisClient.Ping(context.Background()).Err(); pingErr == nil {
					store.backend = "redis"
				}
			}
		}
	} else if strings.EqualFold(os.Getenv("JOB_STORE_BACKEND"), "postgres") {
		// Postgres initialization
		dsn := strings.TrimSpace(os.Getenv("QUARRY_POSTGRES_DSN"))
		if dsn == "" {
			dsn = strings.TrimSpace(os.Getenv("DATABASE_URL"))
		}
		if dsn != "" {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			// Use pgxpool for connection pooling and better performance
			pool, err := pgxpool.New(ctx, dsn)
			if err == nil {
				if pingErr := pool.Ping(ctx); pingErr == nil {
					if ensureErr := ensurePostgresSchema(ctx, pool); ensureErr == nil {
						store.postgresPool = pool
						store.backend = "postgres"
					} else {
						pool.Close()
					}
				} else {
					pool.Close()
				}
			}
		}
	}

	go store.cleanupExpired()
	return store
}

func (s *Store) Close() error {
	var closeErr error
	s.closeOnce.Do(func() {
		close(s.done)
		if s.redisClient != nil {
			closeErr = s.redisClient.Close()
			s.redisClient = nil
		}
		if s.postgresPool != nil {
			s.postgresPool.Close()
			s.postgresPool = nil
		}
	})
	return closeErr
}

func (s *Store) New(meta map[string]string) *Job {
	now := time.Now()
	j := &Job{
		ID:        uuid.NewString(),
		Status:    StatusPending,
		CreatedAt: now,
		UpdatedAt: now,
		ExpiresAt: now.Add(s.ttl),
		Progress:  0,
		Meta:      cloneStringMap(meta),
		Result:    map[string]any{},
	}

	stored := cloneJob(j)
	s.mu.Lock()
	s.jobs[stored.ID] = stored
	s.mu.Unlock()

	if s.backend == "redis" && s.redisClient != nil {
		s.persistRedis(stored)
	}
	if s.backend == "postgres" && s.postgresPool != nil {
		s.persistPostgres(stored)
	}
	return cloneJob(stored)
}

func (s *Store) Get(id string) (*Job, bool) {
	now := time.Now()

	s.mu.Lock()
	if existing, ok := s.jobs[id]; ok {
		if existing.ExpiresAt.After(now) {
			clone := cloneJob(existing)
			s.mu.Unlock()
			return clone, true
		}
		delete(s.jobs, id)
	}
	s.mu.Unlock()

	var loaded *Job
	var ok bool

	if s.backend == "redis" && s.redisClient != nil {
		loaded, ok = s.getRedis(id)
	} else if s.backend == "postgres" && s.postgresPool != nil {
		loaded, ok = s.getPostgres(id)
	}

	if ok && loaded != nil && !loaded.ExpiresAt.After(now) {
		s.deletePersistent(id)
		return nil, false
	}

	if ok && loaded != nil {
		s.mu.Lock()
		s.jobs[id] = cloneJob(loaded)
		s.mu.Unlock()
		return cloneJob(loaded), true
	}

	return nil, false
}

func (s *Store) Update(id string, update func(*Job)) (*Job, bool) {
	if strings.TrimSpace(id) == "" {
		return nil, false
	}
	if update == nil {
		return nil, false
	}

	current, ok := s.Get(id)
	if !ok {
		return nil, false
	}

	update(current)
	now := time.Now()
	current.UpdatedAt = now
	current.ExpiresAt = now.Add(s.ttl)

	stored := cloneJob(current)
	s.mu.Lock()
	s.jobs[id] = stored
	s.mu.Unlock()

	if s.backend == "redis" && s.redisClient != nil {
		s.persistRedis(stored)
	}
	if s.backend == "postgres" && s.postgresPool != nil {
		s.persistPostgres(stored)
	}

	return cloneJob(stored), true
}

func (s *Store) Upsert(job *Job) *Job {
	if job == nil || strings.TrimSpace(job.ID) == "" {
		return nil
	}

	now := time.Now()
	if job.CreatedAt.IsZero() {
		job.CreatedAt = now
	}
	job.UpdatedAt = now
	if job.ExpiresAt.IsZero() {
		job.ExpiresAt = now.Add(s.ttl)
	}
	if job.Result == nil {
		job.Result = map[string]any{}
	}

	stored := cloneJob(job)
	s.mu.Lock()
	s.jobs[stored.ID] = stored
	s.mu.Unlock()

	if s.backend == "redis" && s.redisClient != nil {
		s.persistRedis(stored)
	}
	if s.backend == "postgres" && s.postgresPool != nil {
		s.persistPostgres(stored)
	}

	return cloneJob(stored)
}

// Evict removes a job from the in-memory cache so the next Get() call reads
// fresh data from the persistent backend (postgres/redis). This is used by
// the SSE heartbeat loop to bypass stale cache entries written by a remote
// worker process.
func (s *Store) Evict(id string) {
	s.mu.Lock()
	delete(s.jobs, id)
	s.mu.Unlock()
}

func (s *Store) List() []*Job {
	s.mu.RLock()
	defer s.mu.RUnlock()

	items := make([]*Job, 0, len(s.jobs))
	now := time.Now()
	for _, job := range s.jobs {
		if job == nil || now.After(job.ExpiresAt) {
			continue
		}
		items = append(items, cloneJob(job))
	}
	sort.Slice(items, func(i, j int) bool {
		return items[i].CreatedAt.After(items[j].CreatedAt)
	})
	return items
}

func (s *Store) persistRedis(job *Job) {
	if s.redisClient == nil || job == nil {
		return
	}
	b, err := json.Marshal(job)
	if err != nil {
		return
	}
	_ = s.redisClient.Set(context.Background(), "jobs:"+job.ID, b, s.ttl).Err()
}

func (s *Store) getRedis(id string) (*Job, bool) {
	if s.redisClient == nil {
		return nil, false
	}
	b, err := s.redisClient.Get(context.Background(), "jobs:"+id).Bytes()
	if err != nil {
		return nil, false
	}
	var j Job
	if err := json.Unmarshal(b, &j); err != nil {
		return nil, false
	}
	if j.Result == nil {
		j.Result = map[string]any{}
	}
	return cloneJob(&j), true
}

func (s *Store) persistPostgres(job *Job) {
	if s.postgresPool == nil || job == nil {
		return
	}
	b, err := json.Marshal(job)
	if err != nil {
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	// Using pgx Exec
	_, _ = s.postgresPool.Exec(ctx,
		`INSERT INTO quarry_jobs (id, status, payload, expires_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5)
		 ON CONFLICT (id)
		 DO UPDATE SET 
		 	status = EXCLUDED.status,
		 	payload = EXCLUDED.payload, 
			expires_at = EXCLUDED.expires_at, 
			updated_at = EXCLUDED.updated_at`,
		job.ID,
		job.Status,
		b,
		job.ExpiresAt,
		time.Now(),
	)
}

func (s *Store) getPostgres(id string) (*Job, bool) {
	if s.postgresPool == nil {
		return nil, false
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	var payload []byte
	err := s.postgresPool.QueryRow(ctx,
		`SELECT payload FROM quarry_jobs WHERE id = $1`, id).Scan(&payload)

	if err != nil {
		return nil, false
	}
	var job Job
	if unmarshalErr := json.Unmarshal(payload, &job); unmarshalErr != nil {
		return nil, false
	}
	if job.Result == nil {
		job.Result = map[string]any{}
	}
	return cloneJob(&job), true
}

func (s *Store) deletePersistent(id string) {
	switch {
	case s.redisClient != nil && s.backend == "redis":
		_ = s.redisClient.Del(context.Background(), "jobs:"+id).Err()
	case s.postgresPool != nil && s.backend == "postgres":
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_, _ = s.postgresPool.Exec(ctx, `DELETE FROM quarry_jobs WHERE id = $1`, id)
	}
}

func (s *Store) cleanupExpired() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			now := time.Now()
			expiredIDs := make([]string, 0)

			s.mu.Lock()
			for id, job := range s.jobs {
				if !job.ExpiresAt.After(now) {
					expiredIDs = append(expiredIDs, id)
					delete(s.jobs, id)
				}
			}
			s.mu.Unlock()

			for _, id := range expiredIDs {
				s.deletePersistent(id)
			}
		case <-s.done:
			return
		}
	}
}

func cloneJob(job *Job) *Job {
	if job == nil {
		return nil
	}

	clone := *job
	clone.Meta = cloneStringMap(job.Meta)
	clone.Result = cloneJSONMap(job.Result)
	return &clone
}

func cloneStringMap(src map[string]string) map[string]string {
	if len(src) == 0 {
		if src == nil {
			return nil
		}
		return map[string]string{}
	}

	dst := make(map[string]string, len(src))
	for key, value := range src {
		dst[key] = value
	}
	return dst
}

func ListSortedJobs(ctx context.Context, store *Store, limit int) ([]*Job, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if store == nil {
		return nil, nil
	}
	if limit <= 0 {
		limit = 10
	}

	store.mu.Lock()
	store.cleanupExpiredLocked(time.Now())
	items := make([]*Job, 0, len(store.jobs))
	for _, job := range store.jobs {
		items = append(items, cloneJob(job))
	}
	store.mu.Unlock()

	sort.Slice(items, func(i, j int) bool {
		if items[i].UpdatedAt.Equal(items[j].UpdatedAt) {
			return items[i].CreatedAt.After(items[j].CreatedAt)
		}
		return items[i].UpdatedAt.After(items[j].UpdatedAt)
	})
	if len(items) > limit {
		items = items[:limit]
	}
	return items, nil
}

func (s *Store) cleanupExpiredLocked(now time.Time) {
	for id, job := range s.jobs {
		if !job.ExpiresAt.After(now) {
			delete(s.jobs, id)
		}
	}
}

func ensurePostgresSchema(ctx context.Context, pool *pgxpool.Pool) error {
	// Basic schema init for convenience. Production should use migrations.
	_, err := pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS quarry_jobs (
			id TEXT PRIMARY KEY,
			status TEXT NOT NULL DEFAULT 'pending',
			payload JSONB NOT NULL,
			created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
			updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
			expires_at TIMESTAMP WITH TIME ZONE NOT NULL
		);
		CREATE INDEX IF NOT EXISTS idx_quarry_jobs_status ON quarry_jobs(status);
	`)
	return err
}
