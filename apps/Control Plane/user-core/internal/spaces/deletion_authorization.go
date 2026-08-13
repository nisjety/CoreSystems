package spaces

import (
	"fmt"
	"strings"
)

type DeletionAuthorizationRequest struct {
	SpaceRef         string `json:"space_ref"`
	OrgID            string `json:"org_id"`
	OwnerPrincipalID string `json:"owner_principal_id"`
	RequestID        string `json:"request_id"`
	IdempotencyKey   string `json:"idempotency_key"`
}

// DeletionPolicy is a Control-owned, organization-scoped rollout and
// entitlement floor. The first release phase permits only personal Spaces;
// later kinds require a separately reviewed policy/schema extension.
type DeletionPolicy struct {
	OrgID                  string `json:"org_id"`
	DeletionEntitled       bool   `json:"deletion_entitled"`
	PersonalRolloutEnabled bool   `json:"personal_rollout_enabled"`
}

// LegalHold is a Control-only retention fence. Its reference identifies the
// external legal/compliance record without copying that record's contents
// into the authorization database.
type LegalHold struct {
	SpaceRef string `json:"space_ref"`
	HoldRef  string `json:"hold_ref"`
}

func (p DeletionPolicy) Validate() error {
	if strings.TrimSpace(p.OrgID) == "" {
		return fmt.Errorf("Space deletion org_id is required")
	}
	return nil
}

func (h LegalHold) Validate() error {
	for name, value := range map[string]string{"space_ref": h.SpaceRef, "hold_ref": h.HoldRef} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("Space legal hold %s is required", name)
		}
	}
	return nil
}

func (r DeletionAuthorizationRequest) Validate() error {
	for name, value := range map[string]string{
		"space_ref": r.SpaceRef, "org_id": r.OrgID, "owner_principal_id": r.OwnerPrincipalID,
		"request_id": r.RequestID, "idempotency_key": r.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("Space deletion %s is required", name)
		}
	}
	return nil
}

type DeletionAuthorizationStatus string

const (
	DeletionAuthorized       DeletionAuthorizationStatus = "authorized"
	DeletionBlockedLegalHold DeletionAuthorizationStatus = "blocked_legal_hold"
	DeletionRejected         DeletionAuthorizationStatus = "rejected"
)

type DeletionAuthorizationReceipt struct {
	RequestID string                      `json:"request_id"`
	Status    DeletionAuthorizationStatus `json:"status"`
}

func (r DeletionAuthorizationReceipt) ValidateFor(requestID string) error {
	if strings.TrimSpace(requestID) == "" || r.RequestID != requestID {
		return fmt.Errorf("Space deletion receipt does not match request")
	}
	switch r.Status {
	case DeletionAuthorized, DeletionBlockedLegalHold, DeletionRejected:
		return nil
	default:
		return fmt.Errorf("unknown Space deletion receipt status %q", r.Status)
	}
}
