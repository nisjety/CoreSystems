package handler

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

const (
	spaceImportDecisionHeader   = "X-Space-Import-Decision"
	spaceImportAcceptedHeader   = "X-Space-Import-Authority-Accepted"
	spaceImportDecisionAudience = "data-plane-import"
	spaceImportDecisionAction   = "ingestion.import.write"
	spaceImportDecisionSchema   = "sha256:ingestion-import-v1"
	maxSpaceImportDecisionBytes = 16 * 1024
)

type spaceImportDecisionClaims struct {
	DecisionRef               string    `json:"decision_ref"`
	OrgID                     string    `json:"org_id"`
	SpaceRef                  string    `json:"space_ref"`
	SubjectID                 string    `json:"subject_id"`
	ServiceAudience           string    `json:"service_audience"`
	ActionID                  string    `json:"action_id"`
	ActionSchemaHash          string    `json:"action_schema_hash"`
	PayloadDigest             string    `json:"payload_digest"`
	IdempotencyKey            string    `json:"idempotency_key"`
	RecipientAudienceRef      string    `json:"recipient_audience_ref"`
	PrivacyPolicyRef          string    `json:"privacy_policy_ref"`
	ResourceAuthorizationRef  string    `json:"resource_authorization_ref"`
	AuthorityRevision         int64     `json:"authority_revision"`
	MembershipRevision        int64     `json:"membership_revision"`
	PrivacyRevision           int64     `json:"privacy_revision"`
	RecipientAudienceRevision int64     `json:"recipient_audience_revision"`
	EntitlementRevision       int64     `json:"entitlement_revision"`
	Permissions               []string  `json:"permissions"`
	Purpose                   string    `json:"purpose"`
	LawfulBasis               string    `json:"lawful_basis"`
	PrivacyClass              string    `json:"privacy_class"`
	ThirdPartyAllowed         bool      `json:"third_party_processing_allowed"`
	RetentionClass            string    `json:"retention_class"`
	Residency                 string    `json:"residency"`
	DeletionScope             string    `json:"deletion_scope"`
	ImportSourceType          string    `json:"import_source_type"`
	ZeroDataRetention         bool      `json:"zero_data_retention"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
	Nonce                     string    `json:"nonce"`
}

// verifySpaceImportDecision proves that Imports Core was issued fresh Control
// authority for this exact Data target. It is deliberately optional only for
// legacy unscoped callers. Once the header is supplied it is never ignored.
func verifySpaceImportDecision(r *http.Request) (*spaceImportDecisionClaims, error) {
	token := strings.TrimSpace(r.Header.Get(spaceImportDecisionHeader))
	if token == "" {
		return nil, nil
	}
	claims := verifiedClaims(r)
	if claims == nil || !claims.IsService() || claims.ServiceID != "imports-core" || !claims.HasScope("documents:write") {
		return nil, fmt.Errorf("Space import authority requires the scoped Imports Core principal")
	}
	parts := strings.Split(token, ".")
	if len(parts) != 4 || parts[0] != "v2" || len(token) > maxSpaceImportDecisionBytes {
		return nil, fmt.Errorf("invalid Space import decision envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || strings.TrimSpace(string(keyID)) == "" {
		return nil, fmt.Errorf("untrusted Space import decision key")
	}
	keys, err := configuredSpaceDecisionKeys()
	if err != nil {
		return nil, err
	}
	key, exists := keys[string(keyID)]
	if !exists {
		return nil, fmt.Errorf("untrusted Space import decision key")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || len(payload) == 0 || len(payload) > maxSpaceImportDecisionBytes {
		return nil, fmt.Errorf("invalid Space import decision payload")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize || !ed25519.Verify(key, []byte(strings.Join(parts[:3], ".")), signature) {
		return nil, fmt.Errorf("invalid Space import decision signature")
	}
	var decision spaceImportDecisionClaims
	if err := json.Unmarshal(payload, &decision); err != nil {
		return nil, fmt.Errorf("invalid Space import decision claims")
	}
	if err := decision.validate(OrgIDFrom(r.Context()), time.Now().UTC()); err != nil {
		return nil, err
	}
	return &decision, nil
}

func configuredSpaceDecisionKeys() (map[string]ed25519.PublicKey, error) {
	raw := strings.TrimSpace(os.Getenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON"))
	encoded := map[string]string{}
	if raw != "" {
		if err := json.Unmarshal([]byte(raw), &encoded); err != nil {
			return nil, fmt.Errorf("invalid Control Space decision key set")
		}
	} else {
		id := strings.TrimSpace(os.Getenv("CONTROL_SPACE_DECISION_KEY_ID"))
		key := strings.TrimSpace(os.Getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64"))
		if id == "" || key == "" {
			return nil, fmt.Errorf("Control Space decision key is unavailable")
		}
		encoded[id] = key
	}
	if len(encoded) == 0 {
		return nil, fmt.Errorf("Control Space decision key set is empty")
	}
	keys := make(map[string]ed25519.PublicKey, len(encoded))
	for id, rawKey := range encoded {
		key, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(rawKey))
		if strings.TrimSpace(id) == "" || err != nil || len(key) != ed25519.PublicKeySize {
			return nil, fmt.Errorf("invalid Control Space decision key")
		}
		keys[id] = ed25519.PublicKey(key)
	}
	return keys, nil
}

func (d spaceImportDecisionClaims) validate(orgID string, now time.Time) error {
	for _, value := range []string{
		d.DecisionRef, d.OrgID, d.SpaceRef, d.SubjectID, d.RecipientAudienceRef,
		d.PrivacyPolicyRef, d.ResourceAuthorizationRef, d.PayloadDigest, d.IdempotencyKey,
		d.Purpose, d.LawfulBasis, d.PrivacyClass, d.RetentionClass, d.Residency,
		d.DeletionScope, d.ImportSourceType, d.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("incomplete Space import decision")
		}
	}
	if d.OrgID != orgID || d.ServiceAudience != spaceImportDecisionAudience || d.ActionID != spaceImportDecisionAction || d.ActionSchemaHash != spaceImportDecisionSchema || d.ZeroDataRetention || d.IssuedAt.After(now.Add(time.Minute)) || !d.ExpiresAt.After(now) || d.AuthorityRevision <= 0 || d.MembershipRevision <= 0 || d.PrivacyRevision <= 0 || d.RecipientAudienceRevision <= 0 || d.EntitlementRevision <= 0 {
		return fmt.Errorf("Space import decision does not authorize this write")
	}
	for _, permission := range d.Permissions {
		if permission == "documents:write" {
			return nil
		}
	}
	return fmt.Errorf("Space import decision lacks documents:write")
}

func (d spaceImportDecisionClaims) matchesDocumentInputType(documentType string) bool {
	return strings.TrimSpace(documentType) == d.ImportSourceType
}
