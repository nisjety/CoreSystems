package store

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Proposal kinds and statuses are mirrored from the CHECK constraints in
// migration 0002 so handlers can validate input before hitting the DB.
const (
	ProposalKindDelete  = "delete"
	ProposalKindArchive = "archive"

	ProposalStatusPending  = "pending"
	ProposalStatusApproved = "approved"
	ProposalStatusRejected = "rejected"
	ProposalStatusExecuted = "executed"
	ProposalStatusFailed   = "failed"
)

// Proposal mirrors one row of review_proposals.
type Proposal struct {
	ID             uuid.UUID   `json:"id"`
	OrganizationID string      `json:"organization_id"`
	ProposedBy     string      `json:"proposed_by"`
	Kind           string      `json:"kind"`
	Reason         string      `json:"reason"`
	ItemPKs        []uuid.UUID `json:"item_pks"`
	Status         string      `json:"status"`
	DecidedBy      string      `json:"decided_by,omitempty"`
	DecidedAt      *time.Time  `json:"decided_at,omitempty"`
	ExecutedAt     *time.Time  `json:"executed_at,omitempty"`
	FailureReason  string      `json:"failure_reason,omitempty"`
	Notes          string      `json:"notes,omitempty"`
	CreatedAt      time.Time   `json:"created_at"`
	UpdatedAt      time.Time   `json:"updated_at"`
}

// CreateProposalInput is the write-side payload for a new proposal.
type CreateProposalInput struct {
	OrganizationID string
	ProposedBy     string
	Kind           string
	Reason         string
	ItemPKs        []uuid.UUID
	Notes          string
}

// ErrInvalidTransition is returned when a state change is not allowed
// from the proposal's current status (e.g. approving an executed proposal).
var ErrInvalidTransition = errors.New("store: invalid proposal state transition")

type Proposals struct {
	pool *pgxpool.Pool
}

func (p *Proposals) Create(ctx context.Context, in CreateProposalInput) (Proposal, error) {
	if len(in.ItemPKs) == 0 {
		return Proposal{}, fmt.Errorf("proposal must reference at least one item")
	}
	if in.Kind != ProposalKindDelete && in.Kind != ProposalKindArchive {
		return Proposal{}, fmt.Errorf("invalid proposal kind %q", in.Kind)
	}

	const q = `
INSERT INTO review_proposals (organization_id, proposed_by, kind, reason, item_pks, notes)
VALUES ($1, $2, $3, $4, $5, NULLIF($6, ''))
RETURNING id, organization_id, proposed_by, kind, reason, item_pks, status,
          COALESCE(decided_by, ''), decided_at, executed_at,
          COALESCE(failure_reason, ''), COALESCE(notes, ''), created_at, updated_at`
	var out Proposal
	err := p.pool.QueryRow(ctx, q, in.OrganizationID, in.ProposedBy, in.Kind, in.Reason, in.ItemPKs, in.Notes).Scan(
		&out.ID, &out.OrganizationID, &out.ProposedBy, &out.Kind, &out.Reason, &out.ItemPKs, &out.Status,
		&out.DecidedBy, &out.DecidedAt, &out.ExecutedAt,
		&out.FailureReason, &out.Notes, &out.CreatedAt, &out.UpdatedAt,
	)
	if err != nil {
		return Proposal{}, fmt.Errorf("create proposal: %w", err)
	}
	return out, nil
}

func (p *Proposals) Get(ctx context.Context, id uuid.UUID) (Proposal, error) {
	const q = `
SELECT id, organization_id, proposed_by, kind, reason, item_pks, status,
       COALESCE(decided_by, ''), decided_at, executed_at,
       COALESCE(failure_reason, ''), COALESCE(notes, ''), created_at, updated_at
  FROM review_proposals
 WHERE id = $1`
	var out Proposal
	err := p.pool.QueryRow(ctx, q, id).Scan(
		&out.ID, &out.OrganizationID, &out.ProposedBy, &out.Kind, &out.Reason, &out.ItemPKs, &out.Status,
		&out.DecidedBy, &out.DecidedAt, &out.ExecutedAt,
		&out.FailureReason, &out.Notes, &out.CreatedAt, &out.UpdatedAt,
	)
	if errors.Is(err, pgxNoRows) {
		return Proposal{}, ErrNotFound
	}
	if err != nil {
		return Proposal{}, fmt.Errorf("get proposal: %w", err)
	}
	return out, nil
}

// ListByOrg lists proposals for an organization filtered by status.
// status="" returns everything.
func (p *Proposals) ListByOrg(ctx context.Context, organizationID, status string, limit int) ([]Proposal, error) {
	if limit <= 0 || limit > 500 {
		limit = 100
	}
	const q = `
SELECT id, organization_id, proposed_by, kind, reason, item_pks, status,
       COALESCE(decided_by, ''), decided_at, executed_at,
       COALESCE(failure_reason, ''), COALESCE(notes, ''), created_at, updated_at
  FROM review_proposals
 WHERE organization_id = $1
   AND ($2 = '' OR status = $2)
 ORDER BY created_at DESC
 LIMIT $3`
	rows, err := p.pool.Query(ctx, q, organizationID, status, limit)
	if err != nil {
		return nil, fmt.Errorf("list proposals: %w", err)
	}
	defer rows.Close()
	var out []Proposal
	for rows.Next() {
		var r Proposal
		if err := rows.Scan(
			&r.ID, &r.OrganizationID, &r.ProposedBy, &r.Kind, &r.Reason, &r.ItemPKs, &r.Status,
			&r.DecidedBy, &r.DecidedAt, &r.ExecutedAt,
			&r.FailureReason, &r.Notes, &r.CreatedAt, &r.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Decide transitions a pending proposal to either approved or rejected.
// Any other current status returns ErrInvalidTransition — the audit_log
// remains authoritative for the history; this just enforces a sane lifecycle.
func (p *Proposals) Decide(ctx context.Context, id uuid.UUID, newStatus, decidedBy, notes string) (Proposal, error) {
	if newStatus != ProposalStatusApproved && newStatus != ProposalStatusRejected {
		return Proposal{}, fmt.Errorf("Decide: status %q is not a decision", newStatus)
	}
	const q = `
UPDATE review_proposals
   SET status     = $2,
       decided_by = $3,
       decided_at = NOW(),
       notes      = COALESCE(NULLIF($4, ''), notes)
 WHERE id = $1 AND status = 'pending'
RETURNING id, organization_id, proposed_by, kind, reason, item_pks, status,
          COALESCE(decided_by, ''), decided_at, executed_at,
          COALESCE(failure_reason, ''), COALESCE(notes, ''), created_at, updated_at`
	var out Proposal
	err := p.pool.QueryRow(ctx, q, id, newStatus, decidedBy, notes).Scan(
		&out.ID, &out.OrganizationID, &out.ProposedBy, &out.Kind, &out.Reason, &out.ItemPKs, &out.Status,
		&out.DecidedBy, &out.DecidedAt, &out.ExecutedAt,
		&out.FailureReason, &out.Notes, &out.CreatedAt, &out.UpdatedAt,
	)
	if errors.Is(err, pgxNoRows) {
		// Either the proposal does not exist, or it is no longer pending.
		// Distinguish the two with a second Get so the caller can choose
		// between 404 and 409.
		_, getErr := p.Get(ctx, id)
		if errors.Is(getErr, ErrNotFound) {
			return Proposal{}, ErrNotFound
		}
		return Proposal{}, ErrInvalidTransition
	}
	if err != nil {
		return Proposal{}, fmt.Errorf("decide proposal: %w", err)
	}
	return out, nil
}

// SetExecutionResult transitions an approved proposal to executed or failed.
// Only an approved proposal may be executed — any other current status returns
// ErrInvalidTransition so a double-execute or an execute-before-approve is
// rejected at the database level, not just in the handler.
func (p *Proposals) SetExecutionResult(ctx context.Context, id uuid.UUID, newStatus, failureReason string) (Proposal, error) {
	if newStatus != ProposalStatusExecuted && newStatus != ProposalStatusFailed {
		return Proposal{}, fmt.Errorf("SetExecutionResult: status %q is not an execution outcome", newStatus)
	}
	const q = `
UPDATE review_proposals
   SET status         = $2,
       failure_reason = NULLIF($3, ''),
       executed_at    = NOW()
 WHERE id = $1 AND status = 'approved'
RETURNING id, organization_id, proposed_by, kind, reason, item_pks, status,
          COALESCE(decided_by, ''), decided_at, executed_at,
          COALESCE(failure_reason, ''), COALESCE(notes, ''), created_at, updated_at`
	var out Proposal
	err := p.pool.QueryRow(ctx, q, id, newStatus, failureReason).Scan(
		&out.ID, &out.OrganizationID, &out.ProposedBy, &out.Kind, &out.Reason, &out.ItemPKs, &out.Status,
		&out.DecidedBy, &out.DecidedAt, &out.ExecutedAt,
		&out.FailureReason, &out.Notes, &out.CreatedAt, &out.UpdatedAt,
	)
	if errors.Is(err, pgxNoRows) {
		_, getErr := p.Get(ctx, id)
		if errors.Is(getErr, ErrNotFound) {
			return Proposal{}, ErrNotFound
		}
		return Proposal{}, ErrInvalidTransition
	}
	if err != nil {
		return Proposal{}, fmt.Errorf("set execution result: %w", err)
	}
	return out, nil
}

// Begin starts a tx and exposes pgx.Tx for callers that need to combine a
// proposal update with audit_log writes atomically.
func (p *Proposals) Begin(ctx context.Context) (pgx.Tx, error) {
	return p.pool.BeginTx(ctx, pgx.TxOptions{})
}
