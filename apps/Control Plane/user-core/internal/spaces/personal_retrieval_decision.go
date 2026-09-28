package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

const (
	retrievalReadAction   = "data.retrieval.read"
	retrievalReadAudience = "data-plane-retrieval"
	retrievalReadSchema   = "sha256:retrieval-read-v1"
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

// retrievalPayloadDigest is shared by the personal and shared retrieval
// issuers: it only serializes resolved evidence and request fields, with no
// personal/shared branching of its own.
func retrievalPayloadDigest(evidence PersonalThreadDecisionEvidence, request PersonalRetrievalDecisionRequest) string {
	value := strings.Join([]string{
		"data.retrieval.read", "v1", evidence.Membership.OrgID, evidence.Membership.SubjectID,
		evidence.Membership.SpaceRef, request.DecisionRef, evidence.RecipientAudienceRef,
		evidence.Privacy.PolicyRef, evidence.ResourceAuthorizationRef, retrievalReadSchema,
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
	return newRetrievalReadDecision(evidence, request, now), nil
}

// newRetrievalReadDecision builds the retrieval-read decision shared by the
// personal and shared issuers; only their evidence validation differs.
func newRetrievalReadDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalRetrievalDecisionRequest,
	now time.Time,
) Decision {
	return newEvidenceDecision(
		evidence, request.DecisionRef, retrievalReadAudience, retrievalReadAction, retrievalReadSchema,
		retrievalPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"retrieval:read"}, evidence.Privacy.ZeroDataRetention, now,
	)
}
