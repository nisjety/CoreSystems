package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// D21h: the starter Stop decision for a room browser run
// (model.space.browser_run_stop).
//
// Today only a room manager or owner can stop a run (model.run.control, which
// needs run_control_entitled). The member who started a browser run must be
// able to stop it too, whatever their room role. Control cannot see who
// started a run: the run belongs to Session Core and Model Plane. So this
// decision binds the caller (subject), the Space and the run id, and the
// model-gateway verifier must refuse it unless the run belongs to this Space
// and its owner is the subject. That makes the decision "this current room
// member may ask to stop run R"; whether R is theirs is the verifier's check.
//
// It needs no entitlement bit: stopping is the safe direction, and a person
// who could start a run must always be able to stop it, even after the org
// turned browser_entitled or run_control_entitled off. An archived room still
// issues it (D22aj: an archived room's run may still be stopped). It is
// issued under Space zero data retention too (D23, fixing a gap a D21h
// builder found): a ZDR Space cannot START a browser run (the start decision
// refuses ZDR), but an org can turn ZDR on while a run from before that is
// still active, and "stopping is always allowed" must hold then too. The
// decision still carries ZeroDataRetention for the verifier's own use; the
// model-gateway route's live ZDR check (D21h's gw leg) is about the run
// itself, not about whether its starter may stop it.
//
// It is NOT a run-control or thread grant. Action, schema, audience,
// permission and digest header all differ from model.run.control and
// model.space.browser_run, so a stop token cannot be replayed as either.
const (
	browserRunStopAction   = "model.space.browser_run_stop"
	browserRunStopAudience = "model-plane-model-gateway"
	browserRunStopSchema   = "sha256:space-browser-run-stop-v1"
	// browserRunStopDigestHeader opens the payload digest. The model-gateway
	// verifier recomputes the same bytes, so the header is part of the wire contract.
	browserRunStopDigestHeader = "model.space.browser_run_stop\x00v1\x00"

	// BrowserRunStopPermission is the only permission the decision carries.
	BrowserRunStopPermission = "space:browser:stop"

	maxBrowserRunStopRunIDBytes = 128
)

// BrowserRunStopDecisionRequest carries the operation-bound fields only. It names
// no actor, organisation, role, audience, policy or revision. DecisionRef and
// Nonce are minted by the route, not sent by the caller.
type BrowserRunStopDecisionRequest struct {
	DecisionRef    string
	RunID          string
	IdempotencyKey string
	Nonce          string
}

// BrowserRunStopClaims are the non-secret values the payload digest binds beyond
// the Decision's own fields. Control returns them beside the token; the gateway
// forwards them and the verifier recomputes the digest from them.
type BrowserRunStopClaims struct {
	RunID string `json:"run_id"`
}

// Validate checks shape only, before any authority is read. Surrounding
// whitespace is refused rather than trimmed, so the digest binds exactly the
// bytes the caller sent.
func (r BrowserRunStopDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "run_id": r.RunID,
		"idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("browser run stop decision %s is required", label)
		}
	}
	if !browserTokenOK(r.RunID, maxBrowserRunStopRunIDBytes) {
		return fmt.Errorf("browser run stop decision run_id is invalid")
	}
	if !browserTokenOK(r.IdempotencyKey, browserMaxIdempotencyKeyBytes) {
		return fmt.Errorf("browser run stop decision idempotency_key is invalid")
	}
	return nil
}

// ValidateForBrowserRunStop authorizes a stop request by any current member of a
// shared Space that can hold a browser run (room, project, case). There is no
// role floor and no entitlement bit (see the file comment). A personal Space has
// one member, who stops their own run through the owner-only browser control
// route; a direct or group direct message has no browser run (D-8).
func (e PersonalThreadDecisionEvidence) ValidateForBrowserRunStop() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	switch e.Membership.Kind {
	case KindRoom, KindProject, KindCase:
	default:
		return kindUnsupportedDenial(string(e.Membership.Kind), "a starter browser run stop is available in room, project and case Spaces only")
	}
	if !matchesOneOf(e.Membership.Role, RoleViewer, RoleEditor, RoleManager, RoleOwner) {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot stop a browser run", e.Membership.Role), RoleViewer, RoleEditor, RoleManager, RoleOwner)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("shared Space browser run stop authority is incomplete")
	}
	if err := e.Privacy.Validate(); err != nil {
		return err
	}
	// D23: no ZDR refusal here — see the file comment. Issued either way; the
	// decision's ZeroDataRetention field still carries the fact for the verifier.
	return nil
}

// browserRunStopPayloadDigest binds the exact stop the verifier may perform: the
// run id under the caller, Space, audience and authority. Every field is
// length-prefixed (name, NUL, 8-byte big-endian length, value), then the five
// revisions (name, NUL, 8-byte big-endian value). References and the
// idempotency key are hashed trimmed, exactly as newEvidenceDecision writes
// them into the decision. The golden vectors in
// testdata/space-browser-run-stop-decision.golden.json pin this formula for the
// model-gateway verifier; this function is the source of truth.
func browserRunStopPayloadDigest(evidence PersonalThreadDecisionEvidence, request BrowserRunStopDecisionRequest, claims BrowserRunStopClaims) string {
	hash := sha256.New()
	hash.Write([]byte(browserRunStopDigestHeader))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"run_id", claims.RunID},
		{"recipient_audience_ref", strings.TrimSpace(evidence.RecipientAudienceRef)},
		{"recipient_audience_hash", strings.TrimSpace(evidence.RecipientAudienceHash)},
		{"privacy_policy_ref", strings.TrimSpace(evidence.Privacy.PolicyRef)},
		{"resource_authorization_ref", strings.TrimSpace(evidence.ResourceAuthorizationRef)},
		{"action_schema_hash", browserRunStopSchema},
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
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueBrowserRunStopDecision signs one starter stop request. The evidence must
// come from ResolveSharedBrowserRunStopDecisionEvidence (current membership,
// current recipient audience).
func IssueBrowserRunStopDecision(evidence PersonalThreadDecisionEvidence, request BrowserRunStopDecisionRequest, now time.Time) (Decision, BrowserRunStopClaims, error) {
	if err := request.Validate(); err != nil {
		return Decision{}, BrowserRunStopClaims{}, err
	}
	if now.IsZero() {
		return Decision{}, BrowserRunStopClaims{}, fmt.Errorf("browser run stop decision issuance time is required")
	}
	if err := evidence.ValidateForBrowserRunStop(); err != nil {
		return Decision{}, BrowserRunStopClaims{}, err
	}
	claims := BrowserRunStopClaims{RunID: request.RunID}
	decision := newEvidenceDecision(
		evidence, request.DecisionRef, browserRunStopAudience, browserRunStopAction, browserRunStopSchema,
		browserRunStopPayloadDigest(evidence, request, claims), request.IdempotencyKey, request.Nonce,
		[]string{BrowserRunStopPermission}, evidence.Privacy.ZeroDataRetention, now,
	)
	return decision, claims, nil
}
