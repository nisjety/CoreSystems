package server

// Per-call capability decision evidence.
//
// The response correlation id is useful for audit lookup but is not an
// authorization artifact. This module adds a short-lived Ed25519 JWS whose
// payload binds the exact policy tuple and decision. Execution runtimes verify
// the proof with a deployment-distributed public key before dispatch.

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"
)

const decisionProofType = "model-plane.capability-decision+jws"

// decisionProofVersion is the claim set Sign writes. Version 2 binds the tool,
// the user, the declared class and approval floor, the policy source and the
// pinned tool digest to the decision, so evidence for one tool cannot be
// replayed against another. capability-core always signs it, whatever the
// rollout switch says; execution-core's verifier accepts "1" and "2" while its
// TOOL_CLASS_GATE is off or shadow and only a build that predates the gate
// refuses "2". So the safe order is execution-core first (gate off), then
// capability-core: rolling capability-core first would have every tool call
// refused by an execution-core that does not yet know the version.
const decisionProofVersion = "2"

type decisionProofHeader struct {
	Alg string `json:"alg"`
	Kid string `json:"kid"`
	Typ string `json:"typ"`
}

type decisionProofClaims struct {
	Version           string `json:"v"`
	IssuedAt          int64  `json:"iat"`
	ExpiresAt         int64  `json:"exp"`
	DecisionID        string `json:"decision_id"`
	CapabilityID      string `json:"capability_id"`
	CapabilityVersion string `json:"capability_version"`
	OrgID             string `json:"org_id"`
	RunID             string `json:"run_id"`
	AgentID           string `json:"agent_id"`
	Scope             string `json:"scope"`
	Decision          string `json:"decision"`
	Reason            string `json:"reason"`
	BudgetContext     string `json:"budget_context"`
	// Version 2 claims. They are always written, empty when the evaluation
	// named no tool or asserted no user, so the signed bytes say what was and
	// was not bound.
	UserID          string `json:"user_id"`
	ToolName        string `json:"tool_name"`
	SideEffectClass string `json:"side_effect_class"`
	ApprovalFloor   string `json:"approval_floor"`
	PolicySource    string `json:"policy_source"`
	ToolDigest      string `json:"tool_digest"`
	// Present only when the policy store was successfully read for this call.
	ToolPolicyChecked bool `json:"tool_policy_checked,omitempty"`
}

type DecisionProofSigner struct {
	key ed25519.PrivateKey
	kid string
	ttl time.Duration
}

func NewDecisionProofSignerFromEnv() (*DecisionProofSigner, error) {
	raw := strings.TrimSpace(os.Getenv("CAPABILITY_CORE_DECISION_SIGNING_KEY"))
	if raw == "" {
		return nil, errors.New("CAPABILITY_CORE_DECISION_SIGNING_KEY is required")
	}
	keyBytes, err := base64.StdEncoding.DecodeString(raw)
	if err != nil {
		return nil, fmt.Errorf("decode capability decision signing key: %w", err)
	}
	if len(keyBytes) == ed25519.SeedSize {
		key := ed25519.NewKeyFromSeed(keyBytes)
		return newDecisionProofSigner(key, os.Getenv("CAPABILITY_CORE_DECISION_SIGNING_KID"))
	}
	if len(keyBytes) == ed25519.PrivateKeySize {
		return newDecisionProofSigner(ed25519.PrivateKey(keyBytes), os.Getenv("CAPABILITY_CORE_DECISION_SIGNING_KID"))
	}
	return nil, fmt.Errorf("capability decision signing key must be %d-byte seed or %d-byte private key", ed25519.SeedSize, ed25519.PrivateKeySize)
}

func NewDecisionProofSignerForTest() *DecisionProofSigner {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		panic(err)
	}
	_ = public
	signer, err := newDecisionProofSigner(private, "test-capability-decision-v1")
	if err != nil {
		panic(err)
	}
	return signer
}

func newDecisionProofSigner(key ed25519.PrivateKey, kid string) (*DecisionProofSigner, error) {
	kid = strings.TrimSpace(kid)
	if kid == "" {
		kid = "capability-decision-v1"
	}
	return &DecisionProofSigner{key: key, kid: kid, ttl: 60 * time.Second}, nil
}

func (s *DecisionProofSigner) Sign(claims decisionProofClaims) (string, error) {
	if s == nil || len(s.key) != ed25519.PrivateKeySize {
		return "", errors.New("capability decision proof signer is unavailable")
	}
	now := time.Now().Unix()
	claims.Version = decisionProofVersion
	claims.IssuedAt = now
	claims.ExpiresAt = now + int64(s.ttl/time.Second)
	header, err := json.Marshal(decisionProofHeader{Alg: "EdDSA", Kid: s.kid, Typ: decisionProofType})
	if err != nil {
		return "", err
	}
	payload, err := json.Marshal(claims)
	if err != nil {
		return "", err
	}
	encode := base64.RawURLEncoding.EncodeToString
	protected := encode(header) + "." + encode(payload)
	signature := ed25519.Sign(s.key, []byte(protected))
	return protected + "." + encode(signature), nil
}

func (s *DecisionProofSigner) PublicKey() ed25519.PublicKey {
	if s == nil || len(s.key) != ed25519.PrivateKeySize {
		return nil
	}
	public := make(ed25519.PublicKey, ed25519.PublicKeySize)
	copy(public, s.key[ed25519.SeedSize:])
	return public
}

func decisionProofClaimsFor(reqFields decisionProofRequest, response *decisionProofResponse) decisionProofClaims {
	return decisionProofClaims{
		DecisionID:        response.decisionID,
		CapabilityID:      reqFields.capabilityID,
		CapabilityVersion: response.capabilityVersion,
		OrgID:             reqFields.orgID,
		RunID:             reqFields.runID,
		AgentID:           reqFields.agentID,
		Scope:             reqFields.scope,
		Decision:          response.decision,
		Reason:            response.reason,
		BudgetContext:     response.budgetContext,
		UserID:            reqFields.userID,
		ToolName:          reqFields.toolName,
		SideEffectClass:   response.sideEffectClass,
		ApprovalFloor:     response.approvalFloor,
		PolicySource:      response.policySource,
		ToolDigest:        response.toolDigest,
		ToolPolicyChecked: response.toolPolicyChecked,
	}
}

type decisionProofRequest struct {
	capabilityID string
	runID        string
	agentID      string
	orgID        string
	scope        string
	userID       string
	toolName     string
}

type decisionProofResponse struct {
	decision          string
	reason            string
	budgetContext     string
	decisionID        string
	capabilityVersion string
	sideEffectClass   string
	approvalFloor     string
	policySource      string
	toolDigest        string
	toolPolicyChecked bool
}
