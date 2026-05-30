package jobs

import (
	"context"
	"encoding/json"
	"sort"
	"sync"
	"time"
)

// ExtractionJob tracks one async single-page extraction request.
// The handler updates this record as the background worker progresses so callers
// can poll for status without touching in-flight mutable state directly.
type ExtractionJob struct {
	ID        string                 `json:"id"`
	URL       string                 `json:"url"`
	Schema    string                 `json:"schema,omitempty"`
	Prompt    string                 `json:"prompt,omitempty"`
	Status    ExtractionStatus       `json:"status"`
	Result    map[string]interface{} `json:"result,omitempty"`
	Error     string                 `json:"error,omitempty"`
	CreatedAt time.Time              `json:"created_at"`
	UpdatedAt time.Time              `json:"updated_at"`
	ExpiresAt time.Time              `json:"expires_at"`
	Duration  int64                  `json:"duration_ms,omitempty"`
}

// ExtractionStatus represents the lifecycle state of an extraction job.
type ExtractionStatus string

// Extraction job status constants.
const (
	ExtractionQueued     ExtractionStatus = "queued"
	ExtractionProcessing ExtractionStatus = "processing"
	ExtractionCompleted  ExtractionStatus = "completed"
	ExtractionFailed     ExtractionStatus = "failed"
	ExtractionCancelled  ExtractionStatus = "cancelled"
)

// ExtractionStore isolates async extraction persistence from crawl-job storage.
// The split matters because extraction jobs have different payload shape, TTL,
// and polling semantics than crawl workflow jobs.
type ExtractionStore interface {
	Create(ctx context.Context, job *ExtractionJob) error
	Get(ctx context.Context, jobID string) (*ExtractionJob, error)
	Update(ctx context.Context, job *ExtractionJob) error
	Delete(ctx context.Context, jobID string) error
	List(ctx context.Context, limit int) ([]*ExtractionJob, error)
	Close() error
}

// InMemoryExtractionStore provides an in-memory extraction store.
// It deep-clones records on the way in and out so callers cannot race by mutating
// shared map-backed payloads after persistence.
type InMemoryExtractionStore struct {
	mu        sync.RWMutex
	jobs      map[string]*ExtractionJob
	ttl       time.Duration
	done      chan struct{}
	closeOnce sync.Once
}

// NewInMemoryExtractionStore creates a new in-memory extraction store.
func NewInMemoryExtractionStore(ttl time.Duration) ExtractionStore {
	if ttl <= 0 {
		ttl = time.Hour
	}

	store := &InMemoryExtractionStore{
		jobs: make(map[string]*ExtractionJob),
		ttl:  ttl,
		done: make(chan struct{}),
	}

	// The cleanup loop bounds memory growth for completed async jobs without
	// forcing every status poll to scan the whole store.
	go store.cleanupExpired()
	return store
}

// NewInMemoryJobStore preserves the old constructor name used by the handler.
func NewInMemoryJobStore(ttl time.Duration) ExtractionStore {
	return NewInMemoryExtractionStore(ttl)
}

// Create stores a new extraction job.
func (s *InMemoryExtractionStore) Create(ctx context.Context, job *ExtractionJob) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if job == nil {
		return ErrInvalidID
	}
	if job.ID == "" || job.URL == "" {
		return ErrInvalidID
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

	s.mu.Lock()
	defer s.mu.Unlock()
	s.cleanupExpiredLocked(now)

	if _, exists := s.jobs[stored.ID]; exists {
		return ErrJobExists
	}

	s.jobs[stored.ID] = stored
	return nil
}

// Get retrieves an extraction job by ID.
func (s *InMemoryExtractionStore) Get(ctx context.Context, jobID string) (*ExtractionJob, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}

	now := time.Now()
	s.mu.Lock()
	defer s.mu.Unlock()
	s.cleanupExpiredLocked(now)

	job, exists := s.jobs[jobID]
	if !exists || !job.ExpiresAt.After(now) {
		delete(s.jobs, jobID)
		return nil, ErrJobNotFound
	}

	return cloneExtractionJob(job), nil
}

// Update updates an existing extraction job.
func (s *InMemoryExtractionStore) Update(ctx context.Context, job *ExtractionJob) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if job == nil || job.ID == "" {
		return ErrInvalidID
	}

	now := time.Now()
	s.mu.RLock()
	existing, exists := s.jobs[job.ID]
	s.mu.RUnlock()
	if !exists {
		return ErrJobNotFound
	}

	updated := cloneExtractionJob(job)
	updated.CreatedAt = existing.CreatedAt
	updated.UpdatedAt = now
	updated.ExpiresAt = now.Add(s.ttl)
	if updated.Status == ExtractionCompleted || updated.Status == ExtractionFailed || updated.Status == ExtractionCancelled {
		updated.Duration = updated.UpdatedAt.Sub(updated.CreatedAt).Milliseconds()
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	s.jobs[updated.ID] = updated
	return nil
}

// Delete removes an extraction job.
func (s *InMemoryExtractionStore) Delete(ctx context.Context, jobID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, exists := s.jobs[jobID]; !exists {
		return ErrJobNotFound
	}

	delete(s.jobs, jobID)
	return nil
}

// List returns recent extraction jobs ordered by update time descending.
func (s *InMemoryExtractionStore) List(ctx context.Context, limit int) ([]*ExtractionJob, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if limit <= 0 || limit > 100 {
		limit = 10
	}

	now := time.Now()
	s.mu.Lock()
	defer s.mu.Unlock()
	s.cleanupExpiredLocked(now)

	result := make([]*ExtractionJob, 0, len(s.jobs))
	for _, job := range s.jobs {
		result = append(result, cloneExtractionJob(job))
	}

	sort.Slice(result, func(i, j int) bool {
		if result[i].UpdatedAt.Equal(result[j].UpdatedAt) {
			return result[i].CreatedAt.After(result[j].CreatedAt)
		}
		return result[i].UpdatedAt.After(result[j].UpdatedAt)
	})

	if len(result) > limit {
		result = result[:limit]
	}

	return result, nil
}

// cleanupExpired removes expired jobs every 5 minutes.
func (s *InMemoryExtractionStore) cleanupExpired() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			s.mu.Lock()
			s.cleanupExpiredLocked(time.Now())
			s.mu.Unlock()
		case <-s.done:
			return
		}
	}
}

// Close shuts down the cleanup routine.
func (s *InMemoryExtractionStore) Close() error {
	s.closeOnce.Do(func() {
		close(s.done)
	})
	return nil
}

func (s *InMemoryExtractionStore) cleanupExpiredLocked(now time.Time) {
	for jobID, job := range s.jobs {
		if !job.ExpiresAt.After(now) {
			delete(s.jobs, jobID)
		}
	}
}

func cloneExtractionJob(job *ExtractionJob) *ExtractionJob {
	if job == nil {
		return nil
	}

	clone := *job
	clone.Result = cloneJSONMap(job.Result)
	return &clone
}

func cloneJSONMap(src map[string]interface{}) map[string]interface{} {
	if len(src) == 0 {
		if src == nil {
			return nil
		}
		return map[string]interface{}{}
	}

	encoded, err := json.Marshal(src)
	if err != nil {
		out := make(map[string]interface{}, len(src))
		for key, value := range src {
			out[key] = value
		}
		return out
	}

	var out map[string]interface{}
	if err := json.Unmarshal(encoded, &out); err != nil {
		out = make(map[string]interface{}, len(src))
		for key, value := range src {
			out[key] = value
		}
	}
	return out
}
