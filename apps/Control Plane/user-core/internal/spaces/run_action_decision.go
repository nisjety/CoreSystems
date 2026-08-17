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

const (
	runActionDecisionVersion      = "run-action-v1"
	ticketsCreateServiceAudience  = "application-plane-conversation-core"
	ticketsCreateActionPermission = "owner-action:execute"
)

// RunActionAuthority is the deliberately content-free projection Control gets
// from Session Core. The run's context authorization proves where the request
// originated; it is not and cannot become authorization for a target owner
// resource such as a conversation or ticket.
type RunActionAuthority struct {
	RunID                      string
	OrgID                      string
	SubjectID                  string
	ThreadID                   string
	SpaceRef                   string
	RecipientAudienceRef       string
	RecipientAudienceRevision  int64
	RecipientAudienceHash      string
	PrivacyPolicyRef           string
	RunContextAuthorizationRef string
	AuthorityRevision          int64
	RunStatus                  string
}

func (a RunActionAuthority) Validate() error {
	for name, value := range map[string]string{
		"run_id": a.RunID, "org_id": a.OrgID, "subject_id": a.SubjectID,
		"thread_id": a.ThreadID, "space_ref": a.SpaceRef,
		"recipient_audience_ref":        a.RecipientAudienceRef,
		"recipient_audience_hash":       a.RecipientAudienceHash,
		"privacy_policy_ref":            a.PrivacyPolicyRef,
		"run_context_authorization_ref": a.RunContextAuthorizationRef,
		"run_status":                    a.RunStatus,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("run action authority %s is required", name)
		}
	}
	if a.RecipientAudienceRevision <= 0 || a.AuthorityRevision <= 0 {
		return fmt.Errorf("run action authority revisions must be positive")
	}
	switch strings.TrimSpace(a.RunStatus) {
	case "completed", "failed", "cancelled":
		return fmt.Errorf("terminal run cannot request an owner action")
	}
	return nil
}

// RunActionDecisionRequest is the non-secret, owner-action binding presented
// by the Model execution lane. Actor, organization, Space, audience, privacy,
// and source context are all resolved elsewhere and never accepted here.
type RunActionDecisionRequest struct {
	ActionID         string
	ActionSchemaHash string
	PayloadDigest    string
	IdempotencyKey   string
	DecisionRef      string
	Nonce            string
}

func (r RunActionDecisionRequest) Validate() error {
	for name, value := range map[string]string{
		"action_id": r.ActionID, "action_schema_hash": r.ActionSchemaHash,
		"payload_digest": r.PayloadDigest, "idempotency_key": r.IdempotencyKey,
		"decision_ref": r.DecisionRef, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("run action decision %s is required", name)
		}
	}
	if !validSHA256Commitment(r.ActionSchemaHash) || !validSHA256Commitment(r.PayloadDigest) {
		return fmt.Errorf("run action decision commitments are invalid")
	}
	// The signed decision is forwarded through more than one plane. Keep every
	// caller-supplied identifier within the owner operation's bounded contract
	// before serializing/signing it; an oversized retry key must not become a
	// cross-plane memory or header-amplification vector.
	if len(strings.TrimSpace(r.ActionID)) > 128 || len(strings.TrimSpace(r.IdempotencyKey)) > 200 ||
		len(strings.TrimSpace(r.DecisionRef)) > 200 || len(strings.TrimSpace(r.Nonce)) > 200 {
		return fmt.Errorf("run action decision binding is oversized")
	}
	if _, found := runActionTarget(strings.TrimSpace(r.ActionID)); !found {
		return fmt.Errorf("run action is not owner-approved")
	}
	return nil
}

// RunActionDecision is a signed, target-bound permission to attempt exactly
// one owner-plane action. It intentionally has no target resource grant: the
// owner resolves and enforces its own conversation/ticket ACL immediately
// before the durable effect.
type RunActionDecision struct {
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

func (d RunActionDecision) Validate() error {
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
			return fmt.Errorf("run action decision %s is required", name)
		}
	}
	if !validSHA256Commitment(d.ActionSchemaHash) || !validSHA256Commitment(d.PayloadDigest) {
		return fmt.Errorf("run action decision commitments are invalid")
	}
	if audience, found := runActionTarget(d.ActionID); !found || d.ServiceAudience != audience {
		return fmt.Errorf("run action decision target is invalid")
	}
	if d.RecipientAudienceRevision <= 0 || d.AuthorityRevision <= 0 || len(d.Permissions) != 1 || d.Permissions[0] != ticketsCreateActionPermission {
		return fmt.Errorf("run action decision authority is invalid")
	}
	if d.ZeroDataRetention || d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() || !d.ExpiresAt.After(d.IssuedAt) {
		return fmt.Errorf("run action decision retention or expiry is invalid")
	}
	return nil
}

// IssueRunActionDecision requires Control's freshly resolved Space evidence to
// match Session Core's immutable run projection exactly. Session Core's source
// authorization reference is carried only as provenance; it is never named or
// treated as a target owner resource authorization.
func IssueRunActionDecision(evidence PersonalThreadDecisionEvidence, authority RunActionAuthority, request RunActionDecisionRequest, now time.Time) (RunActionDecision, error) {
	if err := evidence.ValidateForAgentAction(); err != nil {
		return RunActionDecision{}, err
	}
	if err := authority.Validate(); err != nil {
		return RunActionDecision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return RunActionDecision{}, fmt.Errorf("run action decision request is invalid")
	}
	// The run is bound to the thread's original creation authorization. This is
	// proof of the run's source context only; the agent-action entitlement above
	// is current Control policy and neither reference authorizes a target owner
	// resource.
	expectedSourceRef := fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if authority.OrgID != evidence.Membership.OrgID || authority.SubjectID != evidence.Membership.SubjectID ||
		authority.SpaceRef != evidence.Membership.SpaceRef || authority.RecipientAudienceRef != evidence.RecipientAudienceRef ||
		authority.RecipientAudienceRevision != evidence.Membership.Revisions.RecipientAudience ||
		authority.RecipientAudienceHash != evidence.RecipientAudienceHash ||
		authority.PrivacyPolicyRef != evidence.Privacy.PolicyRef ||
		authority.AuthorityRevision != evidence.Membership.Revisions.Authority ||
		authority.RunContextAuthorizationRef != expectedSourceRef ||
		evidence.ResourceAuthorizationRef != expectedSourceRef {
		return RunActionDecision{}, fmt.Errorf("run action authority does not match current Control authority")
	}
	audience, _ := runActionTarget(request.ActionID)
	return RunActionDecision{
		DecisionRef:                strings.TrimSpace(request.DecisionRef),
		RunID:                      strings.TrimSpace(authority.RunID),
		ThreadID:                   strings.TrimSpace(authority.ThreadID),
		OrgID:                      evidence.Membership.OrgID,
		SpaceRef:                   evidence.Membership.SpaceRef,
		SubjectID:                  evidence.Membership.SubjectID,
		ServiceAudience:            audience,
		ActionID:                   strings.TrimSpace(request.ActionID),
		ActionSchemaHash:           strings.TrimSpace(request.ActionSchemaHash),
		PayloadDigest:              strings.TrimSpace(request.PayloadDigest),
		IdempotencyKey:             strings.TrimSpace(request.IdempotencyKey),
		RecipientAudienceRef:       evidence.RecipientAudienceRef,
		RecipientAudienceHash:      evidence.RecipientAudienceHash,
		RecipientAudienceRevision:  evidence.Membership.Revisions.RecipientAudience,
		PrivacyPolicyRef:           evidence.Privacy.PolicyRef,
		RunContextAuthorizationRef: expectedSourceRef,
		AuthorityRevision:          evidence.Membership.Revisions.Authority,
		Permissions:                []string{ticketsCreateActionPermission},
		Purpose:                    evidence.Privacy.Purpose,
		LawfulBasis:                evidence.Privacy.LawfulBasis,
		PrivacyClass:               evidence.Privacy.PrivacyClass,
		ThirdPartyAllowed:          evidence.Privacy.ThirdPartyAllowed,
		RetentionClass:             evidence.Privacy.RetentionClass,
		Residency:                  evidence.Privacy.Residency,
		DeletionScope:              evidence.Privacy.DeletionScope,
		ZeroDataRetention:          false,
		IssuedAt:                   now.UTC(),
		ExpiresAt:                  now.UTC().Add(personalDecisionLifetime),
		Nonce:                      strings.TrimSpace(request.Nonce),
	}, nil
}

// ValidateCurrentRunActionDecision performs Control's effect-time freshness
// check for a decision that was already signature-verified by the owner
// plane. It intentionally receives only the decision's non-secret claims: the
// signed bearer remains local to the execution-to-owner hop. Any effective
// membership, audience, privacy, or entitlement change advances authority and
// must make the old claim set ineligible before the owner commits its effect.
func ValidateCurrentRunActionDecision(evidence PersonalThreadDecisionEvidence, decision RunActionDecision) error {
	if err := evidence.ValidateForAgentAction(); err != nil {
		return fmt.Errorf("current agent action authority is ineligible: %w", err)
	}
	if err := decision.Validate(); err != nil {
		return fmt.Errorf("run action decision is invalid: %w", err)
	}
	expectedSourceRef := fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if decision.OrgID != evidence.Membership.OrgID ||
		decision.SpaceRef != evidence.Membership.SpaceRef ||
		decision.SubjectID != evidence.Membership.SubjectID ||
		decision.ServiceAudience != ticketsCreateServiceAudience ||
		decision.ActionID != "tickets.create" ||
		decision.RecipientAudienceRef != evidence.RecipientAudienceRef ||
		decision.RecipientAudienceHash != evidence.RecipientAudienceHash ||
		decision.RecipientAudienceRevision != evidence.Membership.Revisions.RecipientAudience ||
		decision.PrivacyPolicyRef != evidence.Privacy.PolicyRef ||
		decision.AuthorityRevision != evidence.Membership.Revisions.Authority ||
		decision.RunContextAuthorizationRef != expectedSourceRef ||
		decision.Purpose != evidence.Privacy.Purpose ||
		decision.LawfulBasis != evidence.Privacy.LawfulBasis ||
		decision.PrivacyClass != evidence.Privacy.PrivacyClass ||
		decision.ThirdPartyAllowed != evidence.Privacy.ThirdPartyAllowed ||
		decision.RetentionClass != evidence.Privacy.RetentionClass ||
		decision.Residency != evidence.Privacy.Residency ||
		decision.DeletionScope != evidence.Privacy.DeletionScope ||
		decision.ZeroDataRetention != evidence.Privacy.ZeroDataRetention {
		return fmt.Errorf("run action decision does not match current Control authority")
	}
	return nil
}

// SignRunActionDecision uses a different envelope version from a Space thread
// decision, so neither recipient can accidentally accept the other contract.
func SignRunActionDecision(key SigningKey, decision RunActionDecision) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid run action signing key is required")
	}
	if err := decision.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		return "", fmt.Errorf("marshal run action decision: %w", err)
	}
	signed := runActionDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed))), nil
}

// VerifyRunActionDecision verifies a decision Control previously issued before
// accepting it back on the private owner-effect reservation hop. The envelope
// is not persisted; its immutable facts are copied into the reservation ledger
// only after this verification and a fresh authority-fence check succeed.
func VerifyRunActionDecision(key SigningKey, token string, now time.Time) (RunActionDecision, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize || now.IsZero() {
		return RunActionDecision{}, fmt.Errorf("valid run action verification key is required")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != runActionDecisionVersion {
		return RunActionDecision{}, fmt.Errorf("run action decision envelope is invalid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != key.ID {
		return RunActionDecision{}, fmt.Errorf("run action decision key is invalid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return RunActionDecision{}, fmt.Errorf("run action decision payload is invalid")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	publicKey, ok := key.PrivateKey.Public().(ed25519.PublicKey)
	if err != nil || !ok || len(signature) != ed25519.SignatureSize || !ed25519.Verify(publicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		return RunActionDecision{}, fmt.Errorf("run action decision signature is invalid")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var decision RunActionDecision
	if err := decoder.Decode(&decision); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return RunActionDecision{}, fmt.Errorf("run action decision claims are invalid")
	}
	if err := decision.Validate(); err != nil || decision.ExpiresAt.Before(now) || decision.IssuedAt.After(now.Add(time.Minute)) {
		return RunActionDecision{}, fmt.Errorf("run action decision is invalid or expired")
	}
	return decision, nil
}

func runActionTarget(actionID string) (string, bool) {
	switch strings.TrimSpace(actionID) {
	case "tickets.create":
		return ticketsCreateServiceAudience, true
	default:
		return "", false
	}
}

func validSHA256Commitment(value string) bool {
	value = strings.TrimSpace(value)
	if !strings.HasPrefix(value, "sha256:") || len(value) != len("sha256:")+64 {
		return false
	}
	for _, r := range value[len("sha256:"):] {
		if !(r >= '0' && r <= '9') && !(r >= 'a' && r <= 'f') {
			return false
		}
	}
	return true
}
