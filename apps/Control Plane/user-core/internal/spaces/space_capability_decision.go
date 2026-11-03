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
	// spaceCapabilitySchemaV2 marks a decision signed with egress tiers (EP-9
	// E2, SPACE_SANDBOX_EGRESS_TIERS on): the payload digest also binds the
	// allow-list digest and the egress permission names a tier. A verifier that
	// knows only v1 refuses a v2 decision on the schema check instead of
	// misreading it, so a half-deployed train fails closed.
	spaceCapabilitySchemaV2 = "sha256:space-capability-profile-v2"

	// ProcessesBoundedOneshot is the substrate profile every execution-core
	// reports by default: bounded one-shot children only, nothing that
	// outlives the call that started it.
	ProcessesBoundedOneshot = "bounded_oneshot"
	// ProcessesBackgroundRegistry is what a backend reports when it can host
	// reattachable background processes (S4.2). Mirrored by
	// sandbox-manager's authz.ProcessesBackgroundRegistry and produced by
	// execution-core's sandbox.rs; all three must agree on the spelling,
	// because the value is bound into this decision's payload digest.
	ProcessesBackgroundRegistry = "background_registry"

	// EgressDisabledByDefault is the only egress claim Control signs today.
	// It is the bottom tier of the closed egress vocabulary: the sandbox has
	// no network reach unless a later, separately designed egress track adds
	// a tier here (WS06-D3; the tunnel tier is parked behind SD-06). Mirrored
	// by execution-core's sandbox.rs and capability_client.rs, and bound into
	// this decision's payload digest, so the spelling must agree everywhere.
	EgressDisabledByDefault = "disabled_by_default"
)

// allowedProcessesClaims is the closed vocabulary Control will sign.
//
// This is the first validation the `processes` claim has ever had. It was a
// free-form string checked only for non-emptiness, which meant Control would
// sign a decision for a backend claiming anything at all — and the S3.2
// design's own §1 promised a policy allowlist here that was never built.
// This closes that gap for one dimension rather than all of them: an
// unrecognized claim is refused before signing instead of being carried into
// a digest that two other services then treat as authoritative.
var allowedProcessesClaims = map[string]struct{}{
	ProcessesBoundedOneshot:     {},
	ProcessesBackgroundRegistry: {},
}

// allowedEgressClaims is the closed egress vocabulary Control will sign.
// Before this, any value other than "disabled_by_default" was accepted and
// earned the space:egress permission with no entitlement check. A value that
// is not listed here is refused before signing, which is the same
// unknown-means-disabled rule the other claim dimensions follow: Control never
// widens network reach from a string it does not recognize. Adding a tier is
// a Control decision and a lockstep change with the capability verifier, not
// a backend's to claim.
var allowedEgressClaims = map[string]struct{}{
	EgressDisabledByDefault: {},
}

// allowedTieredEgressClaims is the egress vocabulary Control accepts while
// SPACE_SANDBOX_EGRESS_TIERS is on: the legacy disabled_by_default (read as
// off) plus the closed tier enum. A claim is what the backend can enforce,
// not what it is granted; the grant is min(claim, org ceiling).
var allowedTieredEgressClaims = map[string]struct{}{
	EgressDisabledByDefault: {},
	SandboxEgressOff:        {},
	SandboxEgressRegistries: {},
	SandboxEgressAllowlist:  {},
	SandboxEgressAll:        {},
}

// SandboxEgressGrant is the egress a tiered decision signs: the tier carried
// by the space:egress:<tier> permission (off when there is none) and the
// allow-list bound into the payload digest, non-empty only at allowlist.
type SandboxEgressGrant struct {
	Tier      string   `json:"tier"`
	Allowlist []string `json:"allowlist"`
}

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
	return i.ValidateWithEgressTiers(false)
}

// ValidateWithEgressTiers validates the intent against the egress vocabulary
// of the SPACE_SANDBOX_EGRESS_TIERS switch: off keeps the single legacy
// claim, on accepts the tier enum as well.
func (i SpaceCapabilityIntent) ValidateWithEgressTiers(tiered bool) error {
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
	if _, ok := allowedProcessesClaims[i.Processes]; !ok {
		return fmt.Errorf("space capability intent processes claim %q is not a recognized substrate profile", i.Processes)
	}
	egressClaims := allowedEgressClaims
	if tiered {
		egressClaims = allowedTieredEgressClaims
	}
	if _, ok := egressClaims[i.Egress]; !ok {
		return fmt.Errorf("space capability intent egress claim %q is not a recognized egress tier", i.Egress)
	}
	return nil
}

// claimedEgressTier reads a validated egress claim as a tier: the legacy
// disabled_by_default is off.
func (i SpaceCapabilityIntent) claimedEgressTier() string {
	if i.Egress == EgressDisabledByDefault {
		return SandboxEgressOff
	}
	return i.Egress
}

// spaceCapabilityPayloadDigest commits a decision to precisely one measured
// substrate claim on precisely one backend. Length-prefixed for the same
// collision-resistance reason as the schedule-fire and thread-append
// contracts: a naive concatenation of variable-length fields is ambiguous
// (e.g. "ab"+"c" collides with "a"+"bc").
func spaceCapabilityPayloadDigest(evidence PersonalThreadDecisionEvidence, intent SpaceCapabilityIntent) string {
	return spaceCapabilityPayloadDigestFor(evidence, intent, spaceCapabilitySchema, nil)
}

// spaceCapabilityPayloadDigestFor computes the digest for one schema. A v2
// decision passes the allow-list digest, appended as one more length-prefixed
// field after the v1 fields; v1 passes nil and its digest is unchanged.
func spaceCapabilityPayloadDigestFor(evidence PersonalThreadDecisionEvidence, intent SpaceCapabilityIntent, schema string, egressAllowlistDigest *string) string {
	fields := []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"backend_id", intent.BackendID},
		{"profile_digest", intent.ProfileDigest}, {"persistence", intent.Persistence},
		{"processes", intent.Processes}, {"backup", strconv.FormatBool(intent.Backup)},
		{"egress", intent.Egress}, {"credential_mode", intent.CredentialMode},
		{"recipient_audience_ref", evidence.RecipientAudienceRef}, {"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef}, {"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", schema}, {"idempotency_key", intent.IdempotencyKey},
	}
	if egressAllowlistDigest != nil {
		fields = append(fields, struct{ name, value string }{"egress_allowlist_digest", *egressAllowlistDigest})
	}
	hash := sha256.New()
	hash.Write([]byte("model.space.capability_profile\x00v1\x00"))
	for _, field := range fields {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
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
// with — arguably the most privacy-preserving option under — ZDR. It does
// withhold space:processes, because a background process registry persists
// output and state past the call that started it.
func IssueSpaceCapabilityDecision(
	evidence PersonalThreadDecisionEvidence,
	intent SpaceCapabilityIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := checkSpaceCapabilityAuthority(evidence, intent, false, decisionRef, nonce, now); err != nil {
		return Decision{}, err
	}
	permissions := []string{"space:sandbox:use"}
	// Intent.Validate has already refused every egress claim outside
	// allowedEgressClaims, so this branch is unreachable today. It stays so
	// that a future tier added to the vocabulary cannot be signed without
	// the permission that the verifier checks for.
	if intent.Egress != EgressDisabledByDefault {
		permissions = append(permissions, "space:egress")
	}
	// Deliberately NOT symmetric with egress above. An egress-capable claim
	// without the entitlement is refused outright by the verifier, because a
	// backend that can reach the network must not serve a Space that was
	// never allowed network reach. A process-capable backend is different:
	// it is still a perfectly good bounded one-shot substrate for a Space
	// that is not entitled to background work, so the lease is granted and
	// only the permission is withheld. sandbox-manager then records the
	// absence on the lease row and refuses the process RPCs alone. A Space
	// under zero data retention is treated like an unentitled one: the
	// process registry persists output beyond the call, so the permission is
	// withheld there as well, and the lease itself is still granted.
	if processesPermissionGranted(evidence, intent) {
		permissions = append(permissions, "space:processes")
	}
	return newEvidenceDecision(
		evidence, decisionRef, spaceCapabilityAudience, spaceCapabilityAction, spaceCapabilitySchema,
		spaceCapabilityPayloadDigest(evidence, intent), intent.IdempotencyKey, nonce,
		permissions, evidence.Privacy.ZeroDataRetention, now,
	), nil
}

// IssueTieredSpaceCapabilityDecision is IssueSpaceCapabilityDecision with
// egress tiers (EP-9 E2), used only while SPACE_SANDBOX_EGRESS_TIERS is on.
// The org ceiling is the Space egress entitlement: Control signs
// space:egress:<tier> for min(claim, ceiling), and no egress permission at
// all under zero data retention, whatever the claim and the ceiling say.
// The bare space:egress permission is never signed here: the v2 schema makes
// a v1 verifier refuse the decision before it reads permissions, so the bare
// permission would have no reader.
//
// The lease rule for a claim the grant does not cover is unchanged and lives
// in sandbox-manager: an egress-capable claim with no egress permission is
// refused there until decision D-E2 (grant the lease at off) lands with E3.
func IssueTieredSpaceCapabilityDecision(
	evidence PersonalThreadDecisionEvidence,
	intent SpaceCapabilityIntent,
	ceiling SandboxEgressPolicy,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, SandboxEgressGrant, error) {
	if err := checkSpaceCapabilityAuthority(evidence, intent, true, decisionRef, nonce, now); err != nil {
		return Decision{}, SandboxEgressGrant{}, err
	}
	grant := grantedSandboxEgress(intent.claimedEgressTier(), ceiling, evidence.Privacy.ZeroDataRetention)
	permissions := []string{"space:sandbox:use"}
	if grant.Tier != SandboxEgressOff {
		permissions = append(permissions, "space:egress:"+grant.Tier)
	}
	if processesPermissionGranted(evidence, intent) {
		permissions = append(permissions, "space:processes")
	}
	allowlistDigest := EgressAllowlistDigest(grant.Allowlist)
	return newEvidenceDecision(
		evidence, decisionRef, spaceCapabilityAudience, spaceCapabilityAction, spaceCapabilitySchemaV2,
		spaceCapabilityPayloadDigestFor(evidence, intent, spaceCapabilitySchemaV2, &allowlistDigest),
		intent.IdempotencyKey, nonce, permissions, evidence.Privacy.ZeroDataRetention, now,
	), grant, nil
}

// checkSpaceCapabilityAuthority is the authority floor both decision forms
// share: personal authority, an editor-or-above role, the standing sandbox
// entitlement, a valid intent that names the current membership, and the
// decision material.
func checkSpaceCapabilityAuthority(evidence PersonalThreadDecisionEvidence, intent SpaceCapabilityIntent, tiered bool, decisionRef, nonce string, now time.Time) error {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot acquire a sandbox capability", evidence.Membership.Role)
	}
	if !evidence.SandboxCapabilityEntitled {
		return fmt.Errorf("sandbox capability entitlement is not active")
	}
	if err := intent.ValidateWithEgressTiers(tiered); err != nil {
		return err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return fmt.Errorf("space capability intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return fmt.Errorf("space capability decision fields are required")
	}
	return nil
}

// processesPermissionGranted is the space:processes rule both decision forms
// share; see the comment at its use in IssueSpaceCapabilityDecision.
func processesPermissionGranted(evidence PersonalThreadDecisionEvidence, intent SpaceCapabilityIntent) bool {
	return intent.Processes == ProcessesBackgroundRegistry && evidence.ProcessRegistryEntitled && !evidence.Privacy.ZeroDataRetention
}

// grantedSandboxEgress computes the signed egress: off under zero data
// retention, otherwise the lower of the claimed tier and the org ceiling. A
// ceiling outside the vocabulary counts as off (unknown means off). An
// allowlist grant with an empty list reaches nothing, so it is signed as off
// rather than as a permission with nothing behind it.
func grantedSandboxEgress(claimTier string, ceiling SandboxEgressPolicy, zeroDataRetention bool) SandboxEgressGrant {
	off := SandboxEgressGrant{Tier: SandboxEgressOff, Allowlist: []string{}}
	if zeroDataRetention {
		return off
	}
	claimRank, claimKnown := sandboxEgressRank[claimTier]
	ceilingRank, ceilingKnown := sandboxEgressRank[ceiling.Tier]
	if !claimKnown || !ceilingKnown {
		return off
	}
	tier := claimTier
	if ceilingRank < claimRank {
		tier = ceiling.Tier
	}
	switch tier {
	case SandboxEgressRegistries:
		return SandboxEgressGrant{Tier: tier, Allowlist: []string{}}
	case SandboxEgressAllowlist:
		if len(ceiling.Allowlist) == 0 {
			return off
		}
		return SandboxEgressGrant{Tier: tier, Allowlist: append([]string(nil), ceiling.Allowlist...)}
	default:
		// off, or all, which a stored ceiling can never be (migration 044's
		// CHECK and SandboxEgressPolicy.Normalize both refuse it).
		return off
	}
}

// EgressAllowlistDigest commits a decision to one allow-list, in the order
// signed (the stored list is already sorted and de-duplicated). Each host is
// length-prefixed like the payload digest fields, so ["ab","c"] and
// ["a","bc"] differ. Mirrored by sandbox-manager's egressAllowlistDigest.
func EgressAllowlistDigest(hosts []string) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.capability_profile.egress_allowlist\x00v1\x00"))
	for _, host := range hosts {
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(host)))
		hash.Write(length[:])
		hash.Write([]byte(host))
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}
