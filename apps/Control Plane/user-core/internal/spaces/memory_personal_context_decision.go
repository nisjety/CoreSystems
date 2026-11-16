package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// WS17-S8: the Control decision that lets the current editor, manager or owner of a shared Space use their
// own personal memory for ONE turn in that room. It is per turn: a two-minute life and no standing state.
// The payload digest binds the thread, so a turn decision cannot be replayed in another thread or Space.
// Session Core verifies the digest with its own copy (verify_personal_context_decision in
// session-core space/memory_personal_context.rs) and returns only the owner's own personal rows. Org-scope
// rows are never reachable through this decision (SD-22).
const (
	memoryPersonalContextAction   = "model.memory.personal_context"
	memoryPersonalContextSchema   = "sha256:memory-personal-context-v1"
	memoryPersonalContextAudience = "model-plane"

	// MemoryPersonalContextPermission is the only permission the decision carries.
	MemoryPersonalContextPermission = "memory:personal_context"

	// maxMemoryPersonalContextThreadIDLen bounds the thread id the digest binds.
	maxMemoryPersonalContextThreadIDLen = 256
)

// MemoryPersonalContextDecisionRequest carries the fields of one per-turn decision. It holds no content: the
// thread id and the turn's idempotency key are the whole binding.
type MemoryPersonalContextDecisionRequest struct {
	DecisionRef    string
	ThreadID       string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only. The thread id is bound byte for byte (not trimmed), so a surrounding space is
// refused rather than silently changed.
func (r MemoryPersonalContextDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "thread_id": r.ThreadID,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("memory personal context decision %s is required", label)
		}
	}
	if r.ThreadID != strings.TrimSpace(r.ThreadID) {
		return fmt.Errorf("memory personal context decision thread_id must not have surrounding whitespace")
	}
	if len(r.ThreadID) > maxMemoryPersonalContextThreadIDLen {
		return fmt.Errorf("memory personal context decision thread_id is too long")
	}
	return nil
}

// ValidateForMemoryPersonalContext authorizes one turn of personal memory in a shared Space: the caller is a
// current editor, manager or owner and a current recipient; the org's memory_declassify_entitled bit is on
// (the disclosure entitlement the declassify decision uses; the plan names no separate bit for this turn);
// and the Space is not under Zero Data Retention. A personal Space has no room to speak in and is refused.
func (e PersonalThreadDecisionEvidence) ValidateForMemoryPersonalContext() error {
	if err := e.validateSharedMemoryBase(); err != nil {
		return err
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("memory_personal_context", "personal memory in a Space is not available under Zero Data Retention")
	}
	if !e.MemoryDeclassifyEntitled {
		return entitlementOffDenial("memory_declassify", "personal memory use in a Space is not entitled for this organization")
	}
	return e.Privacy.Validate()
}

// memoryPersonalContextPayloadDigest binds the exact effect Session Core verifies, byte for byte as
// verify_personal_context_decision recomputes it: the prefix, then each (name, value) as name, a NUL, a u64
// big-endian length and the value, in this order, then the authority and recipient-audience revisions.
// The refs and the idempotency key are hashed trimmed, exactly as newEvidenceDecision writes them.
func memoryPersonalContextPayloadDigest(evidence PersonalThreadDecisionEvidence, request MemoryPersonalContextDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.memory.personal_context\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", strings.TrimSpace(request.DecisionRef)},
		{"recipient_audience_ref", strings.TrimSpace(evidence.RecipientAudienceRef)},
		{"recipient_audience_hash", strings.TrimSpace(evidence.RecipientAudienceHash)},
		{"privacy_policy_ref", strings.TrimSpace(evidence.Privacy.PolicyRef)},
		{"resource_authorization_ref", strings.TrimSpace(evidence.ResourceAuthorizationRef)},
		{"thread_id", request.ThreadID},
		{"action_schema_hash", memoryPersonalContextSchema},
		{"idempotency_key", strings.TrimSpace(request.IdempotencyKey)},
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

// IssueMemoryPersonalContextDecision authorizes one turn of personal memory for one thread by one current
// editor, manager or owner. The evidence comes from ResolveSharedMemoryDeclassifyDecisionEvidence, the shared
// Space resolver the memory decisions use. Zero Data Retention is refused before issuance.
func IssueMemoryPersonalContextDecision(evidence PersonalThreadDecisionEvidence, request MemoryPersonalContextDecisionRequest, now time.Time) (Decision, error) {
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if err := evidence.ValidateForMemoryPersonalContext(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("memory personal context decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, memoryPersonalContextAudience, memoryPersonalContextAction, memoryPersonalContextSchema,
		memoryPersonalContextPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{MemoryPersonalContextPermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
