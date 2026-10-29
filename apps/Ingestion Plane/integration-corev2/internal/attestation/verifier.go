package attestation

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"
)

const (
	AttestationType                    = "verevon.provider-write-attestation+jwt"
	AuthorizationHumanIntent           = "human_intent"
	AuthorizationHumanApprovedAIAction = "human_approved_ai_action"

	requiredAudience = "integration-corev2"
	maxCompactJWS    = 16 * 1024
)

var ErrInvalid = errors.New("invalid provider-write attestation")

// supportedIssuers is the closed set of presenter services integration-corev2
// trusts to sign provider-write attestations. Each entry needs its own
// registered key (see ParseTrustedKeysJSON) before it can actually verify —
// this set only bounds which issuer strings are even eligible to register
// one. conversation-core signs human-approved Inbox/Ticketing sends;
// model-execution (execution-core, Model Plane) signs human-approved agent
// tool actions. Adding a new presenter means adding it here AND provisioning
// its key; neither alone is sufficient.
var supportedIssuers = map[string]bool{
	"conversation-core": true,
	"model-execution":   true,
}

func isSupportedIssuer(issuer string) bool {
	return supportedIssuers[issuer]
}

type Header struct {
	Algorithm string `json:"alg"`
	Type      string `json:"typ"`
	KeyID     string `json:"kid"`
}

type Claims struct {
	Version           int64  `json:"v"`
	Issuer            string `json:"iss"`
	Audience          string `json:"aud"`
	PresenterService  string `json:"presenter_service"`
	AuthorizationKind string `json:"authorization_kind"`
	AuthorizationID   string `json:"authorization_id"`
	ApprovalID        string `json:"approval_id,omitempty"`
	ActionID          string `json:"action_id"`
	OrganizationID    string `json:"org_id"`
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

type Binding struct {
	PresenterService string
	OrganizationID   string
	ConnectionID     string
	ProviderKey      string
	Operation        string
	Params           map[string]any
	Body             map[string]any
	PayloadSHA256    string
	IdempotencyKey   string
}

type Verified struct {
	Issuer            string
	KeyID             string
	AuthorizationKind string
	AuthorizationID   string
	ApprovalID        string
	ActionID          string
	ActorID           string
	JWTID             string
	PayloadSHA256     string
}

type trustedKeyJSON struct {
	Issuer    string `json:"issuer"`
	KeyID     string `json:"kid"`
	PublicKey string `json:"public_key"`
}

type TrustedKey struct {
	Issuer    string
	KeyID     string
	PublicKey ed25519.PublicKey
}

type Verifier struct {
	keys map[string]TrustedKey
	now  func() time.Time
}

// ParseTrustedKeysJSON decodes the deployment-supplied trusted-key registry.
// Key ids are required to be globally unique across issuers (not just within
// one issuer's keys): Verify looks a compact JWS's signing key up by kid
// alone, before it has decoded (and so before it can trust) the claimed
// issuer, so two issuers sharing a kid would let one silently shadow the
// other's key in the verifier's lookup table.
func ParseTrustedKeysJSON(raw string) ([]TrustedKey, error) {
	if strings.TrimSpace(raw) == "" || len(raw) > 64*1024 {
		return nil, fmt.Errorf("%w: trusted keys are required", ErrInvalid)
	}
	var encoded []trustedKeyJSON
	if err := decodeStrictJSON([]byte(raw), &encoded); err != nil {
		return nil, fmt.Errorf("%w: decode trusted keys: %v", ErrInvalid, err)
	}
	if len(encoded) == 0 || len(encoded) > 32 {
		return nil, fmt.Errorf("%w: trusted key count must be between 1 and 32", ErrInvalid)
	}
	keys := make([]TrustedKey, 0, len(encoded))
	seenKeyIDs := make(map[string]struct{}, len(encoded))
	for _, candidate := range encoded {
		if !isSupportedIssuer(candidate.Issuer) {
			return nil, fmt.Errorf("%w: unsupported issuer", ErrInvalid)
		}
		if !validIdentifier(candidate.KeyID, 128) || placeholder(candidate.KeyID) {
			return nil, fmt.Errorf("%w: invalid key id", ErrInvalid)
		}
		publicKey, err := base64.StdEncoding.Strict().DecodeString(candidate.PublicKey)
		if err != nil || len(publicKey) != ed25519.PublicKeySize || allBytesEqual(publicKey) {
			return nil, fmt.Errorf("%w: invalid Ed25519 public key", ErrInvalid)
		}
		if _, duplicate := seenKeyIDs[candidate.KeyID]; duplicate {
			return nil, fmt.Errorf("%w: duplicate key id", ErrInvalid)
		}
		seenKeyIDs[candidate.KeyID] = struct{}{}
		keys = append(keys, TrustedKey{
			Issuer: candidate.Issuer, KeyID: candidate.KeyID, PublicKey: append(ed25519.PublicKey(nil), publicKey...),
		})
	}
	return keys, nil
}

// NewVerifier indexes by KeyID alone (see ParseTrustedKeysJSON); a caller that
// builds keys by hand rather than through ParseTrustedKeysJSON is responsible
// for the same global-kid-uniqueness invariant, since a duplicate here simply
// keeps the last entry.
func NewVerifier(keys []TrustedKey, now func() time.Time) *Verifier {
	keyMap := make(map[string]TrustedKey, len(keys))
	for _, key := range keys {
		keyMap[key.KeyID] = TrustedKey{
			Issuer: key.Issuer, KeyID: key.KeyID, PublicKey: append(ed25519.PublicKey(nil), key.PublicKey...),
		}
	}
	if now == nil {
		now = time.Now
	}
	return &Verifier{keys: keyMap, now: now}
}

func (v *Verifier) Verify(compact string, binding Binding) (Verified, error) {
	if v == nil || len(v.keys) == 0 || len(compact) == 0 || len(compact) > maxCompactJWS || strings.TrimSpace(compact) != compact {
		return Verified{}, ErrInvalid
	}
	segments := strings.Split(compact, ".")
	if len(segments) != 3 || segments[0] == "" || segments[1] == "" || segments[2] == "" {
		return Verified{}, ErrInvalid
	}
	headerJSON, err := base64.RawURLEncoding.Strict().DecodeString(segments[0])
	if err != nil {
		return Verified{}, ErrInvalid
	}
	var header Header
	if err := decodeStrictJSON(headerJSON, &header); err != nil || header.Algorithm != "EdDSA" || header.Type != AttestationType || !validIdentifier(header.KeyID, 128) {
		return Verified{}, ErrInvalid
	}
	trusted, ok := v.keys[header.KeyID]
	if !ok {
		return Verified{}, ErrInvalid
	}
	signature, err := base64.RawURLEncoding.Strict().DecodeString(segments[2])
	if err != nil || len(signature) != ed25519.SignatureSize || !ed25519.Verify(trusted.PublicKey, []byte(segments[0]+"."+segments[1]), signature) {
		return Verified{}, ErrInvalid
	}
	claimsJSON, err := base64.RawURLEncoding.Strict().DecodeString(segments[1])
	if err != nil {
		return Verified{}, ErrInvalid
	}
	var claims Claims
	if err := decodeStrictJSON(claimsJSON, &claims); err != nil {
		return Verified{}, ErrInvalid
	}
	var claimFields map[string]json.RawMessage
	if err := json.Unmarshal(claimsJSON, &claimFields); err != nil {
		return Verified{}, ErrInvalid
	}
	// The signing key is trusted for exactly one issuer; a claims payload
	// signed with this key but claiming a different issuer is rejected here,
	// before any of its other fields are trusted.
	if claims.Issuer != trusted.Issuer {
		return Verified{}, ErrInvalid
	}
	if err := validateClaims(claims, claimFields, binding, v.now().UTC()); err != nil {
		return Verified{}, err
	}
	return Verified{
		Issuer: claims.Issuer, KeyID: header.KeyID, AuthorizationKind: claims.AuthorizationKind,
		AuthorizationID: claims.AuthorizationID, ApprovalID: claims.ApprovalID, ActionID: claims.ActionID,
		ActorID: claims.ActorID, JWTID: claims.JWTID, PayloadSHA256: claims.PayloadSHA256,
	}, nil
}

func PayloadSHA256(binding Binding) (string, error) {
	canonical, err := json.Marshal(struct {
		OrganizationID string         `json:"org_id"`
		ConnectionID   string         `json:"connection_id"`
		ProviderKey    string         `json:"provider_key"`
		Operation      string         `json:"operation"`
		Params         map[string]any `json:"params"`
		Body           map[string]any `json:"body"`
	}{
		OrganizationID: strings.TrimSpace(binding.OrganizationID),
		ConnectionID:   strings.TrimSpace(binding.ConnectionID),
		ProviderKey:    strings.TrimSpace(binding.ProviderKey),
		Operation:      strings.TrimSpace(binding.Operation),
		Params:         binding.Params,
		Body:           binding.Body,
	})
	if err != nil {
		return "", fmt.Errorf("marshal provider-write payload: %w", err)
	}
	digest := sha256.Sum256(canonical)
	return hex.EncodeToString(digest[:]), nil
}

func validateClaims(claims Claims, fields map[string]json.RawMessage, binding Binding, now time.Time) error {
	digest, err := PayloadSHA256(binding)
	if err != nil {
		return fmt.Errorf("%w: payload is not canonicalizable", ErrInvalid)
	}
	if binding.PayloadSHA256 != "" && binding.PayloadSHA256 != digest {
		return fmt.Errorf("%w: caller payload digest mismatch", ErrInvalid)
	}
	nowUnix := now.Unix()
	if claims.Version != 1 || claims.Audience != requiredAudience ||
		claims.PresenterService != binding.PresenterService || claims.OrganizationID != binding.OrganizationID ||
		claims.ConnectionID != binding.ConnectionID || claims.ProviderKey != binding.ProviderKey ||
		claims.Operation != strings.TrimSpace(binding.Operation) || claims.PayloadSHA256 != digest ||
		claims.IdempotencyKey != binding.IdempotencyKey {
		return ErrInvalid
	}
	if !validIdentifier(claims.PresenterService, 128) || !validIdentifier(claims.AuthorizationID, 256) ||
		!validIdentifier(claims.ActionID, 256) || !validIdentifier(claims.OrganizationID, 256) ||
		!validIdentifier(claims.ConnectionID, 256) || !validIdentifier(claims.ProviderKey, 128) ||
		!validIdentifier(claims.Operation, 256) || !validIdentifier(claims.ActorID, 256) ||
		!validIdentifier(claims.IdempotencyKey, 200) || !validIdentifier(claims.JWTID, 256) ||
		!validSHA256(claims.PayloadSHA256) {
		return ErrInvalid
	}
	_, approvalPresent := fields["approval_id"]
	switch claims.AuthorizationKind {
	case AuthorizationHumanIntent:
		if approvalPresent || claims.ApprovalID != "" || claims.ActionID != claims.AuthorizationID {
			return ErrInvalid
		}
	case AuthorizationHumanApprovedAIAction:
		if !approvalPresent || !validIdentifier(claims.ApprovalID, 256) || claims.ApprovalID != claims.ActionID {
			return ErrInvalid
		}
	default:
		return ErrInvalid
	}
	lifetimeSeconds := claims.ExpiresAt - claims.IssuedAt
	if claims.IssuedAt <= 0 || claims.NotBefore <= 0 || claims.ExpiresAt <= 0 ||
		claims.IssuedAt != claims.NotBefore || lifetimeSeconds < 1 || lifetimeSeconds > 60 ||
		claims.IssuedAt > nowUnix || claims.NotBefore > nowUnix || claims.ExpiresAt <= nowUnix ||
		claims.NotBefore > claims.ExpiresAt || claims.IssuedAt > claims.ExpiresAt {
		return ErrInvalid
	}
	return nil
}

func decodeStrictJSON(raw []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if decoder.More() {
		return errors.New("unexpected trailing JSON")
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return errors.New("unexpected trailing JSON")
		}
		return err
	}
	return nil
}

func validIdentifier(value string, max int) bool {
	return value != "" && len(value) <= max && strings.TrimSpace(value) == value
}

func validSHA256(value string) bool {
	if len(value) != sha256.Size*2 || strings.ToLower(value) != value {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}

func placeholder(value string) bool {
	lower := strings.ToLower(strings.TrimSpace(value))
	return strings.HasPrefix(lower, "change-me") || strings.HasPrefix(lower, "replace-with") || lower == "placeholder"
}

func allBytesEqual(value []byte) bool {
	if len(value) == 0 {
		return true
	}
	for _, candidate := range value[1:] {
		if candidate != value[0] {
			return false
		}
	}
	return true
}
