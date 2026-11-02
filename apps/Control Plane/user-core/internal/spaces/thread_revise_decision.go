package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// WS12-S5: the Control decision that authorizes revising (editing or deleting)
// one stored message of a Space thread.
//
// Session Core's ReviseMessage verifies the token with its own copy of the
// digest below (apps/Model Plane/rust/crates/mp-contracts/src/message_revision.rs
// documents the byte layout; the two copies are pinned by a shared golden).
// Control signs WHAT the caller may do, never who authored the target: Session
// Core decides the actor class (author, agent owner, moderator) from the stored
// row and refuses anything the permissions do not cover.
//
// Permissions, all derived here from current authority:
//   - thread:revise     always (the org's message_revise_entitled bit is on)
//   - thread:moderate   only for a manager or owner of a non-personal,
//                       non-DM Space whose org has message_moderate_entitled
//   - exactly one of revise:purge_original / revise:retain_original, from the
//     org's space_message_policies row (absent means purge)
//
// A revise decision is refused under Zero Data Retention (it rewrites stored
// content): ZDR fails closed with the typed zdr_forbids_effect refusal.
const (
	threadReviseAction   = "model.thread.revise"
	threadReviseSchema   = "sha256:thread-revise-v1"
	threadReviseAudience = "model-plane"

	// ThreadRevisePermission is carried by every revise decision.
	ThreadRevisePermission = "thread:revise"
	// ThreadModeratePermission lets a manager or owner delete another member's
	// message. It is never issued in a DM kind or a personal Space.
	ThreadModeratePermission = "thread:moderate"
	// RevisePurgeOriginalPermission and ReviseRetainOriginalPermission are the
	// two forms of the org's original-text rule; exactly one rides a decision.
	RevisePurgeOriginalPermission  = "revise:purge_original"
	ReviseRetainOriginalPermission = "revise:retain_original"

	// ThreadReviseActionEdit and ThreadReviseActionDelete are the closed set
	// of revise actions (the Session Core ReviseMessageRequest.action values).
	ThreadReviseActionEdit   = "edit"
	ThreadReviseActionDelete = "delete"

	// OriginalRetentionRetain keeps the pre-edit text in the revision log;
	// OriginalRetentionPurge blanks it. Absence of a policy row is purge.
	OriginalRetentionRetain = "retain"
	OriginalRetentionPurge  = "purge"

	// emptyContentDigest is the SHA-256 commitment of the empty string, which a
	// delete binds in place of new text.
	emptyContentDigest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
)

// ValidOriginalRetention reports whether v is one of the two closed rules.
func ValidOriginalRetention(v string) bool {
	return v == OriginalRetentionRetain || v == OriginalRetentionPurge
}

// ThreadReviseDecisionRequest carries the operation-bound fields of one
// revision. It holds no content: ContentDigest is the SHA-256 commitment to the
// new text (edit) or to the empty string (delete), which Session Core
// recomputes before it writes anything.
type ThreadReviseDecisionRequest struct {
	DecisionRef    string
	ThreadID       string
	MessageID      string
	Action         string
	ContentDigest  string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only.
func (r ThreadReviseDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "thread_id": r.ThreadID, "message_id": r.MessageID,
		"action": r.Action, "content_digest": r.ContentDigest,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("thread revise decision %s is required", label)
		}
	}
	switch r.Action {
	case ThreadReviseActionEdit, ThreadReviseActionDelete:
	default:
		return fmt.Errorf("thread revise action must be edit or delete")
	}
	if !strings.HasPrefix(r.ContentDigest, "sha256:") || len(r.ContentDigest) != len("sha256:")+64 {
		return fmt.Errorf("thread revise content digest is invalid")
	}
	if r.Action == ThreadReviseActionDelete && r.ContentDigest != emptyContentDigest {
		return fmt.Errorf("thread revise delete must commit to the empty content digest")
	}
	return nil
}

// validateThreadReviseCommon holds the checks a personal and a shared revise
// share: entitlement, resource reference, a recognized retention rule, privacy
// and Zero Data Retention. The role floor is editor in both: a downgraded
// viewer cannot revise.
func (e PersonalThreadDecisionEvidence) validateThreadReviseCommon() error {
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot revise a message", e.Membership.Role), "editor", "manager", "owner")
	}
	if !e.MessageReviseEntitled {
		return entitlementOffDenial("message_revise", "message revision entitlement is not active")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("message revision authority is incomplete")
	}
	if !ValidOriginalRetention(e.OriginalRetention) {
		return authorityIncompleteDenial("message original-text rule is unresolved")
	}
	if err := e.Privacy.Validate(); err != nil {
		return err
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("message_revise", "message revision is not available under Zero Data Retention")
	}
	return nil
}

// ValidateForPersonalThreadRevise authorizes revising a message inside a
// personal Space: the sole member, editor floor and above.
func (e PersonalThreadDecisionEvidence) ValidateForPersonalThreadRevise() error {
	if err := e.validatePersonalAuthority(); err != nil {
		return err
	}
	return e.validateThreadReviseCommon()
}

// ValidateForSharedThreadRevise authorizes revising a message inside a shared
// Space (room, project, case, DM): an active member of the current recipient
// audience, editor floor and above.
func (e PersonalThreadDecisionEvidence) ValidateForSharedThreadRevise() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(e.Membership.Kind), "shared message revision requires a non-personal Space")
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	return e.validateThreadReviseCommon()
}

// threadRevisePermissions derives the permission set from validated evidence.
func threadRevisePermissions(e PersonalThreadDecisionEvidence) []string {
	permissions := []string{ThreadRevisePermission}
	if e.MessageModerateEntitled &&
		e.Membership.Kind != KindPersonal && !e.Membership.Kind.IsDM() &&
		matchesOneOf(e.Membership.Role, "manager", "owner") {
		permissions = append(permissions, ThreadModeratePermission)
	}
	if e.OriginalRetention == OriginalRetentionRetain {
		return append(permissions, ReviseRetainOriginalPermission)
	}
	return append(permissions, RevisePurgeOriginalPermission)
}

// ThreadRevisePayloadDigest is the exact effect Session Core verifies. It is
// the thread-append v1 field set with message_id and revise_action added after
// idempotency_key, then the two revisions; every field is length-prefixed.
func ThreadRevisePayloadDigest(evidence PersonalThreadDecisionEvidence, request ThreadReviseDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.thread.revise\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"thread_id", request.ThreadID}, {"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"content_digest", request.ContentDigest},
		{"action_schema_hash", threadReviseSchema}, {"idempotency_key", request.IdempotencyKey},
		{"message_id", request.MessageID}, {"revise_action", request.Action},
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

// IssueThreadReviseDecision issues the revise decision. A refusal is typed
// (role floor, entitlement off, ZDR, audience); an invalid request is untyped.
func IssueThreadReviseDecision(evidence PersonalThreadDecisionEvidence, request ThreadReviseDecisionRequest, now time.Time) (Decision, error) {
	if evidence.Membership.Kind == KindPersonal {
		if err := evidence.ValidateForPersonalThreadRevise(); err != nil {
			return Decision{}, err
		}
	} else if err := evidence.ValidateForSharedThreadRevise(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("thread revise decision request is invalid")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, threadReviseAudience, threadReviseAction, threadReviseSchema,
		ThreadRevisePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		threadRevisePermissions(evidence), evidence.Privacy.ZeroDataRetention, now,
	), nil
}
