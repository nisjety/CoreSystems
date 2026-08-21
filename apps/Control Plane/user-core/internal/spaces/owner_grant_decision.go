package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

const (
	ownerGrantDecisionVersion         = "owner-grant-v1"
	ownerGrantDecisionServiceAudience = "application-plane-conversation-core"
	ownerGrantCreatePermission        = "owner-grant:create"
	ownerGrantRevokePermission        = "owner-grant:revoke"
)

// OwnerGrantDecisionRequest is the small, verified-gateway request used to
// mint a short-lived grant-management decision. The browser supplies no
// subject, organization, role, recipient, or privacy information: Control
// resolves every authority fact from its current records.
type OwnerGrantDecisionRequest struct {
	ConversationID string
	ActionID       string
	Operation      string
	GrantID        string
	IdempotencyKey string
	DecisionRef    string
	Nonce          string
}

func (r OwnerGrantDecisionRequest) Validate() error {
	for name, value := range map[string]string{
		"conversation_id": r.ConversationID,
		"action_id":       r.ActionID,
		"operation":       r.Operation,
		"idempotency_key": r.IdempotencyKey,
		"decision_ref":    r.DecisionRef,
		"nonce":           r.Nonce,
	} {
		if strings.TrimSpace(value) == "" || len(strings.TrimSpace(value)) > 200 {
			return fmt.Errorf("owner grant decision %s is invalid", name)
		}
	}
	if strings.TrimSpace(r.ActionID) != "tickets.create" {
		return fmt.Errorf("owner grant action is not approved")
	}
	switch strings.TrimSpace(r.Operation) {
	case "create":
		if strings.TrimSpace(r.GrantID) != "" {
			return fmt.Errorf("owner grant create must not name a grant")
		}
	case "revoke":
		if strings.TrimSpace(r.GrantID) == "" || len(strings.TrimSpace(r.GrantID)) > 200 {
			return fmt.Errorf("owner grant revoke requires an exact grant id")
		}
	default:
		return fmt.Errorf("owner grant operation is invalid")
	}
	return nil
}

// OwnerGrantDecision is a signed, target-bound authority for a human owner to
// create or revoke Conversation Core's own resource grant. It never grants a
// Model effect and it never substitutes Control's Space facts for the
// Application resource authorization that the owner plane persists.
type OwnerGrantDecision struct {
	DecisionRef               string    `json:"decision_ref"`
	OrgID                     string    `json:"org_id"`
	ConversationID            string    `json:"conversation_id"`
	SpaceRef                  string    `json:"space_ref"`
	SubjectID                 string    `json:"subject_id"`
	ServiceAudience           string    `json:"service_audience"`
	ActionID                  string    `json:"action_id"`
	Operation                 string    `json:"operation"`
	GrantID                   string    `json:"grant_id,omitempty"`
	IdempotencyKey            string    `json:"idempotency_key"`
	RecipientAudienceRef      string    `json:"recipient_audience_ref"`
	RecipientAudienceHash     string    `json:"recipient_audience_hash"`
	RecipientAudienceRevision int64     `json:"recipient_audience_revision"`
	PrivacyPolicyRef          string    `json:"privacy_policy_ref"`
	AuthorityRevision         int64     `json:"authority_revision"`
	Permissions               []string  `json:"permissions"`
	Purpose                   string    `json:"purpose"`
	LawfulBasis               string    `json:"lawful_basis"`
	PrivacyClass              string    `json:"privacy_class"`
	ThirdPartyAllowed         bool      `json:"third_party_processing_allowed"`
	RetentionClass            string    `json:"retention_class"`
	Residency                 string    `json:"residency"`
	DeletionScope             string    `json:"deletion_scope"`
	ZeroDataRetention         bool      `json:"zero_data_retention"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
	Nonce                     string    `json:"nonce"`
}

func (d OwnerGrantDecision) Validate() error {
	for name, value := range map[string]string{
		"decision_ref": d.DecisionRef, "org_id": d.OrgID,
		"conversation_id": d.ConversationID, "space_ref": d.SpaceRef,
		"subject_id": d.SubjectID, "service_audience": d.ServiceAudience,
		"action_id": d.ActionID, "operation": d.Operation,
		"idempotency_key":         d.IdempotencyKey,
		"recipient_audience_ref":  d.RecipientAudienceRef,
		"recipient_audience_hash": d.RecipientAudienceHash,
		"privacy_policy_ref":      d.PrivacyPolicyRef, "purpose": d.Purpose,
		"lawful_basis": d.LawfulBasis, "privacy_class": d.PrivacyClass,
		"retention_class": d.RetentionClass, "residency": d.Residency,
		"deletion_scope": d.DeletionScope, "nonce": d.Nonce,
	} {
		if strings.TrimSpace(value) == "" || len(strings.TrimSpace(value)) > 200 {
			return fmt.Errorf("owner grant decision %s is invalid", name)
		}
	}
	if d.ServiceAudience != ownerGrantDecisionServiceAudience || d.ActionID != "tickets.create" ||
		d.RecipientAudienceRevision <= 0 || d.AuthorityRevision <= 0 ||
		d.ZeroDataRetention || d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() || !d.ExpiresAt.After(d.IssuedAt) ||
		d.ExpiresAt.Sub(d.IssuedAt) > personalDecisionLifetime {
		return fmt.Errorf("owner grant decision authority is invalid")
	}
	switch d.Operation {
	case "create":
		if d.GrantID != "" || len(d.Permissions) != 1 || d.Permissions[0] != ownerGrantCreatePermission {
			return fmt.Errorf("owner grant create authority is invalid")
		}
	case "revoke":
		if strings.TrimSpace(d.GrantID) == "" || len(d.GrantID) > 200 || len(d.Permissions) != 1 || d.Permissions[0] != ownerGrantRevokePermission {
			return fmt.Errorf("owner grant revoke authority is invalid")
		}
	default:
		return fmt.Errorf("owner grant operation is invalid")
	}
	return nil
}

// IssueOwnerGrantDecision accepts only a current personal-space owner or
// manager with the separate durable-agent-action entitlement. Shared spaces
// need their own recipient and conversation-resource policy and are therefore
// deliberately not widened by this initial lifecycle slice.
func IssueOwnerGrantDecision(evidence PersonalThreadDecisionEvidence, request OwnerGrantDecisionRequest, now time.Time) (OwnerGrantDecision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return OwnerGrantDecision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "manager", "owner") || !evidence.AgentActionEntitled || evidence.Privacy.ZeroDataRetention {
		return OwnerGrantDecision{}, fmt.Errorf("current personal Space authority cannot manage agent grants")
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return OwnerGrantDecision{}, fmt.Errorf("owner grant decision request is invalid")
	}
	permission := ownerGrantCreatePermission
	if request.Operation == "revoke" {
		permission = ownerGrantRevokePermission
	}
	return OwnerGrantDecision{
		DecisionRef:               strings.TrimSpace(request.DecisionRef),
		OrgID:                     evidence.Membership.OrgID,
		ConversationID:            strings.TrimSpace(request.ConversationID),
		SpaceRef:                  evidence.Membership.SpaceRef,
		SubjectID:                 evidence.Membership.SubjectID,
		ServiceAudience:           ownerGrantDecisionServiceAudience,
		ActionID:                  "tickets.create",
		Operation:                 strings.TrimSpace(request.Operation),
		GrantID:                   strings.TrimSpace(request.GrantID),
		IdempotencyKey:            strings.TrimSpace(request.IdempotencyKey),
		RecipientAudienceRef:      evidence.RecipientAudienceRef,
		RecipientAudienceHash:     evidence.RecipientAudienceHash,
		RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		PrivacyPolicyRef:          evidence.Privacy.PolicyRef,
		AuthorityRevision:         evidence.Membership.Revisions.Authority,
		Permissions:               []string{permission},
		Purpose:                   evidence.Privacy.Purpose,
		LawfulBasis:               evidence.Privacy.LawfulBasis,
		PrivacyClass:              evidence.Privacy.PrivacyClass,
		ThirdPartyAllowed:         evidence.Privacy.ThirdPartyAllowed,
		RetentionClass:            evidence.Privacy.RetentionClass,
		Residency:                 evidence.Privacy.Residency,
		DeletionScope:             evidence.Privacy.DeletionScope,
		ZeroDataRetention:         false,
		IssuedAt:                  now.UTC(),
		ExpiresAt:                 now.UTC().Add(personalDecisionLifetime),
		Nonce:                     strings.TrimSpace(request.Nonce),
	}, nil
}

// SignOwnerGrantDecision uses a separate envelope from target-effect and
// thread decisions. A token for grant management must never be accepted as a
// ticket-effect bearer.
func SignOwnerGrantDecision(key SigningKey, decision OwnerGrantDecision) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid owner grant signing key is required")
	}
	if err := decision.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		return "", fmt.Errorf("marshal owner grant decision: %w", err)
	}
	signed := ownerGrantDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed))), nil
}
