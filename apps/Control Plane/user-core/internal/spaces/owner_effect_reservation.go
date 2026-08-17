package spaces

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

var ErrOwnerEffectReservationDenied = errors.New("owner effect reservation denied")
var ErrOwnerEffectReservationConflict = errors.New("owner effect reservation conflicts with existing operation")

// OwnerEffectReservationCommitment is the immutable, content-free identifier
// set that an owner plane submits to Control. It is deliberately separate from
// RunActionDecision: Control owns source-authority ordering while the owner
// plane retains target-resource authorization and effect contents.
type OwnerEffectReservationCommitment struct {
	OperationID      string `json:"operation_id"`
	ActionID         string `json:"action_id"`
	ActionSchemaHash string `json:"action_schema_hash"`
	PayloadDigest    string `json:"payload_digest"`
	IdempotencyKey   string `json:"idempotency_key"`
	DecisionRef      string `json:"decision_ref"`
	GrantRef         string `json:"grant_ref"`
}

func (c OwnerEffectReservationCommitment) Validate() error {
	for name, value := range map[string]string{
		"operation_id": c.OperationID, "action_id": c.ActionID,
		"action_schema_hash": c.ActionSchemaHash, "payload_digest": c.PayloadDigest,
		"idempotency_key": c.IdempotencyKey, "decision_ref": c.DecisionRef,
		"grant_ref": c.GrantRef,
	} {
		value = strings.TrimSpace(value)
		if value == "" || len(value) > 200 {
			return fmt.Errorf("owner effect reservation %s is invalid", name)
		}
	}
	if audience, found := runActionTarget(strings.TrimSpace(c.ActionID)); !found || audience != ticketsCreateServiceAudience {
		return fmt.Errorf("owner effect reservation action is not approved")
	}
	if !validSHA256Commitment(strings.TrimSpace(c.ActionSchemaHash)) || !validSHA256Commitment(strings.TrimSpace(c.PayloadDigest)) {
		return fmt.Errorf("owner effect reservation commitments are invalid")
	}
	return nil
}

// Matches is deliberately exact. It is used for idempotent reservation replay:
// reusing an operation ID with even one changed immutable fact is a conflict,
// not a second authorization attempt.
func (c OwnerEffectReservationCommitment) Matches(other OwnerEffectReservationCommitment) bool {
	return strings.TrimSpace(c.OperationID) == strings.TrimSpace(other.OperationID) &&
		strings.TrimSpace(c.ActionID) == strings.TrimSpace(other.ActionID) &&
		strings.TrimSpace(c.ActionSchemaHash) == strings.TrimSpace(other.ActionSchemaHash) &&
		strings.TrimSpace(c.PayloadDigest) == strings.TrimSpace(other.PayloadDigest) &&
		strings.TrimSpace(c.IdempotencyKey) == strings.TrimSpace(other.IdempotencyKey) &&
		strings.TrimSpace(c.DecisionRef) == strings.TrimSpace(other.DecisionRef) &&
		strings.TrimSpace(c.GrantRef) == strings.TrimSpace(other.GrantRef)
}

// OwnerEffectReservation is Control's durable ordering receipt. It never
// contains a signed decision bearer, target resource body, or service secret.
type OwnerEffectReservation struct {
	ReservationID string
	Commitment    OwnerEffectReservationCommitment
	OrgID         string
	SpaceRef      string
	SubjectID     string
	RunID         string
	ThreadID      string
	AudienceRef   string
	AudienceHash  string
	AudienceRev   int64
	PrivacyRef    string
	AuthorityRev  int64
	Status        string
	ExpiresAt     time.Time
	CommittedAt   *time.Time
	CancelledAt   *time.Time
	Reason        string
}

func (r *Repository) ReserveOwnerEffect(ctx context.Context, decision RunActionDecision, commitment OwnerEffectReservationCommitment, now time.Time) (*OwnerEffectReservation, error) {
	if r == nil || r.db == nil || now.IsZero() || decision.ExpiresAt.Before(now) {
		return nil, ErrOwnerEffectReservationDenied
	}
	if err := decision.Validate(); err != nil {
		return nil, ErrOwnerEffectReservationDenied
	}
	if err := commitment.Validate(); err != nil || commitment.ActionID != decision.ActionID ||
		commitment.ActionSchemaHash != decision.ActionSchemaHash || commitment.PayloadDigest != decision.PayloadDigest ||
		commitment.IdempotencyKey != decision.IdempotencyKey || commitment.DecisionRef != decision.DecisionRef {
		return nil, ErrOwnerEffectReservationDenied
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin owner effect reservation: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	var authorityFence int
	if err := tx.QueryRow(ctx, `SELECT 1 FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, decision.SpaceRef).Scan(&authorityFence); err != nil {
		return nil, ErrOwnerEffectReservationDenied
	}
	existing, err := ownerEffectReservationForUpdate(ctx, tx, commitment.OperationID)
	if err == nil {
		if !existing.Commitment.Matches(commitment) {
			return nil, ErrOwnerEffectReservationConflict
		}
		if err := tx.Commit(ctx); err != nil {
			return nil, err
		}
		return existing, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return nil, err
	}
	if err := r.validateCurrentOwnerEffectAuthority(ctx, decision, now); err != nil {
		return nil, err
	}
	reservationID, err := randomOwnerEffectReservationID()
	if err != nil {
		return nil, err
	}
	reservation := &OwnerEffectReservation{
		ReservationID: reservationID, Commitment: commitment,
		OrgID: decision.OrgID, SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID,
		RunID: decision.RunID, ThreadID: decision.ThreadID,
		AudienceRef: decision.RecipientAudienceRef, AudienceHash: decision.RecipientAudienceHash,
		AudienceRev: decision.RecipientAudienceRevision, PrivacyRef: decision.PrivacyPolicyRef,
		AuthorityRev: decision.AuthorityRevision, Status: "reserved", ExpiresAt: decision.ExpiresAt.UTC(),
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO space_owner_effect_reservations (
 operation_id, reservation_id, org_id, space_ref, subject_id, run_id, thread_id,
 action_id, action_schema_hash, payload_digest, idempotency_key, decision_ref, grant_ref,
 recipient_audience_ref, recipient_audience_hash, recipient_audience_revision,
 privacy_policy_ref, authority_revision, status, expires_at
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,'reserved',$19)`,
		commitment.OperationID, reservation.ReservationID, decision.OrgID, decision.SpaceRef, decision.SubjectID, decision.RunID, decision.ThreadID,
		decision.ActionID, decision.ActionSchemaHash, decision.PayloadDigest, decision.IdempotencyKey, decision.DecisionRef, commitment.GrantRef,
		decision.RecipientAudienceRef, decision.RecipientAudienceHash, decision.RecipientAudienceRevision,
		decision.PrivacyPolicyRef, decision.AuthorityRevision, reservation.ExpiresAt,
	); err != nil {
		return nil, fmt.Errorf("insert owner effect reservation: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	return reservation, nil
}

// CommitOwnerEffectReservation is the Control authorization linearization
// point. The caller supplies the same signed decision only for this direct
// hop; it is verified at the HTTP boundary and never persisted here.
func (r *Repository) CommitOwnerEffectReservation(ctx context.Context, reservationID string, decision RunActionDecision, now time.Time) (*OwnerEffectReservation, error) {
	if r == nil || r.db == nil || strings.TrimSpace(reservationID) == "" || now.IsZero() || decision.ExpiresAt.Before(now) {
		return nil, ErrOwnerEffectReservationDenied
	}
	if err := decision.Validate(); err != nil {
		return nil, ErrOwnerEffectReservationDenied
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin owner effect commit: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	reservation, err := ownerEffectReservationByIDForUpdate(ctx, tx, reservationID)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrOwnerEffectReservationDenied
	}
	if err != nil {
		return nil, err
	}
	if !reservationMatchesDecision(reservation, decision) {
		return nil, ErrOwnerEffectReservationDenied
	}
	if reservation.Status == "cancelled" || !reservation.ExpiresAt.After(now) {
		return nil, ErrOwnerEffectReservationDenied
	}
	if reservation.Status == "committed" {
		if err := tx.Commit(ctx); err != nil {
			return nil, err
		}
		return reservation, nil
	}
	var authorityFence int
	if err := tx.QueryRow(ctx, `SELECT 1 FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, decision.SpaceRef).Scan(&authorityFence); err != nil {
		return nil, ErrOwnerEffectReservationDenied
	}
	if err := r.validateCurrentOwnerEffectAuthority(ctx, decision, now); err != nil {
		return nil, err
	}
	committedAt := now.UTC()
	if _, err := tx.Exec(ctx, `UPDATE space_owner_effect_reservations
SET status='committed', committed_at=$2, updated_at=$2
WHERE reservation_id=$1 AND status='reserved'`, reservation.ReservationID, committedAt); err != nil {
		return nil, fmt.Errorf("commit owner effect reservation: %w", err)
	}
	reservation.Status, reservation.CommittedAt = "committed", &committedAt
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	return reservation, nil
}

func (r *Repository) GetOwnerEffectReservation(ctx context.Context, reservationID string) (*OwnerEffectReservation, error) {
	if r == nil || r.db == nil || strings.TrimSpace(reservationID) == "" {
		return nil, ErrOwnerEffectReservationDenied
	}
	reservation, err := ownerEffectReservationByID(ctx, r.db.Pool, reservationID)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrOwnerEffectReservationDenied
	}
	return reservation, err
}

func (r *Repository) validateCurrentOwnerEffectAuthority(ctx context.Context, decision RunActionDecision, now time.Time) error {
	evidence, err := r.ResolveAgentActionDecisionEvidence(ctx, decision.SpaceRef, decision.OrgID, decision.SubjectID)
	if err != nil {
		return ErrOwnerEffectReservationDenied
	}
	if err := ValidateCurrentRunActionDecision(evidence, decision); err != nil || decision.ExpiresAt.Before(now) {
		return ErrOwnerEffectReservationDenied
	}
	return nil
}

type ownerEffectReservationQuery interface {
	QueryRow(context.Context, string, ...any) pgx.Row
}

const ownerEffectReservationSelect = `
SELECT reservation_id, operation_id, org_id, space_ref, subject_id, run_id, thread_id,
       action_id, action_schema_hash, payload_digest,
       idempotency_key, decision_ref, grant_ref, status, expires_at,
       committed_at, cancelled_at, cancellation_reason,
       recipient_audience_ref, recipient_audience_hash, recipient_audience_revision,
       privacy_policy_ref, authority_revision
FROM space_owner_effect_reservations`

func ownerEffectReservationForUpdate(ctx context.Context, tx pgx.Tx, operationID string) (*OwnerEffectReservation, error) {
	return scanOwnerEffectReservation(tx.QueryRow(ctx, ownerEffectReservationSelect+` WHERE operation_id=$1 FOR UPDATE`, operationID))
}

func ownerEffectReservationByIDForUpdate(ctx context.Context, tx pgx.Tx, reservationID string) (*OwnerEffectReservation, error) {
	return scanOwnerEffectReservation(tx.QueryRow(ctx, ownerEffectReservationSelect+` WHERE reservation_id=$1 FOR UPDATE`, reservationID))
}

func ownerEffectReservationByID(ctx context.Context, db ownerEffectReservationQuery, reservationID string) (*OwnerEffectReservation, error) {
	return scanOwnerEffectReservation(db.QueryRow(ctx, ownerEffectReservationSelect+` WHERE reservation_id=$1`, reservationID))
}

func scanOwnerEffectReservation(row pgx.Row) (*OwnerEffectReservation, error) {
	reservation := &OwnerEffectReservation{}
	var committedAt, cancelledAt *time.Time
	if err := row.Scan(&reservation.ReservationID, &reservation.Commitment.OperationID, &reservation.OrgID,
		&reservation.SpaceRef, &reservation.SubjectID, &reservation.RunID, &reservation.ThreadID,
		&reservation.Commitment.ActionID, &reservation.Commitment.ActionSchemaHash, &reservation.Commitment.PayloadDigest, &reservation.Commitment.IdempotencyKey,
		&reservation.Commitment.DecisionRef, &reservation.Commitment.GrantRef, &reservation.Status, &reservation.ExpiresAt,
		&committedAt, &cancelledAt, &reservation.Reason, &reservation.AudienceRef, &reservation.AudienceHash,
		&reservation.AudienceRev, &reservation.PrivacyRef, &reservation.AuthorityRev); err != nil {
		return nil, err
	}
	reservation.CommittedAt, reservation.CancelledAt = committedAt, cancelledAt
	return reservation, nil
}

func reservationMatchesDecision(reservation *OwnerEffectReservation, decision RunActionDecision) bool {
	return reservation != nil && reservation.Commitment.ActionID == decision.ActionID &&
		reservation.Commitment.ActionSchemaHash == decision.ActionSchemaHash &&
		reservation.Commitment.PayloadDigest == decision.PayloadDigest &&
		reservation.Commitment.IdempotencyKey == decision.IdempotencyKey &&
		reservation.Commitment.DecisionRef == decision.DecisionRef &&
		reservation.OrgID == decision.OrgID && reservation.SpaceRef == decision.SpaceRef &&
		reservation.SubjectID == decision.SubjectID && reservation.RunID == decision.RunID &&
		reservation.ThreadID == decision.ThreadID && reservation.AudienceRef == decision.RecipientAudienceRef &&
		reservation.AudienceHash == decision.RecipientAudienceHash && reservation.AudienceRev == decision.RecipientAudienceRevision &&
		reservation.PrivacyRef == decision.PrivacyPolicyRef && reservation.AuthorityRev == decision.AuthorityRevision
}

func randomOwnerEffectReservationID() (string, error) {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return "", fmt.Errorf("owner effect reservation entropy: %w", err)
	}
	return "owner_effect_reservation_" + hex.EncodeToString(bytes), nil
}
