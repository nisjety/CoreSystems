package spaces

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS17-S7: the Control decision that lets one current editor, manager or owner of a shared Space
// share a personal memory into that room (operation `declassify`), or retract the room copy they
// shared (operation `retract`, SD-22: the declassifying owner retracts; room managers do not).
//
// Both operations use one action, schema and permission. The payload digest binds the operation,
// so a declassify token never retracts and a retract token never shares. Session Core verifies the
// digest with its own copy (memory_declassify_payload_digest in session-core grpc.rs) and recomputes
// the content digest from the reviewed text it receives, so the text cannot be swapped after review.
const (
	memoryDeclassifyAction   = "model.memory.declassify"
	memoryDeclassifySchema   = "sha256:memory-declassify-v1"
	memoryDeclassifyAudience = "model-plane"

	// MemoryDeclassifyPermission is the only permission either operation carries.
	MemoryDeclassifyPermission = "memory:declassify"

	// MemoryDeclassifyOperationDeclassify shares one personal memory row into the room. Its memory id
	// is the source row (scope user) and it binds the digest of the reviewed text.
	MemoryDeclassifyOperationDeclassify = "declassify"
	// MemoryDeclassifyOperationRetract removes the room copy this caller shared. Its memory id is the
	// room row and it binds no content digest (Session Core checks the ledger subject).
	MemoryDeclassifyOperationRetract = "retract"
)

// memoryContentDigestPattern is the shape of a content digest: "sha256:" then 64 lowercase hex digits.
var memoryContentDigestPattern = regexp.MustCompile(`^sha256:[0-9a-f]{64}$`)

// MemoryDeclassifyDecisionRequest carries the operation-bound fields of one declassify or retract. It
// holds no content: the digest of the reviewed text is the whole content binding.
type MemoryDeclassifyDecisionRequest struct {
	DecisionRef    string
	Operation      string
	MemoryID       string
	ContentDigest  string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only. The operation set is closed. The memory id is bound byte for byte (not
// trimmed), so a surrounding space is refused rather than silently changed.
func (r MemoryDeclassifyDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "operation": r.Operation, "memory_id": r.MemoryID,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("memory declassify decision %s is required", label)
		}
	}
	if r.MemoryID != strings.TrimSpace(r.MemoryID) {
		return fmt.Errorf("memory declassify decision memory_id must not have surrounding whitespace")
	}
	if len(r.MemoryID) > MaxSpaceMemoryIDLen {
		return fmt.Errorf("memory declassify decision memory_id is too long")
	}
	switch r.Operation {
	case MemoryDeclassifyOperationDeclassify:
		if !memoryContentDigestPattern.MatchString(r.ContentDigest) {
			return fmt.Errorf("memory declassify decision content_digest must be sha256 with 64 lowercase hex digits")
		}
	case MemoryDeclassifyOperationRetract:
		if r.ContentDigest != "" {
			return fmt.Errorf("memory declassify decision content_digest must be empty for %s", MemoryDeclassifyOperationRetract)
		}
	default:
		return fmt.Errorf("memory declassify decision operation must be %s or %s",
			MemoryDeclassifyOperationDeclassify, MemoryDeclassifyOperationRetract)
	}
	return nil
}

// ValidateForMemoryDeclassify authorizes sharing a personal memory into a shared Space: the caller is
// a current editor, manager or owner and a current recipient, the org's memory_declassify_entitled bit
// is on, and the Space is not under Zero Data Retention (a durable copy is refused under ZDR). The
// personal-Space case is refused: a personal Space has one member and nothing to share into.
func (e PersonalThreadDecisionEvidence) ValidateForMemoryDeclassify() error {
	if err := e.validateSharedMemoryBase(); err != nil {
		return err
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("memory_declassify", "sharing memory into a Space is not available under Zero Data Retention")
	}
	if !e.MemoryDeclassifyEntitled {
		return entitlementOffDenial("memory_declassify", "shared Space memory declassify authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateForMemoryRetract authorizes removing the room copy one editor, manager or owner shared.
// Removing is never storing, so it needs no entitlement bit and is not refused under ZDR: turning the
// bit off or the Space to ZDR must not strand a copy the owner cannot take back.
func (e PersonalThreadDecisionEvidence) ValidateForMemoryRetract() error {
	if err := e.validateSharedMemoryBase(); err != nil {
		return err
	}
	return e.Privacy.Validate()
}

// validateSharedMemoryBase is the actor and audience rule both memory operations share: a non-personal
// Space, the editor floor, a current recipient audience and a resource authority reference.
func (e PersonalThreadDecisionEvidence) validateSharedMemoryBase() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(e.Membership.Kind), "memory declassify requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot share memory into a Space", e.Membership.Role), "editor", "manager", "owner")
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("shared Space memory declassify authority is incomplete")
	}
	return nil
}

// ResolveSharedMemoryDeclassifyDecisionEvidence resolves the current membership and current recipient
// audience of a shared Space, and reads memory_declassify_entitled into the evidence. It mirrors
// ResolveSharedRunControlDecisionEvidence and does not validate: the issuer picks the operation's rule,
// because retraction must not depend on the entitlement bit that declassifying does.
func (r *Repository) ResolveSharedMemoryDeclassifyDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.memory_declassify_entitled
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
	).Scan(sharedDecisionEvidenceScanDestinations(&evidence, &evidence.MemoryDeclassifyEntitled)...)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space memory declassify authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:memory-declassify:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	return evidence, nil
}

// memoryDeclassifyPayloadDigest binds the exact effect Session Core verifies, byte for byte as
// memory_declassify_payload_digest recomputes it: the prefix, then each (name, value) as name, a NUL, a
// u64 big-endian length and the value, in this order, then the authority and recipient-audience
// revisions. Every field is length-prefixed so concatenation stays unambiguous.
func memoryDeclassifyPayloadDigest(evidence PersonalThreadDecisionEvidence, request MemoryDeclassifyDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.memory.declassify\x00v1\x00"))
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
		{"content_digest", request.ContentDigest},
		{"action_schema_hash", memoryDeclassifySchema},
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

// IssueMemoryDeclassifyDecision authorizes one declassify or one retract by one current editor,
// manager or owner. The evidence comes from ResolveSharedMemoryDeclassifyDecisionEvidence. The decision
// carries the Space ZDR claim; a declassify is refused before issuance under ZDR, a retract is not.
func IssueMemoryDeclassifyDecision(evidence PersonalThreadDecisionEvidence, request MemoryDeclassifyDecisionRequest, now time.Time) (Decision, error) {
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	var err error
	switch request.Operation {
	case MemoryDeclassifyOperationDeclassify:
		err = evidence.ValidateForMemoryDeclassify()
	default:
		err = evidence.ValidateForMemoryRetract()
	}
	if err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("memory declassify decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, memoryDeclassifyAudience, memoryDeclassifyAction, memoryDeclassifySchema,
		memoryDeclassifyPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{MemoryDeclassifyPermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
