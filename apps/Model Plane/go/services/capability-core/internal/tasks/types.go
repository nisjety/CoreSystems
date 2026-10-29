// Package tasks documents the durable owner for agentic task lifecycles —
// creation, assignment, blocking, and completion — surfaced to the model
// plane as a read-only catalog. The catalog describes tasks seeded for the
// "triodelab" organization; full mutation APIs (Create/Update/Assign/Block/
// Complete) are handled by downstream task-runner services.
package tasks

// Status enumerates the lifecycle states of a Task.
type Status string

// Task lifecycle states.
const (
	StatusCreated   Status = "created"
	StatusAssigned  Status = "assigned"
	StatusBlocked   Status = "blocked"
	StatusCompleted Status = "completed"
)

// Task is a durable unit of work owned by the capability-core task registry.
type Task struct {
	ID             string   `json:"id"`
	IdempotencyKey string   `json:"idempotencyKey"`
	OrgID          string   `json:"orgId"`
	ParentRunID    string   `json:"parentRunId,omitempty"`
	Assignee       string   `json:"assignee,omitempty"`
	Status         Status   `json:"status"`
	Inputs         []string `json:"inputs,omitempty"`
	Outputs        []string `json:"outputs,omitempty"`
	CreatedAt      string   `json:"createdAt"`
	UpdatedAt      string   `json:"updatedAt"`
	Description    string   `json:"description,omitempty"`
}

// Catalog is the read-only list of tasks documenting the durable owner.
type Catalog struct {
	Tasks []Task `json:"tasks"`
}

// Load returns the seeded task catalog.
func Load() Catalog { return catalog }
