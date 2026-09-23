package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

const decisionVersion = "v2"

// SigningKey stays solely in Control. Recipient planes receive only the
// corresponding VerificationKey through a deployment-managed key set.
// Key IDs make rotation explicit: a verifier never guesses which public key
// should be trusted for a decision envelope.
type SigningKey struct {
	ID         string
	PrivateKey ed25519.PrivateKey
}

type VerificationKey struct {
	ID        string
	PublicKey ed25519.PublicKey
}

// LoadSigningKeyFromEnv reads the deployment-managed Control signing material.
// It accepts a base64url Ed25519 seed (32 bytes) or private key (64 bytes) but
// never derives a development fallback. Recipient planes receive only the
// matching public key through their own deployment configuration.
func LoadSigningKeyFromEnv(getenv func(string) string) (SigningKey, error) {
	if getenv == nil {
		return SigningKey{}, fmt.Errorf("space decision environment reader is required")
	}
	id := strings.TrimSpace(getenv("CONTROL_SPACE_DECISION_KEY_ID"))
	raw := strings.TrimSpace(getenv("CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64"))
	if id == "" || raw == "" || strings.IndexFunc(raw, func(r rune) bool { return r == ' ' || r == '\t' || r == '\n' || r == '\r' }) >= 0 {
		return SigningKey{}, fmt.Errorf("control Space decision signing key is not configured")
	}
	decoded, err := base64.RawURLEncoding.DecodeString(raw)
	if err != nil {
		return SigningKey{}, fmt.Errorf("decode Control Space decision signing key: %w", err)
	}
	var privateKey ed25519.PrivateKey
	switch len(decoded) {
	case ed25519.SeedSize:
		privateKey = ed25519.NewKeyFromSeed(decoded)
	case ed25519.PrivateKeySize:
		privateKey = decoded
	default:
		return SigningKey{}, fmt.Errorf("control Space decision signing key has invalid length")
	}
	return SigningKey{ID: id, PrivateKey: privateKey}, nil
}

// Decision is the target-service-bound authority assertion emitted by Control.
// It carries only policy/identity references and metadata; content and secrets
// never enter this token.
type Decision struct {
	DecisionRef               string   `json:"decision_ref"`
	OrgID                     string   `json:"org_id"`
	SpaceRef                  string   `json:"space_ref"`
	SubjectID                 string   `json:"subject_id"`
	ServiceAudience           string   `json:"service_audience"`
	ActionID                  string   `json:"action_id"`
	ActionSchemaHash          string   `json:"action_schema_hash"`
	PayloadDigest             string   `json:"payload_digest"`
	IdempotencyKey            string   `json:"idempotency_key"`
	RecipientAudienceRef      string   `json:"recipient_audience_ref"`
	RecipientAudienceHash     string   `json:"recipient_audience_hash"`
	PrivacyPolicyRef          string   `json:"privacy_policy_ref"`
	ResourceAuthorizationRef  string   `json:"resource_authorization_ref"`
	AuthorityRevision         int64    `json:"authority_revision"`
	MembershipRevision        int64    `json:"membership_revision"`
	PrivacyRevision           int64    `json:"privacy_revision"`
	RecipientAudienceRevision int64    `json:"recipient_audience_revision"`
	EntitlementRevision       int64    `json:"entitlement_revision"`
	Permissions               []string `json:"permissions"`
	Purpose                   string   `json:"purpose"`
	LawfulBasis               string   `json:"lawful_basis"`
	PrivacyClass              string   `json:"privacy_class"`
	ThirdPartyAllowed         bool     `json:"third_party_processing_allowed"`
	RetentionClass            string   `json:"retention_class"`
	Residency                 string   `json:"residency"`
	DeletionScope             string   `json:"deletion_scope"`
	// ImportSourceType is present only for the import action family. It binds a
	// durable connector intent without turning Control into a connector owner.
	ImportSourceType  string    `json:"import_source_type,omitempty"`
	ZeroDataRetention bool      `json:"zero_data_retention"`
	IssuedAt          time.Time `json:"issued_at"`
	ExpiresAt         time.Time `json:"expires_at"`
	Nonce             string    `json:"nonce"`
}

type DecisionExpectation struct {
	OrgID            string
	SpaceRef         string
	SubjectID        string
	ServiceAudience  string
	ActionID         string
	ActionSchemaHash string
	PayloadDigest    string
	IdempotencyKey   string
	Now              time.Time
}

func (d Decision) Validate() error {
	for label, value := range map[string]string{
		"decision_ref":               d.DecisionRef,
		"org_id":                     d.OrgID,
		"space_ref":                  d.SpaceRef,
		"subject_id":                 d.SubjectID,
		"service_audience":           d.ServiceAudience,
		"action_id":                  d.ActionID,
		"action_schema_hash":         d.ActionSchemaHash,
		"payload_digest":             d.PayloadDigest,
		"idempotency_key":            d.IdempotencyKey,
		"recipient_audience_ref":     d.RecipientAudienceRef,
		"recipient_audience_hash":    d.RecipientAudienceHash,
		"privacy_policy_ref":         d.PrivacyPolicyRef,
		"resource_authorization_ref": d.ResourceAuthorizationRef,
		"purpose":                    d.Purpose,
		"lawful_basis":               d.LawfulBasis,
		"privacy_class":              d.PrivacyClass,
		"retention_class":            d.RetentionClass,
		"residency":                  d.Residency,
		"deletion_scope":             d.DeletionScope,
		"nonce":                      d.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space decision %s is required", label)
		}
	}
	if len(d.Permissions) == 0 {
		return fmt.Errorf("space decision permissions are required")
	}
	if err := (AuthorityRevision{
		Authority: d.AuthorityRevision, Membership: d.MembershipRevision,
		Privacy: d.PrivacyRevision, RecipientAudience: d.RecipientAudienceRevision,
		Entitlement: d.EntitlementRevision,
	}).Validate(); err != nil {
		return err
	}
	if d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() || !d.ExpiresAt.After(d.IssuedAt) {
		return fmt.Errorf("space decision expiry must be after issuance")
	}
	return nil
}

// SignDecision issues an asymmetric, key-identified envelope. The private key
// must remain in Control/KMS; target planes verify only with public material.
// The wire format is version.key-id.payload.signature.
func SignDecision(key SigningKey, decision Decision) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid Space decision signing key is required")
	}
	if err := decision.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		return "", fmt.Errorf("marshal Space decision: %w", err)
	}
	encodedKeyID := base64.RawURLEncoding.EncodeToString([]byte(key.ID))
	encoded := base64.RawURLEncoding.EncodeToString(payload)
	signed := decisionVersion + "." + encodedKeyID + "." + encoded
	signature := ed25519.Sign(key.PrivateKey, []byte(signed))
	return signed + "." + base64.RawURLEncoding.EncodeToString(signature), nil
}

// matchesExpectation reports whether the decision exactly binds the caller's
// expected target service, action, and intent.
func (d Decision) matchesExpectation(expected DecisionExpectation) bool {
	return d.OrgID == expected.OrgID && d.SpaceRef == expected.SpaceRef &&
		d.SubjectID == expected.SubjectID && d.ServiceAudience == expected.ServiceAudience &&
		d.ActionID == expected.ActionID && d.ActionSchemaHash == expected.ActionSchemaHash &&
		d.PayloadDigest == expected.PayloadDigest && d.IdempotencyKey == expected.IdempotencyKey
}

func VerifyDecision(key VerificationKey, token string, expected DecisionExpectation) (*Decision, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PublicKey) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("valid Space decision verification key is required")
	}
	parts := strings.Split(token, ".")
	if len(parts) != 4 || parts[0] != decisionVersion {
		return nil, fmt.Errorf("invalid Space decision envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != key.ID {
		return nil, fmt.Errorf("untrusted Space decision signing key")
	}
	provided, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil {
		return nil, fmt.Errorf("invalid Space decision signature encoding")
	}
	signed := strings.Join(parts[:3], ".")
	if !ed25519.Verify(key.PublicKey, []byte(signed), provided) {
		return nil, fmt.Errorf("invalid Space decision signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return nil, fmt.Errorf("invalid Space decision payload encoding")
	}
	var decision Decision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return nil, fmt.Errorf("invalid Space decision payload")
	}
	if err := decision.Validate(); err != nil {
		return nil, err
	}
	if !decision.matchesExpectation(expected) {
		return nil, fmt.Errorf("space decision target does not match expected authority")
	}
	now := expected.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}
	if !now.Before(decision.ExpiresAt) {
		return nil, fmt.Errorf("space decision expired")
	}
	if decision.IssuedAt.After(now.Add(time.Minute)) {
		return nil, fmt.Errorf("space decision issued in the future")
	}
	return &decision, nil
}
