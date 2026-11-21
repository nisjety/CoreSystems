package spaces

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// F-f10-18 (owner D-f10-10): the Control decision that lets a shared Space's
// manager or owner create, change or delete one Space skill (an agent_skills
// row of scope "space"). capability-core refuses every such write with
// 403 space_skill_decision_required until it can verify this decision
// (F-f10-19, SKILL_SPACE_WRITE_VERIFY); the two switches are enabled together.
//
// It is NOT a thread, memory, run-control or approval decision. The action,
// schema, permission and payload digest all differ, and the digest binds the
// operation, the skill id and the digest of the body the caller will send, so
// a create token never changes a skill, one update token never updates another
// skill, and a token for one body cannot be replayed for another.
//
// Role (D-f10-10): the same floor as model.space.memory.manage, manager or
// owner of a shared Space. No role table is invented here: the check reuses
// the memory decision's floor and the Space authority resolver.
//
// Content: the decision and its audit row carry ids and digests only. The
// skill text never reaches Control; the caller hashes the canonical body
// itself and sends the digest.
const (
	spaceSkillWriteAction   = "model.space.skill.write"
	spaceSkillWriteSchema   = "sha256:space-skill-write-v1"
	spaceSkillWriteAudience = "model-plane-capability-core"

	// SpaceSkillWritePermission is the only permission a Space skill write
	// decision carries. It is disjoint from every other decision's permissions.
	SpaceSkillWritePermission = "skill:write"

	// SpaceSkillOperationCreate creates a Space skill. Its skill id is empty:
	// capability-core mints the id.
	SpaceSkillOperationCreate = "create"
	// SpaceSkillOperationUpdate changes one Space skill, named by skill id.
	SpaceSkillOperationUpdate = "update"
	// SpaceSkillOperationDelete deletes one Space skill, named by skill id.
	SpaceSkillOperationDelete = "delete"

	// MaxSpaceSkillIDLen is the longest skill id in bytes.
	MaxSpaceSkillIDLen = 200

	// SpaceSkillWriteDecisionLifetime is how long the decision is valid. It is
	// shorter than the 2 minutes the other Space decisions live: one write
	// follows the issuance at once.
	SpaceSkillWriteDecisionLifetime = 60 * time.Second

	spaceSkillRequestDigestPrefix = "sha256:"
)

// SpaceSkillWriteDecisionRequest carries the operation-bound fields of one
// Space skill write. It holds no skill content: the operation, the skill id and
// the digest of the body are the whole intent.
type SpaceSkillWriteDecisionRequest struct {
	DecisionRef string
	Operation   string
	// SkillID is empty for a create and required for an update or a delete.
	SkillID string
	// RequestDigest is "sha256:" + lowercase hex of the canonical body the
	// caller will send (for a delete, of the empty body).
	RequestDigest  string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only. The operation set is closed: a caller cannot
// obtain a signed claim for an operation Control does not issue. The skill id
// and the request digest are bound byte for byte (they are not trimmed),
// because capability-core recomputes the payload digest from what it receives.
func (r SpaceSkillWriteDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "operation": r.Operation,
		"request_digest": r.RequestDigest, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space skill write decision %s is required", label)
		}
	}
	switch r.Operation {
	case SpaceSkillOperationCreate:
		if r.SkillID != "" {
			return fmt.Errorf("space skill write decision skill_id must be empty for %s", SpaceSkillOperationCreate)
		}
	case SpaceSkillOperationUpdate, SpaceSkillOperationDelete:
		if strings.TrimSpace(r.SkillID) == "" {
			return fmt.Errorf("space skill write decision skill_id is required for %s", r.Operation)
		}
		if r.SkillID != strings.TrimSpace(r.SkillID) {
			return fmt.Errorf("space skill write decision skill_id must not have surrounding whitespace")
		}
	default:
		return fmt.Errorf("space skill write decision operation must be %s, %s or %s",
			SpaceSkillOperationCreate, SpaceSkillOperationUpdate, SpaceSkillOperationDelete)
	}
	if len(r.SkillID) > MaxSpaceSkillIDLen {
		return fmt.Errorf("space skill write decision skill_id is too long")
	}
	if !validSpaceSkillRequestDigest(r.RequestDigest) {
		return fmt.Errorf("space skill write decision request_digest must be sha256: followed by 64 lowercase hex characters")
	}
	return nil
}

func validSpaceSkillRequestDigest(value string) bool {
	hexPart, ok := strings.CutPrefix(value, spaceSkillRequestDigestPrefix)
	if !ok || len(hexPart) != sha256.Size*2 {
		return false
	}
	for _, r := range hexPart {
		if (r < '0' || r > '9') && (r < 'a' || r > 'f') {
			return false
		}
	}
	return true
}

// SpaceSkillWriteEvidence is the shared-Space authority evidence plus the two
// facts only this decision checks in its own code: the Space registration state
// and the member's subject type. They are read by the same query as the rest.
type SpaceSkillWriteEvidence struct {
	PersonalThreadDecisionEvidence
	// State is the Space registration state; only "active" may be written to.
	State string
	// SubjectType is the member row's subject type; only "user" may author.
	SubjectType string
}

// ValidateForSpaceSkillWrite authorizes writing a shared Space's skill: the
// Space must be active and shared, the subject a user (never a service), and the
// caller's current role manager or owner, the floor of model.space.memory.manage
// (D-f10-10). There is no effect entitlement bit for this decision; the role
// floor and the current recipient audience are the authority.
func (e SpaceSkillWriteEvidence) ValidateForSpaceSkillWrite() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if strings.TrimSpace(e.SubjectType) != "user" {
		// A service principal, or anything not proven to be a user, never signs
		// as an author. Indistinguishable from a non-member on purpose.
		return newDenial(DenialMembershipRequired, "current Space membership required")
	}
	if e.State != "active" {
		denial := newDenial(DenialNotActive, "Space is not active")
		denial.State = e.State
		return denial
	}
	if err := ValidateMembershipForSpaceSkillWrite(&e.Membership); err != nil {
		return err
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("shared Space skill authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateMembershipForSpaceSkillWrite is the early check on the caller's
// current membership alone (kind and role), so a refusal is typed before any
// audience or policy row is read. The floor is the memory-manage floor.
func ValidateMembershipForSpaceSkillWrite(membership *CurrentMembership) error {
	if membership == nil {
		return ErrNoCurrentMembership
	}
	if membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(membership.Kind), "Space skill writes require a non-personal Space")
	}
	if !matchesOneOf(membership.Role, RoleManager, RoleOwner) {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot write Space skills", membership.Role), RoleManager, RoleOwner)
	}
	return nil
}

// ResolveSharedSpaceSkillWriteDecisionEvidence resolves the authority to write
// a shared Space's skill. It copies the current-membership and current-
// recipient-audience joins of ResolveSharedSpaceMemoryDecisionEvidence, so a
// removed participant stops resolving at once, and reads no effect entitlement
// bit: none exists for this decision. Unlike memory management, an archived
// Space is returned with its state so the validator refuses it (a typed
// space_not_active), because writing a skill into an archived room is new
// content, not data minimisation.
func (r *Repository) ResolveSharedSpaceSkillWriteDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (SpaceSkillWriteEvidence, error) {
	if r == nil || r.db == nil {
		return SpaceSkillWriteEvidence{}, fmt.Errorf("space authority repository unavailable")
	}
	var evidence SpaceSkillWriteEvidence
	// The entitlement column is a constant TRUE scanned into a discarded bool:
	// this decision has no entitlement bit, and it must not borrow another's.
	var noEntitlement bool
	destinations := append(sharedDecisionEvidenceScanDestinations(&evidence.PersonalThreadDecisionEvidence, &noEntitlement),
		&evidence.State, &evidence.SubjectType)
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, TRUE,
		       s.registration_state, m.subject_type
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audiences a ON a.space_ref=s.space_ref AND a.revision=r.recipient_audience_revision
		JOIN space_recipient_audience_members am ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state IN ('active','archived')
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(destinations...)
	if errors.Is(err, pgx.ErrNoRows) {
		return SpaceSkillWriteEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return SpaceSkillWriteEvidence{}, fmt.Errorf("resolve shared Space skill write authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:skill-write:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSpaceSkillWrite(); err != nil {
		return SpaceSkillWriteEvidence{}, fmt.Errorf("invalid shared Space skill write authority: %w", err)
	}
	return evidence, nil
}

// spaceSkillWritePayloadDigest binds the exact effect capability-core verifies:
// the prefix, then each (name, value) as name + 0x00 + u64 big-endian length +
// value in this order, then the authority and recipient-audience revisions.
// The intent is org_id, user_id (the subject), space_id, operation, skill_id and
// request_digest; the rest is the common decision binding. A verifier must
// recompute it from the values it receives and compare byte for byte.
func spaceSkillWritePayloadDigest(evidence SpaceSkillWriteEvidence, request SpaceSkillWriteDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.skill.write\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"operation", request.Operation},
		{"skill_id", request.SkillID},
		{"request_digest", request.RequestDigest},
		{"action_schema_hash", spaceSkillWriteSchema},
		{"idempotency_key", request.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueSpaceSkillWriteDecision authorizes one create, one update or one delete
// of a Space skill by one current manager or owner of an active shared Space.
// The evidence must come from ResolveSharedSpaceSkillWriteDecisionEvidence
// (current membership and current recipient audience). The decision lives
// SpaceSkillWriteDecisionLifetime, carries only skill:write for capability-core
// and the Space ZDR claim; whether a ZDR Space may hold a skill is the
// verifier's call.
func IssueSpaceSkillWriteDecision(evidence SpaceSkillWriteEvidence, request SpaceSkillWriteDecisionRequest, now time.Time) (Decision, error) {
	if err := evidence.ValidateForSpaceSkillWrite(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("space skill write decision issuance time is required")
	}
	decision := newEvidenceDecision(
		evidence.PersonalThreadDecisionEvidence, request.DecisionRef, spaceSkillWriteAudience, spaceSkillWriteAction, spaceSkillWriteSchema,
		spaceSkillWritePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{SpaceSkillWritePermission}, evidence.Privacy.ZeroDataRetention, now,
	)
	decision.ExpiresAt = now.UTC().Add(SpaceSkillWriteDecisionLifetime)
	return decision, nil
}
