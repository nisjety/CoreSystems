package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

const (
	threadReadAction = "model.thread.read"
	threadReadSchema = "sha256:thread-read-v1"
)

// SharedThreadReadDecisionRequest carries only the operation-bound fields the
// verified gateway path supplies. Deliberately absent: any thread id.
//
// A read decision is Space-scoped rather than thread-scoped because the room
// asks two questions with one authority — "which threads are in this room" and
// "what does this one say" — and a per-thread token would force the listing to
// mint a decision before it knew what to mint one for. Entry-level filtering
// is not lost by this: the decision carries the caller's current audience
// revision, and Session Core compares it against the snapshot stored on each
// row before returning it.
type SharedThreadReadDecisionRequest struct {
	DecisionRef    string
	IdempotencyKey string
	Nonce          string
}

func (r SharedThreadReadDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref":    r.DecisionRef,
		"idempotency_key": r.IdempotencyKey,
		"nonce":           r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("shared thread read decision %s is required", label)
		}
	}
	return nil
}

// sharedThreadReadPayloadDigest binds the exact read effect Model Plane will
// verify. Every field is length-prefixed so concatenation stays unambiguous,
// matching personalThreadCreatePayloadDigest.
func sharedThreadReadPayloadDigest(evidence PersonalThreadDecisionEvidence, request SharedThreadReadDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.thread.read\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", threadReadSchema},
		{"idempotency_key", request.IdempotencyKey},
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
		{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueSharedThreadReadDecision authorizes one short-lived read of a shared
// Space's conversation record by one current member.
//
// It is intentionally NOT reusable as a create or append grant: the action id,
// schema hash, permission and payload digest all differ, so a reader's token
// cannot be replayed to write into the room. The reverse is also true — an
// existing thread:create token does not admit its holder to anyone else's
// turns, which is the whole point of giving reads their own class.
func IssueSharedThreadReadDecision(
	evidence PersonalThreadDecisionEvidence,
	request SharedThreadReadDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedThreadRead(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("shared thread read decision issuance time is required")
	}
	return Decision{
		DecisionRef:               strings.TrimSpace(request.DecisionRef),
		OrgID:                     evidence.Membership.OrgID,
		SpaceRef:                  evidence.Membership.SpaceRef,
		SubjectID:                 evidence.Membership.SubjectID,
		ServiceAudience:           personalThreadCreateAudience,
		ActionID:                  threadReadAction,
		ActionSchemaHash:          threadReadSchema,
		PayloadDigest:             sharedThreadReadPayloadDigest(evidence, request),
		IdempotencyKey:            strings.TrimSpace(request.IdempotencyKey),
		RecipientAudienceRef:      strings.TrimSpace(evidence.RecipientAudienceRef),
		RecipientAudienceHash:     strings.TrimSpace(evidence.RecipientAudienceHash),
		PrivacyPolicyRef:          strings.TrimSpace(evidence.Privacy.PolicyRef),
		ResourceAuthorizationRef:  strings.TrimSpace(evidence.ResourceAuthorizationRef),
		AuthorityRevision:         evidence.Membership.Revisions.Authority,
		MembershipRevision:        evidence.Membership.Revisions.Membership,
		PrivacyRevision:           evidence.Membership.Revisions.Privacy,
		RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		EntitlementRevision:       evidence.Membership.Revisions.Entitlement,
		Permissions:               []string{"thread:read"},
		Purpose:                   strings.TrimSpace(evidence.Privacy.Purpose),
		LawfulBasis:               strings.TrimSpace(evidence.Privacy.LawfulBasis),
		PrivacyClass:              strings.TrimSpace(evidence.Privacy.PrivacyClass),
		ThirdPartyAllowed:         evidence.Privacy.ThirdPartyAllowed,
		RetentionClass:            strings.TrimSpace(evidence.Privacy.RetentionClass),
		Residency:                 strings.TrimSpace(evidence.Privacy.Residency),
		DeletionScope:             strings.TrimSpace(evidence.Privacy.DeletionScope),
		ZeroDataRetention:         evidence.Privacy.ZeroDataRetention,
		IssuedAt:                  now.UTC(),
		ExpiresAt:                 now.UTC().Add(personalDecisionLifetime),
		Nonce:                     strings.TrimSpace(request.Nonce),
	}, nil
}
