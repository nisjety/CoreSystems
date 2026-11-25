package org

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/jackc/pgx/v5"
)

// Legal holds against the reconcile erasure (F-f10-06, decision D-C1).
//
// The Auth Core deletion projection calls POST /internal/orgs/:orgId/reconcile-delete,
// which erases the organisation through gdpr_hard_delete_organization. With
// ERASURE_HOLD_CHECK on, an organisation under an active hold is not erased there
// either: the same refusal the HTTP erasure routes give, audited the same way. The
// anti-resurrection tombstone is still committed first, so a refused erasure stays
// incomplete, and the outbox's retry of the same revision resumes it once the holds
// are released.

// ErrErasureBlockedLegalHold is the typed refusal of an erasure while an organisation
// hold or a thread hold is active. errors.Is matches it on an *ErasureHeldError.
var ErrErasureBlockedLegalHold = errors.New("erasure blocked by an active legal hold")

// ErrErasureHoldCheckUnavailable means the legal holds could not be read, so nothing was
// erased: a hold that cannot be ruled out is treated as present.
var ErrErasureHoldCheckUnavailable = errors.New("erasure hold check unavailable")

// ErasureHeldError is one refused erasure with the active hold counts. It carries counts
// only, never a hold or organisation id.
type ErasureHeldError struct {
	OrgHolds    int
	ThreadHolds int
}

func (e *ErasureHeldError) Error() string {
	return fmt.Sprintf("%s: %d org and %d thread holds", ErrErasureBlockedLegalHold, e.OrgHolds, e.ThreadHolds)
}

// Unwrap makes errors.Is(err, ErrErasureBlockedLegalHold) hold for an *ErasureHeldError.
func (e *ErasureHeldError) Unwrap() error { return ErrErasureBlockedLegalHold }

// GDPRReconcileActorID is the actor of a refused reconcile erasure: the Auth Core
// deletion projection, which calls the internal route with its API key and names no user.
const GDPRReconcileActorID = "system:auth-core-deletion-projection"

// erasureHoldCheckOn reads ERASURE_HOLD_CHECK strictly: only the literal "true" turns the
// reconcile gate on. It is read on every call, as the HTTP erasure routes read it.
func erasureHoldCheckOn() bool {
	return os.Getenv(envErasureHoldCheck) == "true"
}

// holdGateTx is the part of pgx.Tx the reconcile gate reads through, so its decisions are
// tested without a database.
type holdGateTx interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// refuseHeldOrganizationErasure is the reconcile legal-hold gate. It runs inside the
// erasure transaction, after the tombstone is committed and before
// gdpr_hard_delete_organization, and returns *ErasureHeldError while the organisation has
// an active hold (released_at IS NULL).
//
// The organisation row is locked FOR UPDATE first. A hold is placed by an INSERT whose
// foreign key takes FOR KEY SHARE on that row, which conflicts with FOR UPDATE, so no hold
// can be placed between the count and the erasure that follows in the same transaction.
func refuseHeldOrganizationErasure(ctx context.Context, tx holdGateTx, orgID string) error {
	var lockedID string
	if err := tx.QueryRow(ctx, `SELECT id FROM organizations WHERE id = $1 FOR UPDATE`, orgID).Scan(&lockedID); err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return fmt.Errorf("%w: lock organization: %w", ErrErasureHoldCheckUnavailable, err)
	}
	var orgHolds, threadHolds int
	if err := tx.QueryRow(ctx, `
SELECT COUNT(*) FILTER (WHERE scope = 'org')::INT,
       COUNT(*) FILTER (WHERE scope <> 'org')::INT
FROM org_legal_holds
WHERE org_id = $1 AND released_at IS NULL`, orgID).Scan(&orgHolds, &threadHolds); err != nil {
		return fmt.Errorf("%w: read legal holds: %w", ErrErasureHoldCheckUnavailable, err)
	}
	if orgHolds+threadHolds > 0 {
		return &ErasureHeldError{OrgHolds: orgHolds, ThreadHolds: threadHolds}
	}
	return nil
}

// RecordHeldReconcileRefusal writes the refusal row for a reconcile erasure the gate
// refused: one row per refusal, the hard-delete op and the active counts, ids and counts
// only, under the projection actor. The HTTP route calls it after the erasure transaction
// has ended, so the audit write never waits on the organisation lock. The refusal stands
// whether or not the row is written; a failure is logged with a closed word only.
func (s *Service) RecordHeldReconcileRefusal(ctx context.Context, orgID string, held *ErasureHeldError) {
	if held == nil {
		return
	}
	if s.erasureRefusals == nil {
		log.Print("org-core: reconcile deletion: erasure_refusal_audit_unwired")
		return
	}
	actor := retention.Actor{ID: GDPRReconcileActorID, Role: "system"}
	if err := s.erasureRefusals.AuditRefusedErasure(ctx, orgID, actor, retention.ErasureOpHardDelete, held.OrgHolds, held.ThreadHolds); err != nil {
		log.Print("org-core: reconcile deletion: erasure_refusal_audit_failed")
	}
}
