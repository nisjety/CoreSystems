package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// WS09-S7: the Control decision that authorizes cancelling one live Model run
// in a shared Space on behalf of a room manager or owner.
//
// It is NOT a thread read or write grant. The action, schema, permission and
// payload digest all differ from model.thread.read / model.thread.create, and
// the digest binds the run id and the operation, so a token cannot be replayed
// against another run or another operation. Session Core verifies it with its
// own copy of the digest below (WS09-S8) and must refuse, both ways, a read
// token carrying run:cancel and a control token carrying any thread permission.
const (
	runControlAction   = "model.run.control"
	runControlSchema   = "sha256:run-control-v1"
	runControlAudience = "model-plane"

	// RunControlPermission is the only permission a run-control decision
	// carries. It is disjoint from the thread permissions (thread:create /
	// thread:append / thread:read) and from run:wake.
	RunControlPermission = "run:cancel"

	// RunControlOperationCancel is the only operation issued today.
	RunControlOperationCancel = "cancel"

	maxRunControlRunIDLen = 128
)

// RunControlDecisionRequest carries the operation-bound fields of one run
// control. It holds no content: the run id and operation are the whole effect.
type RunControlDecisionRequest struct {
	DecisionRef    string
	RunID          string
	Operation      string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only. It refuses every operation Control does not issue,
// so a caller cannot obtain a signed claim for an operation by asking for it.
func (r RunControlDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "run_id": r.RunID, "operation": r.Operation,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("run control decision %s is required", label)
		}
	}
	if len(strings.TrimSpace(r.RunID)) > maxRunControlRunIDLen {
		return fmt.Errorf("run control decision run_id is too long")
	}
	if strings.TrimSpace(r.Operation) != RunControlOperationCancel {
		return fmt.Errorf("run control decision operation must be %s", RunControlOperationCancel)
	}
	return nil
}

// runControlPayloadDigest binds the exact control effect Session Core verifies:
// the thread-read digest fields plus run_id and operation, every field
// length-prefixed so concatenation stays unambiguous, then the two revisions
// (pattern of sharedThreadReadPayloadDigest).
func runControlPayloadDigest(evidence PersonalThreadDecisionEvidence, request RunControlDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.run.control\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"run_id", strings.TrimSpace(request.RunID)},
		{"operation", strings.TrimSpace(request.Operation)},
		{"action_schema_hash", runControlSchema},
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

// IssueSharedRunControlDecision authorizes cancelling one run by one current
// room manager or owner. The evidence must come from
// ResolveSharedRunControlDecisionEvidence (current membership, current
// recipient audience, run_control_entitled). The decision carries the Space ZDR
// claim; whether a ZDR run may be cancelled this way is Session Core's call.
func IssueSharedRunControlDecision(evidence PersonalThreadDecisionEvidence, request RunControlDecisionRequest, now time.Time) (Decision, error) {
	if err := evidence.ValidateForSharedRunControl(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("run control decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, runControlAudience, runControlAction, runControlSchema,
		runControlPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{RunControlPermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
