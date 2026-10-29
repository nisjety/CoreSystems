package repository

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/domain"
)

type SessionRepository struct {
	pool *pgxpool.Pool
}

func NewSessionRepository(pool *pgxpool.Pool) *SessionRepository {
	return &SessionRepository{pool: pool}
}

// --- Sessions ---

func (r *SessionRepository) Create(ctx context.Context, s *domain.Session) error {
	metadataJSON, _ := json.Marshal(s.Metadata)

	return r.pool.QueryRow(ctx, `
		INSERT INTO sessions (tenant_id, workspace_id, user_id, model_plane_version, status, plan_mode, metadata, org_id, user_role)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
		RETURNING id, created_at, updated_at, org_id, user_role
	`, s.TenantID, s.WorkspaceID, s.UserID, s.ModelPlaneVersion, s.Status, s.PlanMode, metadataJSON, s.OrgID, s.UserRole,
	).Scan(&s.ID, &s.CreatedAt, &s.UpdatedAt, &s.OrgID, &s.UserRole)
}

func (r *SessionRepository) GetByID(ctx context.Context, id string) (*domain.Session, error) {
	s := &domain.Session{}
	var metadataJSON []byte

	err := r.pool.QueryRow(ctx, `
		SELECT id, tenant_id, workspace_id, user_id, model_plane_version, status, plan_mode, metadata, created_at, updated_at, org_id, user_role
		FROM sessions WHERE id = $1
	`, id).Scan(
		&s.ID, &s.TenantID, &s.WorkspaceID, &s.UserID,
		&s.ModelPlaneVersion, &s.Status, &s.PlanMode,
		&metadataJSON, &s.CreatedAt, &s.UpdatedAt, &s.OrgID, &s.UserRole,
	)
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, fmt.Errorf("session not found: %s", id)
		}
		return nil, fmt.Errorf("get session: %w", err)
	}

	if metadataJSON != nil {
		_ = json.Unmarshal(metadataJSON, &s.Metadata)
	}
	return s, nil
}

func (r *SessionRepository) UpdateStatus(ctx context.Context, id string, status domain.SessionStatus) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE sessions SET status = $1, updated_at = NOW() WHERE id = $2
	`, status, id)
	if err != nil {
		return fmt.Errorf("update session status: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("session not found: %s", id)
	}
	return nil
}

func (r *SessionRepository) ListByUser(ctx context.Context, userID string, limit, offset int) ([]domain.Session, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, tenant_id, workspace_id, user_id, model_plane_version, status, plan_mode, metadata, created_at, updated_at, org_id, user_role
		FROM sessions WHERE user_id = $1
		ORDER BY created_at DESC
		LIMIT $2 OFFSET $3
	`, userID, limit, offset)
	if err != nil {
		return nil, fmt.Errorf("list sessions: %w", err)
	}
	defer rows.Close()

	var sessions []domain.Session
	for rows.Next() {
		var s domain.Session
		var metadataJSON []byte
		if err := rows.Scan(
			&s.ID, &s.TenantID, &s.WorkspaceID, &s.UserID,
			&s.ModelPlaneVersion, &s.Status, &s.PlanMode,
			&metadataJSON, &s.CreatedAt, &s.UpdatedAt, &s.OrgID, &s.UserRole,
		); err != nil {
			return nil, fmt.Errorf("scan session: %w", err)
		}
		if metadataJSON != nil {
			_ = json.Unmarshal(metadataJSON, &s.Metadata)
		}
		sessions = append(sessions, s)
	}
	return sessions, nil
}

// --- Events ---

func (r *SessionRepository) AppendEvent(ctx context.Context, sessionID, eventType string, payload []byte) (*domain.SessionEvent, error) {
	evt := &domain.SessionEvent{}
	err := r.pool.QueryRow(ctx, `
		INSERT INTO session_events (session_id, sequence, event_type, payload)
		VALUES ($1, next_session_event_sequence($1), $2, $3)
		RETURNING id, session_id, sequence, event_type, payload, created_at
	`, sessionID, eventType, payload).Scan(
		&evt.ID, &evt.SessionID, &evt.Sequence, &evt.EventType, &evt.Payload, &evt.CreatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("append event: %w", err)
	}
	return evt, nil
}

func (r *SessionRepository) GetEventsSince(ctx context.Context, sessionID string, afterSequence int64, limit int) ([]domain.SessionEvent, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, session_id, sequence, event_type, payload, created_at
		FROM session_events
		WHERE session_id = $1 AND sequence > $2
		ORDER BY sequence ASC
		LIMIT $3
	`, sessionID, afterSequence, limit)
	if err != nil {
		return nil, fmt.Errorf("get events: %w", err)
	}
	defer rows.Close()

	var events []domain.SessionEvent
	for rows.Next() {
		var e domain.SessionEvent
		if err := rows.Scan(&e.ID, &e.SessionID, &e.Sequence, &e.EventType, &e.Payload, &e.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan event: %w", err)
		}
		events = append(events, e)
	}
	return events, nil
}

// --- Approvals ---

func (r *SessionRepository) CreateApproval(ctx context.Context, a *domain.Approval) error {
	return r.pool.QueryRow(ctx, `
		INSERT INTO approval_queue (session_id, tool_name, tool_input)
		VALUES ($1, $2, $3)
		RETURNING id, created_at
	`, a.SessionID, a.ToolName, a.ToolInput).Scan(&a.ID, &a.CreatedAt)
}

func (r *SessionRepository) ResolveApproval(ctx context.Context, id string, approved bool, feedback string) error {
	status := domain.ApprovalRejected
	if approved {
		status = domain.ApprovalApproved
	}

	now := time.Now()
	tag, err := r.pool.Exec(ctx, `
		UPDATE approval_queue SET status = $1, feedback = $2, resolved_at = $3
		WHERE id = $4 AND status = 'pending'
	`, status, feedback, now, id)
	if err != nil {
		return fmt.Errorf("resolve approval: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("approval not found or already resolved: %s", id)
	}
	return nil
}

func (r *SessionRepository) GetPendingApprovals(ctx context.Context, sessionID string) ([]domain.Approval, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, session_id, tool_name, tool_input, status, feedback, resolved_at, created_at
		FROM approval_queue
		WHERE session_id = $1 AND status = 'pending'
		ORDER BY created_at ASC
	`, sessionID)
	if err != nil {
		return nil, fmt.Errorf("get approvals: %w", err)
	}
	defer rows.Close()

	var approvals []domain.Approval
	for rows.Next() {
		var a domain.Approval
		if err := rows.Scan(&a.ID, &a.SessionID, &a.ToolName, &a.ToolInput, &a.Status, &a.Feedback, &a.ResolvedAt, &a.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan approval: %w", err)
		}
		approvals = append(approvals, a)
	}
	return approvals, nil
}
