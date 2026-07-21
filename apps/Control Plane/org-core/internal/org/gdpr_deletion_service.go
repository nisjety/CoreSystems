package org

import (
	"context"
	"fmt"
	"log"
	"strings"
)

// ErrOrgNameMismatch is returned by SoftDelete when the caller-supplied
// org_name confirmation does not exactly (case-sensitively) match the
// organization's real, server-fetched name. The HTTP layer maps this to 400
// with a clear message. Modeled on user-core's *ErrSuccessorRequired typed-
// error pattern (internal/users/gdpr_succession.go) so the HTTP layer can
// distinguish this failure from a generic erasure error with errors.As.
type ErrOrgNameMismatch struct {
	OrgID string
}

func (e *ErrOrgNameMismatch) Error() string {
	return fmt.Sprintf("organization name confirmation does not match organization %s", e.OrgID)
}

// RestoreOrganization reverses a pending soft-delete for orgID: it clears
// deleted_at/status back to active, wipes the org's deletion ledger, and
// publishes velion.org.deletion.cancelled. Returns
// ErrOrganizationNotPendingDeletion (the HTTP layer maps this to 409) if the
// organization is not currently inside its 30-day grace window — this also
// makes a duplicate restore request safe: the first call restores and
// clears the ledger, any retry correctly fails closed with the same
// sentinel rather than reprocessing.
func (s *Service) RestoreOrganization(ctx context.Context, orgID, actorID, actorRole string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("organization id is required")
	}

	if err := s.repo.RestoreOrganization(ctx, orgID); err != nil {
		return err
	}

	if err := s.repo.DeleteDeletionLedger(ctx, orgID); err != nil {
		log.Printf("org-core: restore %s: delete deletion ledger failed: %v", orgID, err)
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}

	orgName := ""
	if org, err := s.repo.GetOrganization(ctx, orgID); err == nil && org != nil {
		orgName = org.Name
	}
	s.publishDeletionCancelled(orgID, orgName, actorID)
	log.Printf("org-core GDPR: organization %s deletion cancelled by %s (role=%s)", orgID, actorID, actorRole)
	return nil
}

// publishDeletionCancelled emits velion.org.deletion.cancelled. A nil shared
// publisher (velion-nats disabled) makes this a no-op.
func (s *Service) publishDeletionCancelled(orgID, orgName, cancelledBy string) {
	sp := s.SharedPub()
	if sp == nil {
		return
	}
	sp.PublishPlain("velion.org.deletion.cancelled", map[string]any{
		"org_id":       orgID,
		"org_name":     orgName,
		"cancelled_by": cancelledBy,
	})
}

// MarkDeletionExported records that userID (acting on their own behalf) has
// received their personal-data export ahead of orgID's scheduled purge.
// Returns ErrNotFound if no ledger row exists for (orgID, userID) — e.g. the
// org is not pending deletion, or userID was not an active member when the
// ledger was created. Idempotent: calling it again simply re-stamps NOW().
func (s *Service) MarkDeletionExported(ctx context.Context, orgID, userID string) error {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}
	if err := s.repo.MarkExported(ctx, orgID, userID); err != nil {
		return err
	}
	log.Printf("org-core GDPR: member %s marked personal-data export received for pending-deletion org %s", userID, orgID)
	return nil
}

// MarkDeletionAcknowledged records that userID (acting on their own behalf)
// has acknowledged orgID's pending-deletion notice. Same semantics as
// MarkDeletionExported.
func (s *Service) MarkDeletionAcknowledged(ctx context.Context, orgID, userID string) error {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}
	if err := s.repo.MarkAcknowledged(ctx, orgID, userID); err != nil {
		return err
	}
	log.Printf("org-core GDPR: member %s acknowledged pending deletion for org %s", userID, orgID)
	return nil
}

// ListDeletionLedger returns every member's export/acknowledgement
// checkpoint for orgID. Owner/admin-only exposure is enforced by the HTTP
// layer, not here.
func (s *Service) ListDeletionLedger(ctx context.Context, orgID string) ([]DeletionLedgerEntry, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	return s.repo.ListDeletionLedger(ctx, orgID)
}

// GetDeletionStatus answers GET /orgs/:id/gdpr/deletion/status: whether
// orgID is currently pending deletion, the calling member's own checkpoint,
// and — only when includeAllMembers is true (the HTTP layer decides this
// from the caller's org-level or platform role) — every member's ledger row.
// Returns ErrNotFound if orgID does not exist at all.
func (s *Service) GetDeletionStatus(ctx context.Context, orgID, callerID string, includeAllMembers bool) (*DeletionStatus, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	org, err := s.repo.GetDeletionStatus(ctx, orgID)
	if err != nil {
		return nil, err
	}

	status := &DeletionStatus{
		OrgName: org.Name,
		Pending: org.DeletedAt != nil,
	}
	if org.DeletedAt != nil {
		deadline := org.DeletedAt.AddDate(0, 0, deletionGracePeriodDays)
		status.Deadline = &deadline
	}

	entries, err := s.ListDeletionLedger(ctx, orgID)
	if err != nil {
		return nil, err
	}
	for i := range entries {
		if entries[i].UserID == callerID {
			status.MemberStatus = &entries[i]
			break
		}
	}
	if includeAllMembers {
		status.Members = entries
	}
	return status, nil
}
