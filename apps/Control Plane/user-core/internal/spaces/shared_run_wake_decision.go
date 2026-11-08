package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// WS08-S1: the Control decision that authorizes starting a Model run from an
// already stored human post ("wake" an addressed agent).
//
// A wake is not a thread append: it adds no turn. It lets one editor, manager
// or owner start one run, for one bound agent, from one stored post, and it
// signs the turn origin so `runs.turn_origin` is a Control claim and never a
// caller field. It is issued for a shared (non-personal) Space and for a
// personal Space exactly as IssueThreadAppendDecision is, from the same
// evidence, with no new entitlement bit (wake reuses thread_create).
//
// Session Core verifies the token with its own copy of the digest below. The
// two copies are pinned by testdata/run-wake-v1.golden.json, which both sides
// assert. Change the digest only on an intended wire change, on both sides.
const (
	runWakeAction   = "model.run.wake"
	runWakeSchema   = "sha256:run-wake-v1"
	runWakeAudience = "model-plane"

	// RunWakePermission is the only permission a wake decision carries. It is
	// disjoint from the thread permissions (thread:create / thread:append /
	// thread:read): a verifier refuses a wake token that carries any thread
	// permission and refuses a thread token that carries this one.
	RunWakePermission = "run:wake"
)

// Turn origins. Control signs one of these into the wake decision; Session Core
// stores it on `runs.turn_origin` (NULL on legacy runs reads as direct).
const (
	TurnOriginDirect     = "direct"
	TurnOriginAmbient    = "ambient"
	TurnOriginAutomation = "automation"
)

// Run origins of the Model Plane run vocabulary (`runs.metadata.origin`), the
// other end of the turn_origin mapping table (ADR-0012, X07).
const (
	RunOriginChat     = "chat"
	RunOriginSchedule = "schedule"
	RunOriginTrigger  = "trigger"
)

// turnOriginRunOrigin is the mapping table between the Space turn origin (owned
// here, signed by Control) and the Model Plane run origin enum (owned by the
// execution side). The table is part of the golden so both sides cannot drift.
var turnOriginRunOrigin = []struct{ TurnOrigin, RunOrigin string }{
	{TurnOriginDirect, RunOriginChat},
	{TurnOriginAmbient, RunOriginTrigger},
	{TurnOriginAutomation, RunOriginSchedule},
}

// RunOriginForTurnOrigin maps a turn origin to the run origin it is recorded as.
// The second result is false for an unknown turn origin.
func RunOriginForTurnOrigin(turnOrigin string) (string, bool) {
	for _, row := range turnOriginRunOrigin {
		if row.TurnOrigin == strings.TrimSpace(turnOrigin) {
			return row.RunOrigin, true
		}
	}
	return "", false
}

// ValidTurnOrigin reports whether origin is a known turn origin (issuable or not).
func ValidTurnOrigin(origin string) bool {
	_, ok := RunOriginForTurnOrigin(origin)
	return ok
}

// RunWakeDecisionRequest carries the operation-bound fields of one wake. It
// holds no content: ContentDigest is the SHA-256 commitment to the stored
// post's bytes, which Session Core recomputes from the trigger row.
type RunWakeDecisionRequest struct {
	DecisionRef      string
	ThreadID         string
	TriggerMessageID string
	ContentDigest    string
	AgentSubjectID   string
	TurnOrigin       string
	IdempotencyKey   string
	Nonce            string
}

// Validate checks shape only. It also refuses every origin Control does not
// issue yet: ambient and automation stay refused until the slices that need
// them exist, so a caller cannot obtain a non-direct claim by asking.
func (r RunWakeDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "thread_id": r.ThreadID, "trigger_message_id": r.TriggerMessageID,
		"content_digest": r.ContentDigest, "agent_subject_id": r.AgentSubjectID,
		"turn_origin": r.TurnOrigin, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("run wake decision %s is required", label)
		}
	}
	if !strings.HasPrefix(r.ContentDigest, "sha256:") || len(r.ContentDigest) != len("sha256:")+64 {
		return fmt.Errorf("run wake content digest is invalid")
	}
	if err := ValidateRunWakeTurnOrigin(r.TurnOrigin); err != nil {
		return err
	}
	return nil
}

// ValidateRunWakeTurnOrigin refuses an unknown origin and any known origin that
// Control does not issue yet. Only direct is issued.
func ValidateRunWakeTurnOrigin(origin string) error {
	if !ValidTurnOrigin(origin) {
		return fmt.Errorf("run wake turn_origin is unknown")
	}
	if strings.TrimSpace(origin) != TurnOriginDirect {
		return fmt.Errorf("run wake turn_origin must be direct")
	}
	return nil
}

// runWakeAgentOnRoster reports whether agentSubjectID is an active service
// member of the Space. RosterForSpace returns active memberships only.
func runWakeAgentOnRoster(roster []RosterMember, agentSubjectID string) bool {
	agentSubjectID = strings.TrimSpace(agentSubjectID)
	for _, member := range roster {
		if member.SubjectType == "service" && strings.TrimSpace(member.SubjectID) == agentSubjectID {
			return true
		}
	}
	return false
}

// runWakePayloadDigest binds the exact wake effect Session Core verifies. It is
// the append field set plus the four wake fields, every field length-prefixed
// so concatenation stays unambiguous, then the two revisions (pattern of
// threadAppendPayloadDigest).
func runWakePayloadDigest(evidence PersonalThreadDecisionEvidence, request RunWakeDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.run.wake\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"thread_id", request.ThreadID}, {"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"trigger_message_id", request.TriggerMessageID}, {"content_digest", request.ContentDigest},
		{"agent_subject_id", request.AgentSubjectID}, {"turn_origin", request.TurnOrigin},
		{"action_schema_hash", runWakeSchema}, {"idempotency_key", request.IdempotencyKey},
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

// IssueRunWakeDecision issues the wake decision. The evidence must be the same
// current-authority evidence a thread append needs (editor and above, thread
// entitlement, current recipient audience), and roster must be the Space's
// current roster: the woken agent has to be an active service member of THIS
// Space, so a wake cannot name an agent the room never bound.
func IssueRunWakeDecision(evidence PersonalThreadDecisionEvidence, roster []RosterMember, request RunWakeDecisionRequest, now time.Time) (Decision, error) {
	if evidence.Membership.Kind == KindPersonal {
		if err := evidence.Validate(); err != nil {
			return Decision{}, err
		}
	} else if err := evidence.ValidateForSharedThread(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("run wake decision request is invalid")
	}
	if !runWakeAgentOnRoster(roster, request.AgentSubjectID) {
		return Decision{}, fmt.Errorf("run wake agent is not an active service member of the Space")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, runWakeAudience, runWakeAction, runWakeSchema,
		runWakePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{RunWakePermission}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
