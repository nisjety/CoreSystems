package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strconv"
	"strings"
	"time"
)

const (
	spaceCapabilityAction   = "model.space.capability_profile"
	spaceCapabilityAudience = "model-plane-sandbox-manager"
	spaceCapabilitySchema   = "sha256:space-capability-profile-v1"
)

// SpaceCapabilityIntent is the non-secret, measured sandbox substrate claim a
// sandbox-manager backend presents to Control immediately before requesting a
// lease. BackendID identifies the specific instance the resulting decision is
// pinned to; a lease request landing on a different instance must fail
// closed rather than be silently served. See
// apps/Frontend Plane/verevonv3/docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md.
type SpaceCapabilityIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	BackendID      string
	ProfileDigest  string
	Persistence    string
	Processes      string
	Backup         bool
	Egress         string
	CredentialMode string
	IdempotencyKey string
}

func (i SpaceCapabilityIntent) Validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"backend_id": i.BackendID, "profile_digest": i.ProfileDigest,
		"persistence": i.Persistence, "processes": i.Processes, "egress": i.Egress,
		"credential_mode": i.CredentialMode, "idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space capability intent %s is required", label)
		}
	}
	if !strings.HasPrefix(i.ProfileDigest, "sha256:") || len(i.ProfileDigest) != len("sha256:")+64 {
		return fmt.Errorf("space capability intent profile digest is invalid")
	}
	return nil
}

// spaceCapabilityPayloadDigest commits a decision to precisely one measured
// substrate claim on precisely one backend. Length-prefixed for the same
// collision-resistance reason as the schedule-fire and thread-append
// contracts: a naive concatenation of variable-length fields is ambiguous
// (e.g. "ab"+"c" collides with "a"+"bc").
func spaceCapabilityPayloadDigest(evidence PersonalThreadDecisionEvidence, intent SpaceCapabilityIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.capability_profile\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"backend_id", intent.BackendID},
		{"profile_digest", intent.ProfileDigest}, {"persistence", intent.Persistence},
		{"processes", intent.Processes}, {"backup", strconv.FormatBool(intent.Backup)},
		{"egress", intent.Egress}, {"credential_mode", intent.CredentialMode},
		{"recipient_audience_ref", evidence.RecipientAudienceRef}, {"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef}, {"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", spaceCapabilitySchema}, {"idempotency_key", intent.IdempotencyKey},
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
		{"authority_revision", evidence.Membership.Revisions.Authority},
		{"membership_revision", evidence.Membership.Revisions.Membership},
		{"privacy_revision", evidence.Membership.Revisions.Privacy},
		{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueSpaceCapabilityDecision binds one sandbox-manager backend's measured,
// self-reported substrate claim to a Space's current authority. It attests
// that the claim is authorized for use by this Space — never that the
// hardware claim itself is true; execution-core's own measurement remains
// the source of truth for what the backend can actually provide. A fresh
// decision is required per capability claim: a Space's standing sandbox
// entitlement does not let a backend silently upgrade what it claims to
// offer without Control re-checking it.
//
// Unlike schedule fire, zero data retention does not disqualify this
// decision: ephemeral, credential-free scratch execution is compatible
// with — arguably the most privacy-preserving option under — ZDR.
func IssueSpaceCapabilityDecision(
	evidence PersonalThreadDecisionEvidence,
	intent SpaceCapabilityIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("Space role %q cannot acquire a sandbox capability", evidence.Membership.Role)
	}
	if !evidence.SandboxCapabilityEntitled {
		return Decision{}, fmt.Errorf("sandbox capability entitlement is not active")
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("space capability intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("space capability decision fields are required")
	}
	permissions := []string{"space:sandbox:use"}
	if intent.Egress != "disabled_by_default" {
		permissions = append(permissions, "space:egress")
	}
	return Decision{
		DecisionRef: strings.TrimSpace(decisionRef), OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef,
		SubjectID: evidence.Membership.SubjectID, ServiceAudience: spaceCapabilityAudience,
		ActionID: spaceCapabilityAction, ActionSchemaHash: spaceCapabilitySchema,
		PayloadDigest: spaceCapabilityPayloadDigest(evidence, intent), IdempotencyKey: strings.TrimSpace(intent.IdempotencyKey),
		RecipientAudienceRef: strings.TrimSpace(evidence.RecipientAudienceRef), RecipientAudienceHash: strings.TrimSpace(evidence.RecipientAudienceHash),
		PrivacyPolicyRef: strings.TrimSpace(evidence.Privacy.PolicyRef), ResourceAuthorizationRef: strings.TrimSpace(evidence.ResourceAuthorizationRef),
		AuthorityRevision: evidence.Membership.Revisions.Authority, MembershipRevision: evidence.Membership.Revisions.Membership,
		PrivacyRevision: evidence.Membership.Revisions.Privacy, RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		EntitlementRevision: evidence.Membership.Revisions.Entitlement, Permissions: permissions,
		Purpose: strings.TrimSpace(evidence.Privacy.Purpose), LawfulBasis: strings.TrimSpace(evidence.Privacy.LawfulBasis),
		PrivacyClass: strings.TrimSpace(evidence.Privacy.PrivacyClass), ThirdPartyAllowed: evidence.Privacy.ThirdPartyAllowed,
		RetentionClass: strings.TrimSpace(evidence.Privacy.RetentionClass), Residency: strings.TrimSpace(evidence.Privacy.Residency),
		DeletionScope: strings.TrimSpace(evidence.Privacy.DeletionScope), ZeroDataRetention: evidence.Privacy.ZeroDataRetention,
		IssuedAt: now.UTC(), ExpiresAt: now.UTC().Add(personalDecisionLifetime), Nonce: strings.TrimSpace(nonce),
	}, nil
}
