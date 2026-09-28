package users

import (
	"context"
	"fmt"
	"strings"
)

// Admin-succession pre-flight for self/admin erasure (design doc Flow B).
//
// A user who is the ONLY active owner/admin of an organization must not be
// able to self-erase (or be erased) without first nominating a successor —
// otherwise the organization is left leaderless. This file is the pre-flight
// gate the HTTP handlers (internal/http/gdpr_handlers.go) run BEFORE touching
// the erasure saga (gdpr_erasure_saga.go). It never mutates the erasure saga
// itself, so it is invisible to the resumable-saga tests in
// gdpr_erasure_saga_postgres_test.go, and to any future caller of
// HardEraseUser/AnonymizeUser that isn't the HTTP self/admin path (e.g. a
// future scheduled-deletion sweep).

// ErrSuccessorRequired is returned when the subject is the sole owner/admin of
// one or more organizations and no successor_user_id was supplied. `Orgs`
// lists every affected organization so the caller can present a picker.
type ErrSuccessorRequired struct {
	Orgs []SoleAdminOrg
}

func (e *ErrSuccessorRequired) Error() string {
	return "successor required for sole-admin organizations"
}

// ErrSuccessorInvalid is returned when a successor_user_id was supplied but
// fails validation for a specific organization (not an active member, or is
// the departing user themselves).
type ErrSuccessorInvalid struct {
	OrgID  string
	Reason string
}

func (e *ErrSuccessorInvalid) Error() string {
	return fmt.Sprintf("invalid successor for organization %s: %s", e.OrgID, e.Reason)
}

// OrgCoreSuccessionClient promotes a successor into a departing sole-admin's
// role in org-core (POST /internal/orgs/:orgId/members/:userId/succession).
// Implemented by clients.OrgCoreClient in production; tests supply a fake so
// the gate is exercised without a live org-core.
type OrgCoreSuccessionClient interface {
	PromoteMemberSuccession(ctx context.Context, orgID, successorUserID, role string) error
}

// SetOrgCoreClient wires the org-core succession client. A nil client (the
// zero value, and the default when the deployment has not configured
// ORG_CORE_BASE_URL / service credentials) makes EnsureSuccession fail closed
// for any user who actually needs a successor, rather than silently skipping
// the handoff.
func (s *Service) SetOrgCoreClient(client OrgCoreSuccessionClient) {
	s.orgCoreClient = client
}

// SoleAdminOrgs returns every organization where userID is the ONLY active
// owner/admin.
func (s *Service) SoleAdminOrgs(ctx context.Context, userID string) ([]SoleAdminOrg, error) {
	userID = strings.TrimSpace(userID)
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	if s == nil || s.repo == nil {
		return nil, fmt.Errorf("user repository is unavailable")
	}
	return s.repo.SoleAdminOrgs(ctx, userID)
}

// EnsureSuccession is the pre-flight gate a self/admin erasure must pass
// before the erasure saga is touched. When targetID is the sole owner/admin
// of one or more organizations, EVERY such organization must receive a
// validated successor:
//   - successorID must be an ACTIVE member of that org
//   - successorID must not be targetID itself
//   - org-core's succession endpoint must accept the promotion (synchronous;
//     any failure aborts before erasure starts, matching this file's other
//     fail-closed gates, e.g. erasureUnavailable)
//
// Returns *ErrSuccessorRequired when no successor was supplied at all,
// *ErrSuccessorInvalid when the supplied successor fails validation for a
// specific org, or nil once every sole-admin org has been handed off. The
// SAME successorID is used for every affected org — Verevon's one-org-per-user
// model makes more than one sole-admin org a rare edge case. When it happens,
// the caller must nominate someone who is an active member of ALL of them.
func (s *Service) EnsureSuccession(ctx context.Context, targetID, successorID string) error {
	targetID = strings.TrimSpace(targetID)
	orgs, err := s.SoleAdminOrgs(ctx, targetID)
	if err != nil {
		return fmt.Errorf("resolve sole-admin organizations: %w", err)
	}
	if len(orgs) == 0 {
		return nil
	}

	successorID = strings.TrimSpace(successorID)
	if successorID == "" {
		return &ErrSuccessorRequired{Orgs: orgs}
	}
	if successorID == targetID {
		return &ErrSuccessorInvalid{OrgID: orgs[0].OrgID, Reason: "successor cannot be the departing user"}
	}
	if s.orgCoreClient == nil {
		return fmt.Errorf("org-core succession client is not configured")
	}

	for _, org := range orgs {
		membership, err := s.repo.GetUserOrgMembership(ctx, successorID, org.OrgID)
		if err != nil {
			return fmt.Errorf("validate successor for organization %s: %w", org.OrgID, err)
		}
		if membership == nil {
			return &ErrSuccessorInvalid{OrgID: org.OrgID, Reason: "successor is not an active member of this organization"}
		}

		role := "owner"
		if current, err := s.repo.GetUserOrgMembership(ctx, targetID, org.OrgID); err != nil {
			return fmt.Errorf("resolve departing role for organization %s: %w", org.OrgID, err)
		} else if current != nil && current.Role != "" {
			role = current.Role
		}

		if err := s.orgCoreClient.PromoteMemberSuccession(ctx, org.OrgID, successorID, role); err != nil {
			return fmt.Errorf("promote successor in organization %s: %w", org.OrgID, err)
		}
	}
	return nil
}
