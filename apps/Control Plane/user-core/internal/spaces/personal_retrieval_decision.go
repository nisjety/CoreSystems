package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

const (
	personalRetrievalAction   = "data.retrieval.read"
	personalRetrievalAudience = "data-plane-retrieval"
	personalRetrievalSchema   = "sha256:retrieval-read-v1"
)

// PersonalRetrievalDecisionRequest has only a retry identity. Control derives
// the actor, tenant, audience, privacy claims, and resource authorization
// from current state; callers cannot turn an arbitrary query into authority.
type PersonalRetrievalDecisionRequest struct {
	DecisionRef    string
	IdempotencyKey string
	Nonce          string
}

func (r PersonalRetrievalDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("personal retrieval decision %s is required", label)
		}
	}
	return nil
}

func personalRetrievalPayloadDigest(evidence PersonalThreadDecisionEvidence, request PersonalRetrievalDecisionRequest) string {
	value := strings.Join([]string{
		"data.retrieval.read", "v1", evidence.Membership.OrgID, evidence.Membership.SubjectID,
		evidence.Membership.SpaceRef, request.DecisionRef, evidence.RecipientAudienceRef,
		evidence.Privacy.PolicyRef, evidence.ResourceAuthorizationRef, personalRetrievalSchema,
		request.IdempotencyKey,
	}, "\x00")
	sum := sha256.Sum256([]byte(value))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// IssuePersonalRetrievalDecision emits a short-lived, target-specific read
// authority. It never reuses a thread:create decision or its resource ref.
func IssuePersonalRetrievalDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalRetrievalDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForRetrieval(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("personal retrieval decision issuance time is required")
	}
	return Decision{
		DecisionRef:               strings.TrimSpace(request.DecisionRef),
		OrgID:                     evidence.Membership.OrgID,
		SpaceRef:                  evidence.Membership.SpaceRef,
		SubjectID:                 evidence.Membership.SubjectID,
		ServiceAudience:           personalRetrievalAudience,
		ActionID:                  personalRetrievalAction,
		ActionSchemaHash:          personalRetrievalSchema,
		PayloadDigest:             personalRetrievalPayloadDigest(evidence, request),
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
		Permissions:               []string{"retrieval:read"},
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
