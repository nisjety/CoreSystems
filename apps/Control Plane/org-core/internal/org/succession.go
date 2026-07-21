package org

import (
	"context"
	"fmt"
	"strings"
)

// PromoteMemberSuccession promotes an EXISTING active member into a base role
// (owner|admin) as part of an admin-succession handoff. It is the service-layer
// entry point for internal/http/succession_handlers.go's
// POST /internal/orgs/:orgId/members/:userId/succession, called synchronously
// by user-core's self-erasure pre-flight (Service.EnsureSuccession in
// user-core's internal/users/gdpr_succession.go) before that user's erasure
// saga starts. See Repository.PromoteMember for the underlying mutation and
// migrations/010_owner_invariant.up.sql for the DB-level invariant this
// respects.
func (s *Service) PromoteMemberSuccession(ctx context.Context, orgID, userID, role string) error {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	role = strings.ToLower(strings.TrimSpace(role))
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}
	if role != "owner" && role != "admin" {
		return fmt.Errorf("role must be owner or admin")
	}
	if s == nil || s.repo == nil {
		return fmt.Errorf("organization repository is unavailable")
	}
	return s.repo.PromoteMember(ctx, orgID, userID, role)
}
