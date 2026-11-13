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

// EP-8 S7: the Control decision that lets a Space's owner or admin list, or
// delete one row of, the memory Model Plane keeps for that Space.
//
// It is NOT a thread read, thread write, run-control or approval decision. The
// action, schema, permission and payload digest all differ from those, and the
// digest binds the operation and the memory id, so a list token never deletes
// and one delete token never deletes another row. Session Core verifies it with
// its own copy of the digest below (space_memory_payload_digest in
// session-core grpc.rs) and must refuse, both ways, a thread, run-control, wake
// or approval token carrying memory:manage.
//
// Space roles are viewer < editor < manager < owner. "Admin" in the plan means
// the elevated roles: manager and owner. A member below that reads Space memory
// only through recall, never through this decision.
const (
	spaceMemoryAction   = "model.space.memory.manage"
	spaceMemorySchema   = "sha256:space-memory-manage-v1"
	spaceMemoryAudience = "model-plane"

	// SpaceMemoryPermission is the only permission a Space memory decision
	// carries. It is disjoint from the thread, run-control, wake and approval
	// permissions.
	SpaceMemoryPermission = "memory:manage"

	// SpaceMemoryOperationList lists the Space's memory rows. Its memory id is empty.
	SpaceMemoryOperationList = "list"
	// SpaceMemoryOperationDelete deletes one Space memory row, named by memory id.
	SpaceMemoryOperationDelete = "delete"
	// SpaceMemoryOperationEraseAgent erases one named agent's memory in the
	// Space (EP-6 P3, EP-8 S10). Its memory id is the agent's subject id, so one
	// token erases one agent's rows and no other agent's.
	SpaceMemoryOperationEraseAgent = "erase_agent"

	// MaxSpaceMemoryIDLen is the longest memory id (or agent subject id) in bytes.
	MaxSpaceMemoryIDLen = 200
)

// SpaceMemoryDecisionRequest carries the operation-bound fields of one Space
// memory management call. It holds no memory content: the operation and the id
// are the whole effect.
type SpaceMemoryDecisionRequest struct {
	DecisionRef    string
	Operation      string
	MemoryID       string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only. The operation set is closed: a caller cannot
// obtain a signed claim for an operation Control does not issue. The memory id
// is bound byte for byte (it is not trimmed), because Session Core recomputes
// the digest from the id it receives.
func (r SpaceMemoryDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "operation": r.Operation,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space memory decision %s is required", label)
		}
	}
	switch r.Operation {
	case SpaceMemoryOperationList:
		if r.MemoryID != "" {
			return fmt.Errorf("space memory decision memory_id must be empty for %s", SpaceMemoryOperationList)
		}
	case SpaceMemoryOperationDelete, SpaceMemoryOperationEraseAgent:
		if strings.TrimSpace(r.MemoryID) == "" {
			return fmt.Errorf("space memory decision memory_id is required for %s", r.Operation)
		}
		if r.MemoryID != strings.TrimSpace(r.MemoryID) {
			return fmt.Errorf("space memory decision memory_id must not have surrounding whitespace")
		}
	default:
		return fmt.Errorf("space memory decision operation must be %s, %s or %s",
			SpaceMemoryOperationList, SpaceMemoryOperationDelete, SpaceMemoryOperationEraseAgent)
	}
	if len(r.MemoryID) > MaxSpaceMemoryIDLen {
		return fmt.Errorf("space memory decision memory_id is too long")
	}
	return nil
}

// ValidateForSpaceMemoryManage authorizes managing a shared Space's memory:
// the Space must be shared (a personal Space has one member and no Space
// memory), and the caller's current role must be manager or owner. There is no
// effect entitlement bit for this decision; the role floor and the current
// recipient audience are the authority.
func (e PersonalThreadDecisionEvidence) ValidateForSpaceMemoryManage() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(e.Membership.Kind), "Space memory management requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, RoleManager, RoleOwner) {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot manage Space memory", e.Membership.Role), RoleManager, RoleOwner)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("shared Space memory authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateMembershipForSpaceMemoryManage is the early check on the caller's
// current membership alone (kind and role), so a refusal is typed before any
// audience or policy row is read.
func ValidateMembershipForSpaceMemoryManage(membership *CurrentMembership) error {
	if membership == nil {
		return ErrNoCurrentMembership
	}
	if membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(membership.Kind), "Space memory management requires a non-personal Space")
	}
	if !matchesOneOf(membership.Role, RoleManager, RoleOwner) {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot manage Space memory", membership.Role), RoleManager, RoleOwner)
	}
	return nil
}

// ResolveSharedSpaceMemoryDecisionEvidence resolves the authority to list or
// delete a shared Space's memory. It copies the current-membership and
// current-recipient-audience joins of ResolveSharedRunControlDecisionEvidence,
// so a removed participant stops resolving at once, but reads no effect
// entitlement bit: none exists for this decision. The Space may be active or
// archived (removing stored memory of an archived room is data minimisation,
// not new content).
func (r *Repository) ResolveSharedSpaceMemoryDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	// The last column is a constant TRUE scanned into a discarded bool: this
	// decision has no entitlement bit, and it must not borrow another effect's.
	var noEntitlement bool
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, TRUE
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
	).Scan(sharedDecisionEvidenceScanDestinations(&evidence, &noEntitlement)...)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space memory authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:memory-manage:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSpaceMemoryManage(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid shared Space memory authority: %w", err)
	}
	return evidence, nil
}

// spaceMemoryPayloadDigest binds the exact effect Session Core verifies, byte
// for byte as session-core's space_memory_payload_digest recomputes it: the
// prefix, then each (name, value) as name + 0x00 + u64 big-endian length +
// value in this order, then the authority and recipient-audience revisions.
// The order differs from the other decisions' (operation and memory_id sit
// before action_schema_hash), so it must not be shared with them.
func spaceMemoryPayloadDigest(evidence PersonalThreadDecisionEvidence, request SpaceMemoryDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.memory.manage\x00v1\x00"))
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
		{"memory_id", request.MemoryID},
		{"action_schema_hash", spaceMemorySchema},
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

// IssueSpaceMemoryDecision authorizes one list, one row delete or one agent
// erase of a shared Space's memory by one current manager or owner. The
// evidence must come from ResolveSharedSpaceMemoryDecisionEvidence (current
// membership and current recipient audience). The decision carries the Space
// ZDR claim; whether a ZDR Space may be managed this way is Session Core's call
// (it refuses Space memory on a ZDR or waived turn).
func IssueSpaceMemoryDecision(evidence PersonalThreadDecisionEvidence, request SpaceMemoryDecisionRequest, now time.Time) (Decision, error) {
	if err := evidence.ValidateForSpaceMemoryManage(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("space memory decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, spaceMemoryAudience, spaceMemoryAction, spaceMemorySchema,
		spaceMemoryPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{SpaceMemoryPermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
