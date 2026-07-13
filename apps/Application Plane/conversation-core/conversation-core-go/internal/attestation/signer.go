// Package attestation issues short-lived, effect-bound provider-write proofs.
// The private key remains in conversation-core; integration-corev2 verifies the
// compact JWS before it permits any provider mutation.
package attestation

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"regexp"
	"strings"
	"time"
)

const (
	IssuerConversationCore       = "conversation-core"
	AudienceIntegrationCore      = "integration-corev2"
	PresenterConversationCore    = "conversation-core"
	TypeProviderWriteAttestation = "velion.provider-write-attestation+jwt"

	AuthorizationHumanIntent           = "human_intent"
	AuthorizationHumanApprovedAIAction = "human_approved_ai_action"

	defaultTTL = 30 * time.Second
	maxTTL     = 60 * time.Second
)

var (
	keyIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`)
	hexSHA256    = regexp.MustCompile(`^[0-9a-f]{64}$`)
)

// Config fixes the trust domain and supplies the Ed25519 signing key.
type Config struct {
	PrivateKey ed25519.PrivateKey
	KeyID      string
	Issuer     string
	Audience   string
	Presenter  string
	TTL        time.Duration
	Now        func() time.Time
	Random     io.Reader
}

// Authorization is the durable, exact provider effect conversation-core has
// authorized. Content is represented only by PayloadSHA256.
type Authorization struct {
	AuthorizationKind string
	AuthorizationID   string
	ApprovalID        string
	ActionID          string
	OrgID             string
	ConnectionID      string
	ProviderKey       string
	Operation         string
	ActorID           string
	PayloadSHA256     string
	IdempotencyKey    string
}

// Claims is the signed wire contract verified by integration-corev2.
type Claims struct {
	Version           int    `json:"v"`
	Issuer            string `json:"iss"`
	Audience          string `json:"aud"`
	PresenterService  string `json:"presenter_service"`
	AuthorizationKind string `json:"authorization_kind"`
	AuthorizationID   string `json:"authorization_id"`
	ApprovalID        string `json:"approval_id,omitempty"`
	ActionID          string `json:"action_id"`
	OrgID             string `json:"org_id"`
	ConnectionID      string `json:"connection_id"`
	ProviderKey       string `json:"provider_key"`
	Operation         string `json:"operation"`
	ActorID           string `json:"actor_id"`
	PayloadSHA256     string `json:"payload_sha256"`
	IdempotencyKey    string `json:"idempotency_key"`
	JWTID             string `json:"jti"`
	IssuedAt          int64  `json:"iat"`
	NotBefore         int64  `json:"nbf"`
	ExpiresAt         int64  `json:"exp"`
}

type Signer struct {
	privateKey ed25519.PrivateKey
	keyID      string
	ttl        time.Duration
	now        func() time.Time
	random     io.Reader
}

func NewSigner(config Config) (*Signer, error) {
	if len(config.PrivateKey) != ed25519.PrivateKeySize {
		return nil, fmt.Errorf("provider-write attestation private key must be a %d-byte Ed25519 private key", ed25519.PrivateKeySize)
	}
	derivedPrivateKey := ed25519.NewKeyFromSeed(config.PrivateKey[:ed25519.SeedSize])
	if subtle.ConstantTimeCompare(config.PrivateKey, derivedPrivateKey) != 1 {
		return nil, fmt.Errorf("provider-write attestation private key is internally inconsistent")
	}
	keyID := strings.TrimSpace(config.KeyID)
	if !keyIDPattern.MatchString(keyID) {
		return nil, fmt.Errorf("provider-write attestation key id is invalid")
	}
	if config.Issuer != IssuerConversationCore || config.Audience != AudienceIntegrationCore || config.Presenter != PresenterConversationCore {
		return nil, fmt.Errorf("provider-write attestation trust domain is invalid")
	}
	ttl := config.TTL
	if ttl == 0 {
		ttl = defaultTTL
	}
	if ttl < time.Second || ttl > maxTTL {
		return nil, fmt.Errorf("provider-write attestation TTL must be between 1s and %s", maxTTL)
	}
	now := config.Now
	if now == nil {
		now = time.Now
	}
	random := config.Random
	if random == nil {
		random = rand.Reader
	}
	return &Signer{
		privateKey: append(ed25519.PrivateKey(nil), config.PrivateKey...),
		keyID:      keyID, ttl: ttl, now: now, random: random,
	}, nil
}

// Sign returns a compact Ed25519 JWS with the exact protected header and
// effect-bound claims required by integration-corev2.
func (s *Signer) Sign(authorization Authorization) (string, error) {
	if s == nil || len(s.privateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("provider-write attestation signer is not configured")
	}
	authorization = normalizeAuthorization(authorization)
	if err := validateAuthorization(authorization); err != nil {
		return "", err
	}
	jtiBytes := make([]byte, 16)
	if _, err := io.ReadFull(s.random, jtiBytes); err != nil {
		return "", fmt.Errorf("generate provider-write attestation jti: %w", err)
	}
	now := s.now().UTC().Truncate(time.Second)
	claims := Claims{
		Version: 1, Issuer: IssuerConversationCore, Audience: AudienceIntegrationCore,
		PresenterService:  PresenterConversationCore,
		AuthorizationKind: authorization.AuthorizationKind,
		AuthorizationID:   authorization.AuthorizationID, ApprovalID: authorization.ApprovalID,
		ActionID: authorization.ActionID, OrgID: authorization.OrgID,
		ConnectionID: authorization.ConnectionID, ProviderKey: authorization.ProviderKey,
		Operation: authorization.Operation, ActorID: authorization.ActorID,
		PayloadSHA256: authorization.PayloadSHA256, IdempotencyKey: authorization.IdempotencyKey,
		JWTID:    base64.RawURLEncoding.EncodeToString(jtiBytes),
		IssuedAt: now.Unix(), NotBefore: now.Unix(), ExpiresAt: now.Add(s.ttl).Unix(),
	}
	header := struct {
		Algorithm string `json:"alg"`
		Type      string `json:"typ"`
		KeyID     string `json:"kid"`
	}{Algorithm: "EdDSA", Type: TypeProviderWriteAttestation, KeyID: s.keyID}
	headerJSON, err := json.Marshal(header)
	if err != nil {
		return "", fmt.Errorf("encode provider-write attestation header: %w", err)
	}
	claimsJSON, err := json.Marshal(claims)
	if err != nil {
		return "", fmt.Errorf("encode provider-write attestation claims: %w", err)
	}
	protected := base64.RawURLEncoding.EncodeToString(headerJSON) + "." + base64.RawURLEncoding.EncodeToString(claimsJSON)
	signature := ed25519.Sign(s.privateKey, []byte(protected))
	return protected + "." + base64.RawURLEncoding.EncodeToString(signature), nil
}

func normalizeAuthorization(value Authorization) Authorization {
	return Authorization{
		AuthorizationKind: strings.TrimSpace(value.AuthorizationKind),
		AuthorizationID:   strings.TrimSpace(value.AuthorizationID), ApprovalID: strings.TrimSpace(value.ApprovalID),
		ActionID: strings.TrimSpace(value.ActionID), OrgID: strings.TrimSpace(value.OrgID),
		ConnectionID: strings.TrimSpace(value.ConnectionID), ProviderKey: strings.TrimSpace(value.ProviderKey),
		Operation: strings.TrimSpace(value.Operation), ActorID: strings.TrimSpace(value.ActorID),
		PayloadSHA256: strings.TrimSpace(value.PayloadSHA256), IdempotencyKey: strings.TrimSpace(value.IdempotencyKey),
	}
}

func validateAuthorization(value Authorization) error {
	for name, field := range map[string]string{
		"authorization_id": value.AuthorizationID, "action_id": value.ActionID,
		"org_id": value.OrgID, "connection_id": value.ConnectionID,
		"provider_key": value.ProviderKey, "operation": value.Operation,
		"actor_id": value.ActorID, "idempotency_key": value.IdempotencyKey,
	} {
		if field == "" {
			return fmt.Errorf("provider-write attestation %s is required", name)
		}
	}
	if !hexSHA256.MatchString(value.PayloadSHA256) {
		return fmt.Errorf("provider-write attestation payload_sha256 must be lowercase hexadecimal SHA-256")
	}
	switch value.AuthorizationKind {
	case AuthorizationHumanIntent:
		if value.ApprovalID != "" || value.ActionID != value.AuthorizationID {
			return fmt.Errorf("human intent must omit approval_id and bind action_id to authorization_id")
		}
	case AuthorizationHumanApprovedAIAction:
		if value.ApprovalID == "" || value.ApprovalID != value.ActionID {
			return fmt.Errorf("approved AI action must bind matching approval_id and action_id")
		}
	default:
		return fmt.Errorf("provider-write attestation authorization_kind is invalid")
	}
	return nil
}

// DecodePrivateKey parses the standard-base64 64-byte Ed25519 private-key
// representation used by deployment secret references.
func DecodePrivateKey(encoded string) (ed25519.PrivateKey, error) {
	decoded, err := base64.StdEncoding.Strict().DecodeString(strings.TrimSpace(encoded))
	if err != nil || len(decoded) != ed25519.PrivateKeySize {
		return nil, fmt.Errorf("provider-write attestation private key must be standard-base64 encoded %d-byte Ed25519 private key", ed25519.PrivateKeySize)
	}
	return ed25519.PrivateKey(decoded), nil
}

// IsLowerHexSHA256 is shared by boundary validation without exposing the
// signing key or accepting non-canonical digest encodings.
func IsLowerHexSHA256(value string) bool {
	if !hexSHA256.MatchString(value) {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}
