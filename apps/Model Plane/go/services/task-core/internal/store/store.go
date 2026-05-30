// Package store implements in-memory storage for task lifecycle.
// A task represents a named unit of work — optionally recurring via a cron
// expression — owned by an organisation.
package store

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"
)

// Sentinel errors for task operations.
var (
	ErrTaskNotFound = errors.New("task not found")
	ErrEmptyName    = errors.New("task name is required")
	ErrEmptyOrgID   = errors.New("org_id is required")
)

// Status represents the lifecycle state of a task.
type Status string

const (
	StatusPending   Status = "pending"
	StatusRunning   Status = "running"
	StatusCompleted Status = "completed"
	StatusFailed    Status = "failed"
)

// Task is a schedulable unit of work.
type Task struct {
	ID        string
	OrgID     string
	Name      string
	CronExpr  string // empty for one-shot tasks
	Payload   string
	Status    Status
	NextRunAt time.Time
	LastRunAt time.Time
	CreatedAt time.Time
}

// Store is a thread-safe in-memory task store.
type Store struct {
	mu    sync.Mutex
	tasks map[string]*Task
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{tasks: make(map[string]*Task)}
}

// Create persists a new task and returns a copy.
func (s *Store) Create(orgID, name, cronExpr, payload string, nextRunAt time.Time) (*Task, error) {
	if orgID == "" {
		return nil, ErrEmptyOrgID
	}
	if name == "" {
		return nil, ErrEmptyName
	}
	id, err := newID()
	if err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	t := &Task{
		ID:        id,
		OrgID:     orgID,
		Name:      name,
		CronExpr:  cronExpr,
		Payload:   payload,
		Status:    StatusPending,
		NextRunAt: nextRunAt,
		CreatedAt: now,
	}
	s.mu.Lock()
	s.tasks[id] = t
	s.mu.Unlock()
	return copyTask(t), nil
}

// Get returns a task by ID.
func (s *Store) Get(id string) (*Task, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[id]
	if !ok {
		return nil, ErrTaskNotFound
	}
	return copyTask(t), nil
}

// List returns all tasks belonging to the given org.
func (s *Store) List(orgID string) []*Task {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []*Task
	for _, t := range s.tasks {
		if t.OrgID == orgID {
			out = append(out, copyTask(t))
		}
	}
	return out
}

// UpdateStatus transitions a task to a new status and records the run time.
func (s *Store) UpdateStatus(id string, status Status) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[id]
	if !ok {
		return ErrTaskNotFound
	}
	t.Status = status
	if status == StatusRunning {
		t.LastRunAt = time.Now().UTC()
	}
	return nil
}

// SetNextRun updates the next scheduled execution time for a task.
func (s *Store) SetNextRun(id string, next time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[id]
	if !ok {
		return ErrTaskNotFound
	}
	t.NextRunAt = next
	return nil
}

// ListDue returns all tasks whose NextRunAt is at or before the provided time
// and whose status is pending.
func (s *Store) ListDue(now time.Time) []*Task {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []*Task
	for _, t := range s.tasks {
		if t.Status == StatusPending && !t.NextRunAt.IsZero() && !t.NextRunAt.After(now) {
			out = append(out, copyTask(t))
		}
	}
	return out
}

// copyTask returns a shallow copy to avoid leaking the internal pointer.
func copyTask(t *Task) *Task {
	cp := *t
	return &cp
}

func newID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
