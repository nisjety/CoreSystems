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

	// ThreadReadAudienceSessionCore is the default recipient and the only one
	// that existed before S4.2: Session Core, which owns the conversation
	// record this decision was written for. The literal is `model-plane`
	// rather than something service-shaped because that is the value Session
	// Core's verifier already compares against; renaming it would invalidate
	// every decision in flight for a cosmetic gain.
	ThreadReadAudienceSessionCore = personalThreadCreateAudience

	// ThreadReadAudienceSandboxManager names sandbox-manager as the recipient,
	// so the Work tab can read a Space's background processes (S4.2 §7) under
	// the authority it already holds for that room.
	//
	// A SECOND AUDIENCE RATHER THAN A SECOND ACTION, deliberately. The
	// question the caller is asking is unchanged — "may this member see what
	// this Space did?" — and processes are part of that record in the same way
	// runs are. Minting a `model.process.read` action would have created a
	// second authority to keep in sync with the first, and a Space where a
	// member could read the conversation but not the work it produced (or the
	// reverse) is not a state anyone would choose on purpose.
	//
	// What the audience DOES buy is that the two are not interchangeable: a
	// decision issued for Session Core is refused by sandbox-manager and vice
	// versa, because each verifier pins its own. So the gateway must ask for
	// the recipient it means, and a token leaked from one path cannot be
	// replayed against the other.
	ThreadReadAudienceSandboxManager = "model-plane-sandbox-manager"
)

// threadReadAudiences is the closed set a caller may request. Closed rather
// than free-form for the reason S4.2 step 3 closed the `processes` claim: an
// unvalidated audience string is one Control would sign happily and no
// recipient would ever accept, which surfaces as an unexplained permission
// denial three services away.
var threadReadAudiences = map[string]struct{}{
	ThreadReadAudienceSessionCore:    {},
	ThreadReadAudienceSandboxManager: {},
}

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
	// ServiceAudience names the Model Plane service that will verify this
	// decision. Empty means Session Core, so every existing caller keeps the
	// decision it always got without knowing this field exists.
	ServiceAudience string
}

// Audience resolves the requested recipient, defaulting to Session Core.
func (r SharedThreadReadDecisionRequest) Audience() string {
	audience := strings.TrimSpace(r.ServiceAudience)
	if audience == "" {
		return ThreadReadAudienceSessionCore
	}
	return audience
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
	if _, ok := threadReadAudiences[r.Audience()]; !ok {
		return fmt.Errorf("shared thread read decision service_audience %q is not a recognized Model Plane recipient", r.Audience())
	}
	return nil
}

// sharedThreadReadPayloadDigest binds the exact read effect Model Plane will
// verify. Every field is length-prefixed so concatenation stays unambiguous,
// matching personalThreadCreatePayloadDigest.
//
// The service audience is deliberately NOT a digest input, even though S4.2
// made it variable. It is a signed field of the decision itself, so a tampered
// audience fails the signature check before any digest is recomputed — and
// adding it here would change the digest for every existing Session Core
// decision, which Session Core recomputes independently and would then reject.
// A formula two services must agree on is not the place to record something
// the signature already covers.
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
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
	)
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
	return newEvidenceDecision(
		evidence, request.DecisionRef, request.Audience(), threadReadAction, threadReadSchema,
		sharedThreadReadPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"thread:read"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
