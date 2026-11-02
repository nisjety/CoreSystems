package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
)

// WS10-S2: the signed proof that a standing rule covered one call. Session Core
// verifies it before it materialises an already-granted approval row (S3), so a
// forged or replayed claim cannot pass a prompt.
//
// It is a different envelope version from run-action-v1 and from a Space thread
// decision, so no recipient can accept one for the other. It carries no content:
// ids, keys, the schema hash and a short expiry.
const (
	approvalGrantUseVersion  = "approval-grant-use-v1"
	approvalGrantUseAudience = "model-plane"
	// ApprovalGrantUseLifetime is the token's whole life: it passes one prompt
	// and is spent in the gate call that asked for it.
	ApprovalGrantUseLifetime = 2 * time.Minute
)

// ApprovalGrantUse is the claim set. SubjectID is the run's requester: the
// approved action still runs as them.
type ApprovalGrantUse struct {
	TokenRef          string    `json:"token_ref"`
	RuleID            string    `json:"rule_id"`
	Scope             string    `json:"scope"`
	RunID             string    `json:"run_id"`
	OrgID             string    `json:"org_id"`
	SpaceRef          string    `json:"space_ref"`
	SubjectID         string    `json:"subject_id"`
	AgentSubjectID    string    `json:"agent_subject_id"`
	ToolKey           string    `json:"tool_key"`
	ActionKey         string    `json:"action_key"`
	ActionSchemaHash  string    `json:"action_schema_hash"`
	AuthorityRevision int64     `json:"authority_revision"`
	Audience          string    `json:"audience"`
	IssuedAt          time.Time `json:"issued_at"`
	ExpiresAt         time.Time `json:"expires_at"`
	Nonce             string    `json:"nonce"`
}

// Validate checks every claim is present and shaped.
func (u ApprovalGrantUse) Validate() error {
	for name, value := range map[string]string{
		"token_ref": u.TokenRef, "rule_id": u.RuleID, "scope": u.Scope, "run_id": u.RunID,
		"org_id": u.OrgID, "space_ref": u.SpaceRef, "subject_id": u.SubjectID,
		"agent_subject_id": u.AgentSubjectID, "tool_key": u.ToolKey, "action_key": u.ActionKey,
		"action_schema_hash": u.ActionSchemaHash, "audience": u.Audience, "nonce": u.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("approval grant use %s is required", name)
		}
	}
	if u.Scope != string(ApprovalScopeAlways) || u.Audience != approvalGrantUseAudience {
		return fmt.Errorf("approval grant use scope or audience is invalid")
	}
	if !validSHA256Commitment(u.ActionSchemaHash) || u.AuthorityRevision <= 0 {
		return fmt.Errorf("approval grant use commitments are invalid")
	}
	if u.IssuedAt.IsZero() || u.ExpiresAt.IsZero() || !u.ExpiresAt.After(u.IssuedAt) ||
		u.ExpiresAt.Sub(u.IssuedAt) > ApprovalGrantUseLifetime {
		return fmt.Errorf("approval grant use expiry is invalid")
	}
	return nil
}

// NewApprovalGrantUse builds the claims for one used rule. evidence is the
// requester's freshly resolved authority.
func NewApprovalGrantUse(evidence PersonalThreadDecisionEvidence, rule ApprovalRule, runID, tokenRef, nonce string, now time.Time) (ApprovalGrantUse, error) {
	if now.IsZero() {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use issuance time is required")
	}
	use := ApprovalGrantUse{
		TokenRef: strings.TrimSpace(tokenRef), RuleID: rule.RuleID, Scope: rule.Scope,
		RunID: strings.TrimSpace(runID), OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef,
		SubjectID: evidence.Membership.SubjectID, AgentSubjectID: rule.AgentSubjectID,
		ToolKey: rule.ToolKey, ActionKey: rule.ActionKey, ActionSchemaHash: rule.ActionSchemaHash,
		AuthorityRevision: evidence.Membership.Revisions.Authority, Audience: approvalGrantUseAudience,
		IssuedAt: now.UTC(), ExpiresAt: now.UTC().Add(ApprovalGrantUseLifetime), Nonce: strings.TrimSpace(nonce),
	}
	if err := use.Validate(); err != nil {
		return ApprovalGrantUse{}, err
	}
	return use, nil
}

// SignApprovalGrantUse signs the claims under the Space decision key with its
// own envelope version.
func SignApprovalGrantUse(key SigningKey, use ApprovalGrantUse) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid approval grant signing key is required")
	}
	if err := use.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(use)
	if err != nil {
		return "", fmt.Errorf("marshal approval grant use: %w", err)
	}
	signed := approvalGrantUseVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed))), nil
}

// VerifyApprovalGrantUse verifies a token Control issued. Session Core has its
// own verifier; this one backs the parity vectors and Control-side tests.
func VerifyApprovalGrantUse(key SigningKey, token string, now time.Time) (ApprovalGrantUse, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize || now.IsZero() {
		return ApprovalGrantUse{}, fmt.Errorf("valid approval grant verification key is required")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != approvalGrantUseVersion {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use envelope is invalid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != key.ID {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use key is invalid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use payload is invalid")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	publicKey, ok := key.PrivateKey.Public().(ed25519.PublicKey)
	if err != nil || !ok || len(signature) != ed25519.SignatureSize || !ed25519.Verify(publicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use signature is invalid")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var use ApprovalGrantUse
	if err := decoder.Decode(&use); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use claims are invalid")
	}
	if err := use.Validate(); err != nil || use.ExpiresAt.Before(now) || use.IssuedAt.After(now.Add(time.Minute)) {
		return ApprovalGrantUse{}, fmt.Errorf("approval grant use is invalid or expired")
	}
	return use, nil
}
