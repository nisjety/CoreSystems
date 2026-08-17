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
	runActionDecisionVersion         = "run-action-v1"
	runActionDecisionServiceAudience = "application-plane-conversation-core"
	runActionDecisionPermission      = "owner-action:execute"
	runActionDecisionLifetime        = 2 * time.Minute
)

// runActionDecision is Control's target-bound, short-lived owner-action
// decision. The source run context reference is provenance only: it is never
// interpreted here as authorization for a conversation or ticket.
type runActionDecision struct {
	DecisionRef                string    `json:"decision_ref"`
	RunID                      string    `json:"run_id"`
	ThreadID                   string    `json:"thread_id"`
	OrgID                      string    `json:"org_id"`
	SpaceRef                   string    `json:"space_ref"`
	SubjectID                  string    `json:"subject_id"`
	ServiceAudience            string    `json:"service_audience"`
	ActionID                   string    `json:"action_id"`
	ActionSchemaHash           string    `json:"action_schema_hash"`
	PayloadDigest              string    `json:"payload_digest"`
	IdempotencyKey             string    `json:"idempotency_key"`
	RecipientAudienceRef       string    `json:"recipient_audience_ref"`
	RecipientAudienceHash      string    `json:"recipient_audience_hash"`
	RecipientAudienceRevision  int64     `json:"recipient_audience_revision"`
	PrivacyPolicyRef           string    `json:"privacy_policy_ref"`
	RunContextAuthorizationRef string    `json:"run_context_authorization_ref"`
	AuthorityRevision          int64     `json:"authority_revision"`
	Permissions                []string  `json:"permissions"`
	Purpose                    string    `json:"purpose"`
	LawfulBasis                string    `json:"lawful_basis"`
	PrivacyClass               string    `json:"privacy_class"`
	ThirdPartyAllowed          bool      `json:"third_party_processing_allowed"`
	RetentionClass             string    `json:"retention_class"`
	Residency                  string    `json:"residency"`
	DeletionScope              string    `json:"deletion_scope"`
	ZeroDataRetention          bool      `json:"zero_data_retention"`
	IssuedAt                   time.Time `json:"issued_at"`
	ExpiresAt                  time.Time `json:"expires_at"`
	Nonce                      string    `json:"nonce"`
}

// RunActionDecisionVerifier verifies a Control-issued, target-bound decision
// with exactly one configured public key. Empty or invalid configuration is
// deliberately represented by a nil verifier at the HTTP boundary, which
// keeps the private owner route fail-closed in dev until it is wired.
type RunActionDecisionVerifier struct {
	keyID     string
	publicKey ed25519.PublicKey
	now       func() time.Time
}

func NewRunActionDecisionVerifier(keyID, publicKeyEncoded string) (*RunActionDecisionVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	if keyID == "" {
		return nil, fmt.Errorf("Control run action decision key id is required")
	}
	encoded := strings.TrimSpace(publicKeyEncoded)
	publicKeyBytes, err := base64.RawStdEncoding.DecodeString(encoded)
	if err != nil {
		publicKeyBytes, err = base64.StdEncoding.DecodeString(encoded)
	}
	if err != nil || len(publicKeyBytes) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control run action decision public key is invalid")
	}
	return &RunActionDecisionVerifier{
		keyID:     keyID,
		publicKey: ed25519.PublicKey(publicKeyBytes),
		now:       func() time.Time { return time.Now().UTC() },
	}, nil
}

func (v *RunActionDecisionVerifier) Verify(token string) (runActionDecision, error) {
	if v == nil || len(v.publicKey) != ed25519.PublicKeySize || strings.TrimSpace(v.keyID) == "" {
		return runActionDecision{}, fmt.Errorf("Control run action decision verifier is unavailable")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != runActionDecisionVersion {
		return runActionDecision{}, fmt.Errorf("Control run action decision envelope is invalid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != v.keyID {
		return runActionDecision{}, fmt.Errorf("Control run action decision key is invalid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return runActionDecision{}, fmt.Errorf("Control run action decision payload is invalid")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize || !ed25519.Verify(v.publicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		return runActionDecision{}, fmt.Errorf("Control run action decision signature is invalid")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var decision runActionDecision
	if err := decoder.Decode(&decision); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return runActionDecision{}, fmt.Errorf("Control run action decision claims are invalid")
	}
	if err := decision.Validate(v.now()); err != nil {
		return runActionDecision{}, err
	}
	return decision, nil
}

func (d runActionDecision) Validate(now time.Time) error {
	for name, value := range map[string]string{
		"decision_ref": d.DecisionRef, "run_id": d.RunID, "thread_id": d.ThreadID,
		"org_id": d.OrgID, "space_ref": d.SpaceRef, "subject_id": d.SubjectID,
		"service_audience": d.ServiceAudience, "action_id": d.ActionID,
		"action_schema_hash": d.ActionSchemaHash, "payload_digest": d.PayloadDigest,
		"idempotency_key": d.IdempotencyKey, "recipient_audience_ref": d.RecipientAudienceRef,
		"recipient_audience_hash": d.RecipientAudienceHash, "privacy_policy_ref": d.PrivacyPolicyRef,
		"run_context_authorization_ref": d.RunContextAuthorizationRef,
		"purpose":                       d.Purpose, "lawful_basis": d.LawfulBasis, "privacy_class": d.PrivacyClass,
		"retention_class": d.RetentionClass, "residency": d.Residency,
		"deletion_scope": d.DeletionScope, "nonce": d.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("Control run action decision %s is required", name)
		}
	}
	if d.ServiceAudience != runActionDecisionServiceAudience || d.ActionID != "tickets.create" ||
		!validRunActionCommitment(d.ActionSchemaHash) || !validRunActionCommitment(d.PayloadDigest) ||
		d.RecipientAudienceRevision <= 0 || d.AuthorityRevision <= 0 ||
		len(d.Permissions) != 1 || d.Permissions[0] != runActionDecisionPermission ||
		d.ZeroDataRetention || d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() ||
		!d.ExpiresAt.After(d.IssuedAt) || d.ExpiresAt.Sub(d.IssuedAt) > runActionDecisionLifetime ||
		d.ExpiresAt.Before(now) || d.IssuedAt.After(now.Add(time.Minute)) {
		return fmt.Errorf("Control run action decision authority is invalid or expired")
	}
	return nil
}

func validRunActionCommitment(value string) bool {
	value = strings.TrimSpace(value)
	if !strings.HasPrefix(value, "sha256:") || len(value) != len("sha256:")+64 {
		return false
	}
	_, err := hex.DecodeString(value[len("sha256:"):])
	return err == nil
}

// agentTicketCreateBody is intentionally narrower than the human ticket body.
// The workload cannot set assignment, timestamps, labels, source, confidence,
// SLA, or any other owner-controlled operational field.
type agentTicketCreateBody struct {
	RunID                string `json:"run_id"`
	ControlDecisionToken string `json:"control_decision_token"`
	IdempotencyKey       string `json:"idempotency_key"`
	// Continuation-only binding. Immediate model calls omit it; a resumed
	// approval must carry the frozen owner and the signed Control subject must
	// match it before any owner reservation or transaction begins.
	OwnerUserID    string `json:"owner_user_id,omitempty"`
	ConversationID string `json:"conversation_id"`
	WorkType       string `json:"work_type"`
	Priority       string `json:"priority"`
	Severity       string `json:"severity"`
	Category       string `json:"category"`
	Intent         string `json:"intent"`
}

// agentTicketOperationReconcileBody is used only after execution-core cannot
// determine whether a ticket write response was delivered. It carries the
// original signed decision and immutable commitments, but no mutable ticket
// input; reconciliation is a read and never re-runs the effect.
type agentTicketOperationReconcileBody struct {
	RunID                string `json:"run_id"`
	OrgID                string `json:"org_id"`
	ControlDecisionToken string `json:"control_decision_token"`
	ActionSchemaHash     string `json:"action_schema_hash"`
	PayloadDigest        string `json:"payload_digest"`
	IdempotencyKey       string `json:"idempotency_key"`
}

func agentTicketPayloadDigest(runID, orgID string, body agentTicketCreateBody) string {
	// A struct (rather than a generic map) gives the cross-plane payload a
	// fixed shape. Values are normalized exactly as the owner will consume
	// them; no conversation content is copied into the decision.
	payload, _ := json.Marshal(struct {
		ActionID       string `json:"action_id"`
		RunID          string `json:"run_id"`
		OrgID          string `json:"org_id"`
		IdempotencyKey string `json:"idempotency_key"`
		ConversationID string `json:"conversation_id"`
		WorkType       string `json:"work_type"`
		Priority       string `json:"priority"`
		Severity       string `json:"severity"`
		Category       string `json:"category"`
		Intent         string `json:"intent"`
	}{
		ActionID: "tickets.create", RunID: strings.TrimSpace(runID), OrgID: strings.TrimSpace(orgID),
		IdempotencyKey: strings.TrimSpace(body.IdempotencyKey), ConversationID: strings.TrimSpace(body.ConversationID),
		WorkType: strings.ToLower(strings.TrimSpace(body.WorkType)), Priority: strings.ToLower(strings.TrimSpace(body.Priority)),
		Severity: strings.ToLower(strings.TrimSpace(body.Severity)), Category: strings.TrimSpace(body.Category), Intent: strings.TrimSpace(body.Intent),
	})
	digest := sha256.Sum256(payload)
	return "sha256:" + hex.EncodeToString(digest[:])
}
