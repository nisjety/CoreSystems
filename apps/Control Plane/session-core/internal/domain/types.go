package domain

import (
	"errors"
	"time"
)

var ErrOrgMembershipDenied = errors.New("org membership denied")

// SessionStatus represents the lifecycle state of a session.
type SessionStatus string

const (
	SessionStatusActive    SessionStatus = "active"
	SessionStatusPaused    SessionStatus = "paused"
	SessionStatusCompleted SessionStatus = "completed"
	SessionStatusFailed    SessionStatus = "failed"
	SessionStatusCancelled SessionStatus = "cancelled"
)

// ModelPlaneVersion determines which event plane handles this session.
type ModelPlaneVersion string

const (
	ModelPlaneV1 ModelPlaneVersion = "v1"
	ModelPlaneV2 ModelPlaneVersion = "v2"
)

// ApprovalStatus for human-in-the-loop decisions.
type ApprovalStatus string

const (
	ApprovalPending  ApprovalStatus = "pending"
	ApprovalApproved ApprovalStatus = "approved"
	ApprovalRejected ApprovalStatus = "rejected"
)

// Session is the root aggregate for a user's agent interaction.
type Session struct {
	ID                string            `json:"id"`
	TenantID          string            `json:"tenant_id"`
	WorkspaceID       string            `json:"workspace_id"`
	UserID            string            `json:"user_id"`
	OrgID             string            `json:"org_id" db:"org_id"`
	UserRole          string            `json:"user_role" db:"user_role"`
	ModelPlaneVersion ModelPlaneVersion `json:"model_plane_version"`
	Status            SessionStatus     `json:"status"`
	PlanMode          bool              `json:"plan_mode"`
	Metadata          map[string]any    `json:"metadata,omitempty"`
	CreatedAt         time.Time         `json:"created_at"`
	UpdatedAt         time.Time         `json:"updated_at"`
}

// SessionEvent is an append-only event within a session.
type SessionEvent struct {
	ID        string    `json:"id"`
	SessionID string    `json:"session_id"`
	Sequence  int64     `json:"sequence"`
	EventType string    `json:"event_type"`
	Payload   []byte    `json:"payload"`
	CreatedAt time.Time `json:"created_at"`
}

// Approval represents a pending human decision.
type Approval struct {
	ID         string         `json:"id"`
	SessionID  string         `json:"session_id"`
	ToolName   string         `json:"tool_name"`
	ToolInput  []byte         `json:"tool_input"`
	Status     ApprovalStatus `json:"status"`
	Feedback   string         `json:"feedback,omitempty"`
	ResolvedAt *time.Time     `json:"resolved_at,omitempty"`
	CreatedAt  time.Time      `json:"created_at"`
}

// SessionState is the full state snapshot returned to clients.
type SessionState struct {
	Session          Session    `json:"session"`
	PendingApprovals []Approval `json:"pending_approvals"`
	EventCursor      int64      `json:"event_cursor"`
}

// --- Request DTOs ---

type CreateSessionRequest struct {
	TenantID    string         `json:"tenant_id" binding:"required"`
	WorkspaceID string         `json:"workspace_id" binding:"required"`
	OrgID       string         `json:"org_id" binding:"required"`
	PlanMode    bool           `json:"plan_mode"`
	Metadata    map[string]any `json:"metadata,omitempty"`
}

type SendMessageRequest struct {
	Role    string `json:"role" binding:"required"`
	Content string `json:"content" binding:"required"`
}

type ApprovalDecisionRequest struct {
	Approve  bool   `json:"approve"`
	Feedback string `json:"feedback,omitempty"`
}

// --- Orchestration types ---

type AgentTaskStatus string

const (
	AgentTaskPending AgentTaskStatus = "pending"
	AgentTaskRunning AgentTaskStatus = "running"
	AgentTaskDone    AgentTaskStatus = "done"
	AgentTaskFailed  AgentTaskStatus = "failed"
)

type AgentTask struct {
	ID        string          `json:"id"`
	SessionID string          `json:"session_id"`
	TenantID  string          `json:"tenant_id"`
	TaskType  string          `json:"task_type"`
	Payload   []byte          `json:"payload"`
	Status    AgentTaskStatus `json:"status"`
	Metadata  map[string]any  `json:"metadata,omitempty"`
	CreatedAt time.Time       `json:"created_at"`
	UpdatedAt time.Time       `json:"updated_at"`
}

type AgentCronTask struct {
	ID        string     `json:"id"`
	TenantID  string     `json:"tenant_id"`
	Name      string     `json:"name"`
	CronExpr  string     `json:"cron_expr"`
	TaskType  string     `json:"task_type"`
	Payload   []byte     `json:"payload"`
	Enabled   bool       `json:"enabled"`
	LastRun   *time.Time `json:"last_run,omitempty"`
	CreatedAt time.Time  `json:"created_at"`
	UpdatedAt time.Time  `json:"updated_at"`
}

type HookConfig struct {
	ID        string            `json:"id"`
	TenantID  string            `json:"tenant_id"`
	EventType string            `json:"event_type"`
	Endpoint  string            `json:"endpoint"`
	Headers   map[string]string `json:"headers,omitempty"`
	Enabled   bool              `json:"enabled"`
	CreatedAt time.Time         `json:"created_at"`
	UpdatedAt time.Time         `json:"updated_at"`
}

type AgentSkill struct {
	ID          string         `json:"id"`
	TenantID    string         `json:"tenant_id"`
	Name        string         `json:"name"`
	Description string         `json:"description"`
	Config      map[string]any `json:"config,omitempty"`
	CreatedAt   time.Time      `json:"created_at"`
	UpdatedAt   time.Time      `json:"updated_at"`
}

type AgentMemory struct {
	ID        string    `json:"id"`
	SessionID string    `json:"session_id"`
	TenantID  string    `json:"tenant_id"`
	Key       string    `json:"key"`
	Value     []byte    `json:"value"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

// ─── Plan ────────────────────────────────────────────────────────────────────

type PlanState string

const (
	PlanStateDraft      PlanState = "DRAFT"
	PlanStateProposed   PlanState = "PROPOSED"
	PlanStateApproved   PlanState = "APPROVED"
	PlanStateRejected   PlanState = "REJECTED"
	PlanStateExecuting  PlanState = "EXECUTING"
	PlanStateCompleted  PlanState = "COMPLETED"
	PlanStateFailed     PlanState = "FAILED"
	PlanStateSuperseded PlanState = "SUPERSEDED"
	PlanStateArchived   PlanState = "ARCHIVED"
)

type Plan struct {
	ID         string         `json:"id"`
	RunID      string         `json:"run_id"`
	ThreadID   string         `json:"thread_id"`
	Author     string         `json:"author"`
	State      PlanState      `json:"state"`
	Summary    string         `json:"summary"`
	Supersedes *string        `json:"supersedes,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
	CreatedAt  time.Time      `json:"created_at"`
	UpdatedAt  time.Time      `json:"updated_at"`
}

type PlanStepState string

const (
	PlanStepStatePending PlanStepState = "PENDING"
	PlanStepStateRunning PlanStepState = "RUNNING"
	PlanStepStateDone    PlanStepState = "DONE"
	PlanStepStateSkipped PlanStepState = "SKIPPED"
	PlanStepStateFailed  PlanStepState = "FAILED"
)

type PlanStep struct {
	ID        string        `json:"id"`
	PlanID    string        `json:"plan_id"`
	StepOrder int           `json:"step_order"`
	Title     string        `json:"title"`
	Operation string        `json:"operation"`
	State     PlanStepState `json:"state"`
	CreatedAt time.Time     `json:"created_at"`
	UpdatedAt time.Time     `json:"updated_at"`
}

// ─── Approval ─────────────────────────────────────────────────────────────────

type ApprovalKind string

const (
	ApprovalKindPlan        ApprovalKind = "PLAN"
	ApprovalKindToolCall    ApprovalKind = "TOOL_CALL"
	ApprovalKindPermission  ApprovalKind = "PERMISSION"
	ApprovalKindDestructive ApprovalKind = "DESTRUCTIVE"
	ApprovalKindCost        ApprovalKind = "COST"
)

type ApprovalState string

const (
	ApprovalStateRequested ApprovalState = "REQUESTED"
	ApprovalStateGranted   ApprovalState = "GRANTED"
	ApprovalStateDenied    ApprovalState = "DENIED"
	ApprovalStateTimedOut  ApprovalState = "TIMED_OUT"
)

type ModelApproval struct {
	ID             string         `json:"id"`
	RunID          string         `json:"run_id"`
	StepID         *string        `json:"step_id,omitempty"`
	Kind           ApprovalKind   `json:"kind"`
	State          ApprovalState  `json:"state"`
	RequestedOf    string         `json:"requested_of"`
	DecidedBy      *string        `json:"decided_by,omitempty"`
	DecisionReason *string        `json:"decision_reason,omitempty"`
	Context        map[string]any `json:"context,omitempty"`
	RequestedAt    time.Time      `json:"requested_at"`
	DecidedAt      *time.Time     `json:"decided_at,omitempty"`
	ExpiresAt      *time.Time     `json:"expires_at,omitempty"`
}

// ─── Todo ─────────────────────────────────────────────────────────────────────

type TodoState string

const (
	TodoStatePending    TodoState = "PENDING"
	TodoStateInProgress TodoState = "IN_PROGRESS"
	TodoStateBlocked    TodoState = "BLOCKED"
	TodoStateCompleted  TodoState = "COMPLETED"
	TodoStateCancelled  TodoState = "CANCELLED"
)

type TodoPriority string

const (
	TodoPriorityLow    TodoPriority = "LOW"
	TodoPriorityNormal TodoPriority = "NORMAL"
	TodoPriorityHigh   TodoPriority = "HIGH"
	TodoPriorityUrgent TodoPriority = "URGENT"
)

type Todo struct {
	ID          string         `json:"id"`
	ThreadID    string         `json:"thread_id"`
	RunID       string         `json:"run_id"`
	Assignee    string         `json:"assignee"`
	Title       string         `json:"title"`
	Description string         `json:"description"`
	State       TodoState      `json:"state"`
	Priority    TodoPriority   `json:"priority"`
	BlockedBy   []string       `json:"blocked_by,omitempty"`
	Metadata    map[string]any `json:"metadata,omitempty"`
	CreatedAt   time.Time      `json:"created_at"`
	UpdatedAt   time.Time      `json:"updated_at"`
	CompletedAt *time.Time     `json:"completed_at,omitempty"`
}

// ─── Lineage ──────────────────────────────────────────────────────────────────

type LineageRole string

const (
	LineageRoleCoder      LineageRole = "CODER"
	LineageRoleReviewer   LineageRole = "REVIEWER"
	LineageRoleResearcher LineageRole = "RESEARCHER"
	LineageRoleExplorer   LineageRole = "EXPLORER"
	LineageRoleGeneric    LineageRole = "GENERIC"
)

type LineageEdge struct {
	ParentRunID string      `json:"parent_run_id"`
	ChildRunID  string      `json:"child_run_id"`
	Role        LineageRole `json:"role"`
	SpawnedAt   time.Time   `json:"spawned_at"`
}
