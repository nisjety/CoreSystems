package authz

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"time"
)

const (
	spaceCapabilityDecisionVersion = "v2"
	spaceCapabilityAction          = "model.space.capability_profile"
	spaceCapabilityAudience        = "model-plane-sandbox-manager"
	spaceCapabilitySchema          = "sha256:space-capability-profile-v1"
)

// SpaceCapabilityVerifier verifies only Control's public, key-identified
// Space capability decision envelope (user-core's
// internal/spaces/space_capability_decision.go). It holds no private key —
// mirrors capability-core's ControlDecisionVerifier
// (internal/cron/control_authorizer.go:110-145, 275-297), kept as a separate
// local copy because sandbox-manager and user-core are independently
// deployed Go modules with no shared package for this envelope.
type SpaceCapabilityVerifier struct {
	keyID  string
	public ed25519.PublicKey
}

// LoadSpaceCapabilityVerifierFromEnv reads the same deployment-distributed
// Control Space decision public key used by every other recipient plane
// (CONTROL_SPACE_DECISION_KEY_ID / CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64
// — see capability-core/cmd/main.go and orchestrator-core/internal/config).
// There is no development fallback: a missing or malformed key must not
// silently disable verification.
func LoadSpaceCapabilityVerifierFromEnv(getenv func(string) string) (*SpaceCapabilityVerifier, error) {
	if getenv == nil {
		return nil, fmt.Errorf("Space capability verifier environment reader is required")
	}
	keyID := strings.TrimSpace(getenv("CONTROL_SPACE_DECISION_KEY_ID"))
	publicRaw, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64")))
	if keyID == "" || err != nil || len(publicRaw) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Space capability decision public key is invalid")
	}
	return &SpaceCapabilityVerifier{keyID: keyID, public: ed25519.PublicKey(publicRaw)}, nil
}

// spaceCapabilityDecision mirrors Control's Decision envelope
// (user-core/internal/spaces/decision.go) field-for-field via matching JSON
// tags. Only the fields this verifier actually needs are declared; the
// envelope carries more (Purpose, LawfulBasis, ...) that this recipient has
// no reason to interpret.
type spaceCapabilityDecision struct {
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
	ExpiresAt                 time.Time `json:"expires_at"`
}

// SpaceCapabilityClaims mirrors Control's unsigned claims sidecar
// (user-core/internal/http/spaces.go: sandboxCapabilityClaims) returned
// alongside the signed decision token. The decision's PayloadDigest binds to
// exactly these values, so tampering with the plaintext claims — in
// particular BackendID, the field this verifier exists to pin — invalidates
// the digest check even though claims are never signed directly.
type SpaceCapabilityClaims struct {
	BackendID      string `json:"backend_id"`
	ProfileDigest  string `json:"profile_digest"`
	Persistence    string `json:"persistence"`
	Processes      string `json:"processes"`
	Backup         bool   `json:"backup"`
	Egress         string `json:"egress"`
	CredentialMode string `json:"credential_mode"`
}

// CapabilityExpectation is the request-derived context a verified decision
// must match. Nothing here is trusted from the token or claims themselves —
// it comes from the caller's authenticated identity and the lease request.
type CapabilityExpectation struct {
	OrgID     string
	SpaceRef  string
	SubjectID string
	Now       time.Time
}

// ProcessesBackgroundRegistry is the substrate profile value a backend
// reports when it can host reattachable background processes, as opposed to
// the bounded one-shot children every execution-core can run. Control
// validates the claim against this same closed vocabulary before signing
// (S4.2 design doc §4).
const ProcessesBackgroundRegistry = "background_registry"

// VerifiedCapability is what a caller may trust after Verify: the claims
// themselves, plus the permissions the decision actually granted.
//
// Permissions are surfaced (rather than consumed entirely inside Verify)
// because AcquireLease has to persist one of them — whether this lease may
// host background processes — onto the lease row. The leases table otherwise
// keeps nothing from the decision but backend_id, so without this there is
// nothing for a later process RPC, arriving on a service token with no user
// bearer in hand, to check against.
type VerifiedCapability struct {
	Claims      SpaceCapabilityClaims
	Permissions []string
	// RecipientAudienceRevision is the Space's audience revision the decision
	// was signed under. Surfaced for the same reason Permissions is: the lease
	// row has to keep it, because a later reader arrives with its own decision
	// and something has to say which audience the recorded work belongs to.
	RecipientAudienceRevision int64
}

// HasPermission reports whether the decision granted permission.
func (c VerifiedCapability) HasPermission(permission string) bool {
	return hasPermission(c.Permissions, permission)
}

// AllowsBackgroundProcesses reports whether this lease may host S4.2
// background processes: the backend must claim the capability AND Control
// must have granted it.
//
// Both halves are required and neither implies the other. A backend claiming
// the profile without the grant is a Space that simply is not entitled —
// which, unlike egress, is NOT a reason to refuse the lease: a
// process-capable backend is still a perfectly good one-shot substrate. The
// grant without the claim would be a Space entitled to something this
// backend cannot do.
func (c VerifiedCapability) AllowsBackgroundProcesses() bool {
	return c.Claims.Processes == ProcessesBackgroundRegistry && c.HasPermission("space:processes")
}

// Verify checks token's signature and claimsJSON's digest binding, then
// returns the claims a caller may trust — in particular BackendID, which
// AcquireLease pins against this instance's own configured backend id — plus
// the decision's granted permissions.
func (v *SpaceCapabilityVerifier) Verify(token, claimsJSON string, expect CapabilityExpectation) (VerifiedCapability, error) {
	decision, err := v.verify(token)
	if err != nil {
		return VerifiedCapability{}, err
	}
	var claims SpaceCapabilityClaims
	if err := json.Unmarshal([]byte(claimsJSON), &claims); err != nil {
		return VerifiedCapability{}, fmt.Errorf("invalid Space capability claims payload")
	}
	if decision.OrgID != expect.OrgID || decision.SpaceRef != expect.SpaceRef || decision.SubjectID != expect.SubjectID ||
		decision.ServiceAudience != spaceCapabilityAudience || decision.ActionID != spaceCapabilityAction ||
		decision.ActionSchemaHash != spaceCapabilitySchema {
		return VerifiedCapability{}, fmt.Errorf("Space capability decision does not match the claimed lease request")
	}
	now := expect.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}
	if !now.Before(decision.ExpiresAt) || !hasPermission(decision.Permissions, "space:sandbox:use") {
		return VerifiedCapability{}, fmt.Errorf("Space capability decision is not currently usable")
	}
	if decision.PayloadDigest != expectedCapabilityPayloadDigest(decision, claims) {
		return VerifiedCapability{}, fmt.Errorf("Space capability decision payload does not bind this claim")
	}
	if claims.Egress != "disabled_by_default" && !hasPermission(decision.Permissions, "space:egress") {
		return VerifiedCapability{}, fmt.Errorf("Space capability decision does not grant egress for the claimed profile")
	}
	return VerifiedCapability{
		Claims:                    claims,
		Permissions:               decision.Permissions,
		RecipientAudienceRevision: decision.RecipientAudienceRevision,
	}, nil
}

// verifyEnvelope checks the parts every Control Space decision shares —
// version prefix, trusted key id, and the Ed25519 signature over
// `version.keyid.payload` — and returns the decoded payload for the caller to
// interpret.
//
// Shared by the capability decision (AcquireLease) and the read decision
// (S4.2 §7's human process reads) because the envelope genuinely is one
// format. Only what the payload MEANS differs between them, so only that is
// written twice; duplicating base64url-and-ed25519 would be two chances to
// get the same thing subtly wrong.
//
// Note the RawURLEncoding: these tokens are base64URL, and a std-base64
// decoder silently fails on every token containing `-` or `_`, which disables
// the whole decision lane rather than rejecting one token loudly.
func (v *SpaceCapabilityVerifier) verifyEnvelope(token string) ([]byte, error) {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != spaceCapabilityDecisionVersion {
		return nil, fmt.Errorf("invalid Space decision envelope")
	}
	encodedKeyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(encodedKeyID) != v.keyID {
		return nil, fmt.Errorf("untrusted Space decision key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(v.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return nil, fmt.Errorf("invalid Space decision signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return nil, fmt.Errorf("invalid Space decision payload")
	}
	return payload, nil
}

func (v *SpaceCapabilityVerifier) verify(token string) (spaceCapabilityDecision, error) {
	payload, err := v.verifyEnvelope(token)
	if err != nil {
		return spaceCapabilityDecision{}, err
	}
	var decision spaceCapabilityDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return spaceCapabilityDecision{}, fmt.Errorf("invalid Space capability decision JSON")
	}
	return decision, nil
}

func hasPermission(permissions []string, required string) bool {
	for _, permission := range permissions {
		if permission == required {
			return true
		}
	}
	return false
}

// expectedCapabilityPayloadDigest mirrors Control's spaceCapabilityPayloadDigest
// (user-core/internal/spaces/space_capability_decision.go) exactly, field
// for field: the Membership/Privacy-derived values Control folded into the
// decision itself, plus the intent-only values carried in the unsigned
// claims sidecar. Length-prefixed for the same collision-resistance reason
// as every other Space decision digest in this repo.
func expectedCapabilityPayloadDigest(decision spaceCapabilityDecision, claims SpaceCapabilityClaims) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.capability_profile\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID},
		{"space_id", decision.SpaceRef}, {"backend_id", claims.BackendID},
		{"profile_digest", claims.ProfileDigest}, {"persistence", claims.Persistence},
		{"processes", claims.Processes}, {"backup", strconv.FormatBool(claims.Backup)},
		{"egress", claims.Egress}, {"credential_mode", claims.CredentialMode},
		{"recipient_audience_ref", decision.RecipientAudienceRef}, {"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef}, {"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", spaceCapabilitySchema}, {"idempotency_key", decision.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	for _, revision := range []struct {
		name  string
		value int64
	}{
		{"authority_revision", decision.AuthorityRevision},
		{"membership_revision", decision.MembershipRevision},
		{"privacy_revision", decision.PrivacyRevision},
		{"recipient_audience_revision", decision.RecipientAudienceRevision},
		{"entitlement_revision", decision.EntitlementRevision},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}
