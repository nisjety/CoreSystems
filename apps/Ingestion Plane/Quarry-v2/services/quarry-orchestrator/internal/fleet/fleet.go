// Package fleet — W5 durable fleet state.
//
// A fleet is a coordinated batch of agent runs that share a budget.
// The orchestrator's `FleetOrchestrator` Temporal workflow drives
// fan-out / fan-in: it starts N `AgentRunWF` children with the fleet's
// shared constraints and joins on completion or budget exhaustion.
//
// This package is the Go-side durable envelope; the Rust runtime's
// `quarry_runtime::fleet::FleetTask` / `FleetBudgetTracker` is the
// live in-process pool for the duration of a workflow branch.
package fleet

import (
	"errors"
	"sync"
	"time"
)

// Status mirrors Rust `FleetStatus`.
type Status string

const (
	StatusPending   Status = "pending"
	StatusRunning   Status = "running"
	StatusCompleted Status = "completed"
	StatusFailed    Status = "failed"
	StatusCancelled Status = "cancelled"
)

// Task is the durable fleet envelope. Persisted in the orchestrator's
// store (or the control plane's `quarry_fleets` table when wired;
// today it is in-memory for dev/test like the rest of the
// orchestrator's store).
type Task struct {
	FleetID            string    `json:"fleet_id"`
	OrgID              string    `json:"org_id"`
	BudgetUSD          *float64  `json:"budget_usd,omitempty"`
	MaxParallelRuns    int       `json:"max_parallel_runs"`
	SharedProfileID    *string   `json:"shared_profile_id,omitempty"`
	SharedDomainIntelID *string  `json:"shared_domain_intel_id,omitempty"`
	MemberRunIDs       []string  `json:"member_run_ids"`
	Status             Status    `json:"status"`
	CreatedAt          time.Time `json:"created_at"`
	FinishedAt         *time.Time `json:"finished_at,omitempty"`
}

// BudgetTracker aggregates spend across member runs. Mirrors Rust
// `FleetBudgetTracker` but without atomics — the orchestrator owns
// the tracker inside a single workflow goroutine.
type BudgetTracker struct {
	mu            sync.Mutex
	budgetMicroUSD *int64
	spentMicroUSD  int64
	perRunMicroUSD map[string]int64
}

func NewBudgetTracker(budgetUSD *float64) *BudgetTracker {
	var budgetMicro *int64
	if budgetUSD != nil {
		micro := int64(*budgetUSD * 1_000_000)
		budgetMicro = &micro
	}
	return &BudgetTracker{
		budgetMicroUSD: budgetMicro,
		perRunMicroUSD: make(map[string]int64),
	}
}

func (b *BudgetTracker) Record(runID string, additionalUSD float64) {
	if additionalUSD < 0 || additionalUSD != additionalUSD { // NaN check
		return
	}
	micro := int64(additionalUSD * 1_000_000)
	b.RecordMicro(runID, micro)
}

func (b *BudgetTracker) RecordMicro(runID string, microUSD int64) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.spentMicroUSD += microUSD
	b.perRunMicroUSD[runID] += microUSD
}

func (b *BudgetTracker) SpentUSD() float64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	return float64(b.spentMicroUSD) / 1_000_000
}

func (b *BudgetTracker) OverBudget() (spent, limit float64, over bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.budgetMicroUSD == nil {
		return 0, 0, false
	}
	limit = float64(*b.budgetMicroUSD) / 1_000_000
	spent = float64(b.spentMicroUSD) / 1_000_000
	return spent, limit, spent >= limit
}

// Registry is the in-process fleet registry for the orchestrator.
type Registry struct {
	mu       sync.RWMutex
	fleets   map[string]*Task
	trackers map[string]*BudgetTracker
}

func NewRegistry() *Registry {
	return &Registry{
		fleets:   make(map[string]*Task),
		trackers: make(map[string]*BudgetTracker),
	}
}

func (r *Registry) Create(task *Task) (*BudgetTracker, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.fleets[task.FleetID]; exists {
		return nil, errors.New("fleet already exists: " + task.FleetID)
	}
	if task.MaxParallelRuns < 1 {
		task.MaxParallelRuns = 1
	}
	if task.CreatedAt.IsZero() {
		task.CreatedAt = time.Now().UTC()
	}
	if task.Status == "" {
		task.Status = StatusPending
	}
	tracker := NewBudgetTracker(task.BudgetUSD)
	r.fleets[task.FleetID] = task
	r.trackers[task.FleetID] = tracker
	return tracker, nil
}

func (r *Registry) Get(fleetID string) (*Task, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	t, ok := r.fleets[fleetID]
	if !ok {
		return nil, false
	}
	cp := *t
	return &cp, true
}

func (r *Registry) Tracker(fleetID string) (*BudgetTracker, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	t, ok := r.trackers[fleetID]
	return t, ok
}

func (r *Registry) AddMember(fleetID, runID string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	t, ok := r.fleets[fleetID]
	if !ok {
		return errors.New("fleet not found: " + fleetID)
	}
	for _, id := range t.MemberRunIDs {
		if id == runID {
			return nil
		}
	}
	t.MemberRunIDs = append(t.MemberRunIDs, runID)
	return nil
}

func (r *Registry) Finish(fleetID string, status Status) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	t, ok := r.fleets[fleetID]
	if !ok {
		return errors.New("fleet not found: " + fleetID)
	}
	t.Status = status
	now := time.Now().UTC()
	t.FinishedAt = &now
	return nil
}

func (r *Registry) Len() int {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return len(r.fleets)
}

// Subject helpers — mirror Rust `fleet::subjects`.
func Subject(fleetID, event string) string {
	return "quarry.fleet." + fleetID + "." + event
}

func Wildcard(fleetID string) string {
	return "quarry.fleet." + fleetID + ".>"
}
