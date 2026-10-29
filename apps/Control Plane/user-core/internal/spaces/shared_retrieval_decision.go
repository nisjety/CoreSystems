package spaces

import (
	"fmt"
	"strings"
	"time"
)

// IssueSharedRetrievalDecision uses the same retrieval effect digest and
// audience as personal retrieval, but only after Control has resolved a
// current shared recipient audience and confirmed retrieval entitlement for
// that Space's org. It deliberately accepts no caller-selected recipient
// fields; those come from the registered audience snapshot in Repository.
func IssueSharedRetrievalDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalRetrievalDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedRetrieval(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("shared retrieval decision issuance time is required")
	}
	return Decision{
		DecisionRef:               strings.TrimSpace(request.DecisionRef),
		OrgID:                     evidence.Membership.OrgID,
		SpaceRef:                  evidence.Membership.SpaceRef,
		SubjectID:                 evidence.Membership.SubjectID,
		ServiceAudience:           retrievalReadAudience,
		ActionID:                  retrievalReadAction,
		ActionSchemaHash:          retrievalReadSchema,
		PayloadDigest:             retrievalPayloadDigest(evidence, request),
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
