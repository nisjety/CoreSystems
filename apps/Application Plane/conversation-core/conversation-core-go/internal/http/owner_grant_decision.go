package http

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
)

const (
	ownerGrantDecisionVersion         = "owner-grant-v1"
	ownerGrantDecisionServiceAudience = "application-plane-conversation-core"
	ownerGrantDecisionLifetime        = 2 * time.Minute
	ownerGrantCreatePermission        = "owner-grant:create"
	ownerGrantRevokePermission        = "owner-grant:revoke"
)

// ownerGrantDecision is a Control-signed management authority for
// Conversation Core's own resource grant. It is deliberately a different
// envelope from run-action-v1, so either bearer is structurally rejected at
// the other's boundary.
type ownerGrantDecision struct {
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

type OwnerGrantDecisionVerifier struct {
	keyID     string
	publicKey ed25519.PublicKey
	now       func() time.Time
}

func NewOwnerGrantDecisionVerifier(keyID, publicKeyEncoded string) (*OwnerGrantDecisionVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	if keyID == "" {
		return nil, fmt.Errorf("Control owner grant decision key id is required")
	}
	encoded := strings.TrimSpace(publicKeyEncoded)
	publicKeyBytes, err := base64.RawStdEncoding.DecodeString(encoded)
	if err != nil {
		publicKeyBytes, err = base64.StdEncoding.DecodeString(encoded)
	}
	if err != nil || len(publicKeyBytes) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control owner grant decision public key is invalid")
	}
	return &OwnerGrantDecisionVerifier{keyID: keyID, publicKey: ed25519.PublicKey(publicKeyBytes), now: func() time.Time { return time.Now().UTC() }}, nil
}

func (v *OwnerGrantDecisionVerifier) Verify(token string) (ownerGrantDecision, error) {
	if v == nil || len(v.publicKey) != ed25519.PublicKeySize || strings.TrimSpace(v.keyID) == "" {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision verifier is unavailable")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != ownerGrantDecisionVersion {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision envelope is invalid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != v.keyID {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision key is invalid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision payload is invalid")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize || !ed25519.Verify(v.publicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision signature is invalid")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var decision ownerGrantDecision
	if err := decoder.Decode(&decision); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return ownerGrantDecision{}, fmt.Errorf("Control owner grant decision claims are invalid")
	}
	if err := decision.Validate(v.now()); err != nil {
		return ownerGrantDecision{}, err
	}
	return decision, nil
}

func (d ownerGrantDecision) Validate(now time.Time) error {
	for name, value := range map[string]string{
		"decision_ref": d.DecisionRef, "org_id": d.OrgID, "conversation_id": d.ConversationID,
		"space_ref": d.SpaceRef, "subject_id": d.SubjectID, "service_audience": d.ServiceAudience,
		"action_id": d.ActionID, "operation": d.Operation, "idempotency_key": d.IdempotencyKey,
		"recipient_audience_ref": d.RecipientAudienceRef, "recipient_audience_hash": d.RecipientAudienceHash,
		"privacy_policy_ref": d.PrivacyPolicyRef, "purpose": d.Purpose, "lawful_basis": d.LawfulBasis,
		"privacy_class": d.PrivacyClass, "retention_class": d.RetentionClass, "residency": d.Residency,
		"deletion_scope": d.DeletionScope, "nonce": d.Nonce,
	} {
		if strings.TrimSpace(value) == "" || len(strings.TrimSpace(value)) > 200 {
			return fmt.Errorf("Control owner grant decision %s is invalid", name)
		}
	}
	if d.ServiceAudience != ownerGrantDecisionServiceAudience || d.ActionID != "tickets.create" ||
		d.RecipientAudienceRevision <= 0 || d.AuthorityRevision <= 0 || d.ZeroDataRetention ||
		d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() || !d.ExpiresAt.After(d.IssuedAt) ||
		d.ExpiresAt.Sub(d.IssuedAt) > ownerGrantDecisionLifetime || d.ExpiresAt.Before(now) || d.IssuedAt.After(now.Add(time.Minute)) {
		return fmt.Errorf("Control owner grant decision authority is invalid or expired")
	}
	switch d.Operation {
	case "create":
		if d.GrantID != "" || len(d.Permissions) != 1 || d.Permissions[0] != ownerGrantCreatePermission {
			return fmt.Errorf("Control owner grant create authority is invalid")
		}
	case "revoke":
		if strings.TrimSpace(d.GrantID) == "" || len(d.GrantID) > 200 || len(d.Permissions) != 1 || d.Permissions[0] != ownerGrantRevokePermission {
			return fmt.Errorf("Control owner grant revoke authority is invalid")
		}
	default:
		return fmt.Errorf("Control owner grant operation is invalid")
	}
	return nil
}

// ownerGrantRequestSHA256 binds a receipt to the exact signed management
// authority without persisting the bearer itself. It is never a conversation
// content digest.
func ownerGrantRequestSHA256(decision ownerGrantDecision) string {
	payload, _ := json.Marshal(struct {
		Version        string `json:"version"`
		DecisionRef    string `json:"decision_ref"`
		OrgID          string `json:"org_id"`
		ConversationID string `json:"conversation_id"`
		ActionID       string `json:"action_id"`
		Operation      string `json:"operation"`
		GrantID        string `json:"grant_id"`
		SpaceRef       string `json:"space_ref"`
		SubjectID      string `json:"subject_id"`
		IdempotencyKey string `json:"idempotency_key"`
	}{
		Version: "owner-grant-request/v1", DecisionRef: decision.DecisionRef, OrgID: decision.OrgID,
		ConversationID: decision.ConversationID, ActionID: decision.ActionID, Operation: decision.Operation,
		GrantID: decision.GrantID, SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID,
		IdempotencyKey: decision.IdempotencyKey,
	})
	digest := sha256.Sum256(payload)
	return "sha256:" + hex.EncodeToString(digest[:])
}
