package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// WS10-S6 / WS10-S7: the Control decision that lets one Space member decide an
// approval that Model Plane raised in a Space run, and the turn_origin rules
// that bound what such a decision may do.
//
// The decision is a Control claim, never a caller field: Session Core reads the
// approval's turn_origin (a copy of the run's Control-signed claim) and refuses
// any scope wider than once for a non-direct turn. Control applies the same
// rules BEFORE it signs, so a wide scope on an ambient or automation turn is
// never signed in the first place (defence in depth; Session Core still
// refuses). NULL or empty reads as direct: every run that predates the claim
// was started by a person.
//
// The payload digest is pinned to mp-contracts `space_approvals.rs`
// (decide_payload_digest): header `model.approval.decide\0v1\0`, then each
// field as `name \0 uint64be(len) value`, in the order org_id, user_id (the
// approver), approval_id, space_id, space_decision_ref, decision, scope,
// action_schema_hash. Change it only on an intended wire change, on both sides.
const (
	approvalDecideAction   = "model.approval.decide"
	approvalDecideSchema   = "sha256:approval-decide-v1"
	approvalDecideAudience = "model-plane"

	// ApprovalDecidePermission is the only permission a member-decide decision
	// carries. It is disjoint from the thread, run and wake permissions.
	ApprovalDecidePermission = "approval:decide"
)

// Decisions an approver can make.
const (
	ApprovalDecisionGranted = "granted"
	ApprovalDecisionDenied  = "denied"
)

// Scopes a member decide can name. Session Core owns once, thread and run;
// "always" is a Control standing rule created through the rule routes, never
// through a decide.
const ApprovalScopeRun ApprovalGrantScope = "run"

// Addressee kinds as Session Core stores them (`approvals.addressee_kind`).
const (
	ApprovalAddresseeNamed = "named"
)

// TurnOriginIsDirect reports whether origin is a person's own turn. An empty
// origin is direct (pre-claim runs). An unknown origin is NOT direct: an
// unrecognised claim never gains the wider direct rights.
func TurnOriginIsDirect(origin string) bool {
	origin = strings.TrimSpace(origin)
	return origin == "" || origin == TurnOriginDirect
}

// ApprovalScopeAllowedForTurnOrigin is the S7 scope rule: a direct (or
// unclaimed) turn may use any decide scope; an ambient, automation or unknown
// turn may decide once only, because no person was watching that turn and a
// wider grant would let one judgement stand for later calls.
func ApprovalScopeAllowedForTurnOrigin(origin, scope string) bool {
	scope = strings.TrimSpace(scope)
	switch scope {
	case string(ApprovalScopeOnce):
		return true
	case string(ApprovalScopeThread), string(ApprovalScopeRun):
		return TurnOriginIsDirect(origin)
	}
	return false
}

// DefaultApprovalAddresseeKindForTurnOrigin is the S7 addressee rule: a
// non-direct turn has no requester at the keyboard, so an approval with no
// stated addressee goes to any manager; a direct turn keeps the requester. A
// stated addressee is returned unchanged.
func DefaultApprovalAddresseeKindForTurnOrigin(origin, stated string) string {
	stated = strings.TrimSpace(stated)
	if stated != "" {
		return stated
	}
	if TurnOriginIsDirect(origin) {
		return ApprovalAddresseeRequester
	}
	return ApprovalAddresseeAnyManager
}

// ApprovalDecideAuthority is the content-free projection Control reads from
// Session Core (RunService.ResolveApprovalAuthority) before it signs: ids,
// state and the addressee, never the action's arguments.
type ApprovalDecideAuthority struct {
	ApprovalID           string
	OrgID                string
	SpaceRef             string
	RunID                string
	RequesterSubjectID   string
	AddresseeKind        string
	AddresseeSubjectID   string
	TurnOrigin           string
	State                string
	ZeroDataRetentionRun bool
}

// Validate checks the projection is complete and the approval is still open.
func (a ApprovalDecideAuthority) Validate() error {
	for name, value := range map[string]string{
		"approval_id": a.ApprovalID, "org_id": a.OrgID, "space_ref": a.SpaceRef,
		"run_id": a.RunID, "requester_subject_id": a.RequesterSubjectID, "state": a.State,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("approval decide authority %s is required", name)
		}
	}
	if strings.TrimSpace(a.State) != "requested" {
		return fmt.Errorf("approval decide authority approval is not open")
	}
	if a.ZeroDataRetentionRun {
		return fmt.Errorf("approval decide authority run is under Zero Data Retention")
	}
	return nil
}

// addresseeKind is the approval's effective addressee kind after the S7
// default for its turn origin.
func (a ApprovalDecideAuthority) addresseeKind() string {
	return DefaultApprovalAddresseeKindForTurnOrigin(a.TurnOrigin, a.AddresseeKind)
}

// ApprovalDecideRequest carries the operation-bound fields of one decide.
type ApprovalDecideRequest struct {
	DecisionRef    string
	ApprovalID     string
	Decision       string
	Scope          string
	IdempotencyKey string
	Nonce          string
}

// Validate checks shape only: scope and origin are judged against the approval
// in IssueApprovalDecideDecision.
func (r ApprovalDecideRequest) Validate() error {
	for name, value := range map[string]string{
		"decision_ref": r.DecisionRef, "approval_id": r.ApprovalID, "decision": r.Decision,
		"scope": r.Scope, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("approval decide %s is required", name)
		}
	}
	switch strings.TrimSpace(r.Decision) {
	case ApprovalDecisionGranted, ApprovalDecisionDenied:
	default:
		return fmt.Errorf("approval decide decision is invalid")
	}
	switch strings.TrimSpace(r.Scope) {
	case string(ApprovalScopeOnce), string(ApprovalScopeThread), string(ApprovalScopeRun):
	default:
		return fmt.Errorf("approval decide scope is invalid")
	}
	if len(strings.TrimSpace(r.IdempotencyKey)) > 200 || len(strings.TrimSpace(r.DecisionRef)) > 200 ||
		len(strings.TrimSpace(r.Nonce)) > 200 || len(strings.TrimSpace(r.ApprovalID)) > 128 {
		return fmt.Errorf("approval decide binding is oversized")
	}
	return nil
}

// ValidateForApprovalDecide authorizes a member deciding an approval: any
// current member of a shared Space who is in the current recipient audience
// (viewer floor, because a named addressee may be any member; who may decide
// THIS approval is the addressee rule in approvalDecideAddresseeAllows), not
// under Zero Data Retention (a durable decision record fails closed).
func (e PersonalThreadDecisionEvidence) ValidateForApprovalDecide() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return kindUnsupportedDenial(string(e.Membership.Kind), "member approval decide requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "viewer", "editor", "manager", "owner") {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot decide an approval", e.Membership.Role), "viewer", "editor", "manager", "owner")
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return authorityIncompleteDenial("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return authorityIncompleteDenial("approval decide authority is incomplete")
	}
	if err := e.Privacy.Validate(); err != nil {
		return err
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("approval_decide", "member approval decisions are not available under Zero Data Retention")
	}
	return nil
}

// approvalDecideAddresseeAllows is the addressee rule: the approval's effective
// addressee (after the origin default) must include the caller. A requester
// addressee is the run's requester only; named is that one member; any_editor
// and any_manager are by role. Session Core re-checks this with live
// revocation state.
func approvalDecideAddresseeAllows(authority ApprovalDecideAuthority, subjectID, role string) bool {
	subjectID = strings.TrimSpace(subjectID)
	switch authority.addresseeKind() {
	case ApprovalAddresseeRequester:
		return subjectID != "" && subjectID == strings.TrimSpace(authority.RequesterSubjectID)
	case ApprovalAddresseeNamed:
		return subjectID != "" && subjectID == strings.TrimSpace(authority.AddresseeSubjectID)
	case ApprovalAddresseeAnyEditor:
		return matchesOneOf(role, "editor", "manager", "owner")
	case ApprovalAddresseeAnyManager:
		return matchesOneOf(role, "manager", "owner")
	}
	return false
}

// approvalDecidePayloadDigest binds the exact decide Session Core verifies.
// Field order and encoding mirror mp-contracts decide_payload_digest.
func approvalDecidePayloadDigest(orgID, approverID, spaceRef string, request ApprovalDecideRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.approval.decide\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", orgID}, {"user_id", approverID}, {"approval_id", strings.TrimSpace(request.ApprovalID)},
		{"space_id", spaceRef}, {"space_decision_ref", strings.TrimSpace(request.DecisionRef)},
		{"decision", strings.TrimSpace(request.Decision)}, {"scope", strings.TrimSpace(request.Scope)},
		{"action_schema_hash", approvalDecideSchema},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueApprovalDecideDecision issues the member-decide decision. The evidence
// is the approver's freshly resolved authority; authority is Session Core's
// content-free projection of the approval. It refuses (with an error the route
// maps to the typed denial) when the approval belongs to another Space or org,
// is not open, the caller is not an addressee, or the scope is wider than the
// approval's turn origin allows (S7).
func IssueApprovalDecideDecision(evidence PersonalThreadDecisionEvidence, authority ApprovalDecideAuthority, request ApprovalDecideRequest, now time.Time) (Decision, error) {
	if err := evidence.ValidateForApprovalDecide(); err != nil {
		return Decision{}, err
	}
	if err := authority.Validate(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("approval decide request is invalid")
	}
	if strings.TrimSpace(authority.ApprovalID) != strings.TrimSpace(request.ApprovalID) ||
		strings.TrimSpace(authority.OrgID) != evidence.Membership.OrgID ||
		strings.TrimSpace(authority.SpaceRef) != evidence.Membership.SpaceRef {
		return Decision{}, fmt.Errorf("approval decide authority does not match the approver's Space")
	}
	// S7: a non-direct turn decides once only. A denial carries no grant, so any
	// scope is harmless for it, but the scope field is still committed to the
	// digest: pin it to once so a denial cannot be replayed as a wide grant.
	if !ApprovalScopeAllowedForTurnOrigin(authority.TurnOrigin, request.Scope) {
		return Decision{}, fmt.Errorf("approval decide scope is not permitted for this turn origin")
	}
	if !approvalDecideAddresseeAllows(authority, evidence.Membership.SubjectID, evidence.Membership.Role) {
		return Decision{}, fmt.Errorf("approval decide caller is not an addressee of this approval")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, approvalDecideAudience, approvalDecideAction, approvalDecideSchema,
		approvalDecidePayloadDigest(evidence.Membership.OrgID, evidence.Membership.SubjectID, evidence.Membership.SpaceRef, request),
		request.IdempotencyKey, request.Nonce,
		[]string{ApprovalDecidePermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
