// Package spacedecision verifies the Control-signed Space decisions that gate the room canvas (WS19-S3). It follows the
// documents-api verifier (services/documents-api-go/internal/handler/space_import.go) and the Control issuer
// (apps/Control Plane/user-core/internal/spaces/canvas_decision.go). The envelope is v2.<key id>.<payload>.<signature>,
// Ed25519 over the first three parts. The canvas payload digest is recomputed in digest.go, byte for byte, and the golden
// vector in user-core's testdata pins it on both sides.
package spacedecision

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"time"
)

// EnvelopeVersion and MaxEnvelopeBytes are part of the decision wire contract.
const (
	EnvelopeVersion  = "v2"
	MaxEnvelopeBytes = 16 * 1024
	clockSkew        = time.Minute
)

// Refusal codes. The handler answers each one with HTTP 403 and the code unchanged.
const (
	CodeDecisionInvalid  = "space_decision_invalid"
	CodeDecisionMismatch = "space_decision_mismatch"
	CodeDigestMismatch   = "space_decision_digest_mismatch"
	CodeHistoryFloor     = "space_history_floor_unsupported"
)

// Canvas authority. These match Control's canvas_decision.go. A change needs both sides and the golden vector.
const (
	AudienceCanvas        = "data-plane-canvas"
	ActionCanvasRead      = "data.canvas.read"
	ActionCanvasWrite     = "data.canvas.write"
	SchemaCanvasRead      = "sha256:space-canvas-read-v1"
	SchemaCanvasWrite     = "sha256:space-canvas-write-v1"
	PermissionCanvasRead  = "canvas:read"
	PermissionCanvasWrite = "canvas:write"
)

// Refusal is a typed refusal of a decision. Code is the wire code. Message is short and safe to show the caller.
type Refusal struct {
	Code    string
	Message string
}

func (e *Refusal) Error() string { return e.Code + ": " + e.Message }

func refuse(code, message string) error { return &Refusal{Code: code, Message: message} }

// KeySet maps a Control key id to its Ed25519 public key. It holds public material only.
type KeySet map[string]ed25519.PublicKey

// ParseKeySet builds the key set from the rotation JSON map (key id to base64url public key) when that is set. Otherwise
// it uses the single key id and key. Startup calls it and refuses on any error, so an unusable key never serves a request.
func ParseKeySet(rawJSON, keyID, publicKeyBase64 string) (KeySet, error) {
	encoded := map[string]string{}
	if raw := strings.TrimSpace(rawJSON); raw != "" {
		if err := json.Unmarshal([]byte(raw), &encoded); err != nil {
			return nil, errors.New("the Control Space decision key set is not valid JSON")
		}
	} else {
		id := strings.TrimSpace(keyID)
		key := strings.TrimSpace(publicKeyBase64)
		if id == "" || key == "" {
			return nil, errors.New("no Control Space decision key is configured")
		}
		encoded[id] = key
	}
	if len(encoded) == 0 {
		return nil, errors.New("the Control Space decision key set is empty")
	}
	keys := make(KeySet, len(encoded))
	for id, raw := range encoded {
		key, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(raw))
		if strings.TrimSpace(id) == "" || err != nil || len(key) != ed25519.PublicKeySize {
			return nil, errors.New("a Control Space decision public key is not valid")
		}
		keys[id] = ed25519.PublicKey(key)
	}
	return keys, nil
}

// Claims is the decision body Control signs (user-core decision.go, Decision). The JSON names and types match Control's.
// Fields that wiki-store does not read are left out.
type Claims struct {
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
	RecipientAudienceHash     string    `json:"recipient_audience_hash"`
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
	RetentionClass            string    `json:"retention_class"`
	Residency                 string    `json:"residency"`
	DeletionScope             string    `json:"deletion_scope"`
	HistoryFloorRevision      int64     `json:"history_floor_revision,omitempty"`
	ZeroDataRetention         bool      `json:"zero_data_retention"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
	Nonce                     string    `json:"nonce"`
	SpaceKind                 string    `json:"space_kind,omitempty"`
}

// Verify checks the envelope, its key id, the Ed25519 signature, the payload's completeness and its life. It binds no
// route: CheckCanvas binds the route, and the digest binds the operation. Every failure here is space_decision_invalid.
func Verify(token string, keys KeySet, now time.Time) (Claims, error) {
	if token == "" || len(token) > MaxEnvelopeBytes {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision envelope is not valid")
	}
	parts := strings.Split(token, ".")
	if len(parts) != 4 || parts[0] != EnvelopeVersion {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision envelope is not valid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || len(keyID) == 0 {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision key is not trusted")
	}
	key, ok := keys[string(keyID)]
	if !ok || len(key) != ed25519.PublicKeySize {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision key is not trusted")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision signature is not valid")
	}
	if !ed25519.Verify(key, []byte(strings.Join(parts[:3], ".")), signature) {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision signature is not valid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || len(payload) == 0 {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision payload is not valid")
	}
	var claims Claims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return Claims{}, refuse(CodeDecisionInvalid, "the Space decision payload is not valid")
	}
	if err := claims.validate(now); err != nil {
		return Claims{}, err
	}
	return claims, nil
}

// validate mirrors Control's Decision.Validate and its window check. Every required reference is set, the revisions are
// positive, and the decision is inside its life and was not issued in the future.
func (c Claims) validate(now time.Time) error {
	for _, value := range []string{
		c.DecisionRef, c.OrgID, c.SpaceRef, c.SubjectID, c.ServiceAudience, c.ActionID, c.ActionSchemaHash,
		c.PayloadDigest, c.IdempotencyKey, c.RecipientAudienceRef, c.RecipientAudienceHash, c.PrivacyPolicyRef,
		c.ResourceAuthorizationRef, c.Purpose, c.LawfulBasis, c.PrivacyClass, c.RetentionClass, c.Residency,
		c.DeletionScope, c.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return refuse(CodeDecisionInvalid, "the Space decision is incomplete")
		}
	}
	if len(c.Permissions) == 0 {
		return refuse(CodeDecisionInvalid, "the Space decision carries no permission")
	}
	if c.AuthorityRevision <= 0 || c.MembershipRevision <= 0 || c.PrivacyRevision <= 0 ||
		c.RecipientAudienceRevision <= 0 || c.EntitlementRevision <= 0 {
		return refuse(CodeDecisionInvalid, "the Space decision revisions are not positive")
	}
	if c.IssuedAt.IsZero() || c.ExpiresAt.IsZero() || !c.ExpiresAt.After(c.IssuedAt) {
		return refuse(CodeDecisionInvalid, "the Space decision window is not valid")
	}
	if !now.Before(c.ExpiresAt) {
		return refuse(CodeDecisionInvalid, "the Space decision has expired")
	}
	if c.IssuedAt.After(now.Add(clockSkew)) {
		return refuse(CodeDecisionInvalid, "the Space decision was issued in the future")
	}
	return nil
}

// CheckCanvas binds a verified decision to the canvas request it arrives with. The caller's org and subject, the path's
// Space, the canvas audience, the read or write action, schema and permission, and the Space kind must all match. A
// decision issued under zero data retention is refused. So is one that carries a history floor: canvas text has no
// history window yet, so a since_join member is refused rather than shown text written before the join.
func (c Claims) CheckCanvas(orgID, subjectID, spaceRef string, write bool) error {
	action, schema, permission := ActionCanvasRead, SchemaCanvasRead, PermissionCanvasRead
	if write {
		action, schema, permission = ActionCanvasWrite, SchemaCanvasWrite, PermissionCanvasWrite
	}
	mismatch := func(message string) error { return refuse(CodeDecisionMismatch, message) }
	switch {
	case c.OrgID != orgID:
		return mismatch("the Space decision belongs to another organization")
	case c.SpaceRef != spaceRef:
		return mismatch("the Space decision was issued for another Space")
	case c.SubjectID != subjectID:
		return mismatch("the Space decision was issued to another user")
	case c.ServiceAudience != AudienceCanvas:
		return mismatch("the Space decision was issued for another service")
	case c.ActionID != action || c.ActionSchemaHash != schema:
		return mismatch("the Space decision authorizes another action")
	case !slices.Contains(c.Permissions, permission):
		return mismatch("the Space decision lacks the canvas permission")
	case c.ZeroDataRetention:
		return mismatch("the Space decision is issued under zero data retention")
	case c.HistoryFloorRevision > 0:
		return refuse(CodeHistoryFloor, "this room's history window does not apply to the canvas")
	}
	switch c.SpaceKind {
	case "personal", "room", "project", "case":
		return nil
	default:
		return mismatch("the Space kind has no canvas")
	}
}
