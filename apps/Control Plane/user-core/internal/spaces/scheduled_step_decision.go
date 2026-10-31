package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"hash"
	"strings"
	"time"
)

const (
	scheduledStepAction   = "model.schedule.step"
	scheduledStepAudience = "model-plane-execution-core"
	scheduledStepSchema   = "sha256:space-scheduled-step-v1"
	// A step bound to a stored-consent contract binds the contract digest and
	// expiry, so it carries its own schema hash; the v1 digest is unchanged.
	scheduledStepSchemaContract = "sha256:space-scheduled-step-v2"
)

// ScheduledStepIntent is the non-secret binding Orchestrator presents for one
// deterministic turn. It contains no goal, tool, credential, or policy body.
type ScheduledStepIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	RunID          string
	ThreadID       string
	ScheduleID     string
	FireKey        string
	TemplateDigest string
	StepID         string
	StepIndex      uint32
	PolicyDigest   string
	IdempotencyKey string
	// ContractDigest and ContractExpiresAt are set only for a step that runs
	// under a stored-consent contract; zero values keep the legacy v1 binding.
	ContractDigest    string
	ContractExpiresAt time.Time
}

// ValidateAuthorityRequest validates the non-subject fields that Control sends
// to Session Core. SubjectID is deliberately omitted: the prepared scheduled
// run is the authority source and returns the subject that Control must use.
func (i ScheduledStepIntent) ValidateAuthorityRequest() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef,
		"run_id": i.RunID, "thread_id": i.ThreadID, "schedule_id": i.ScheduleID,
		"fire_key": i.FireKey, "template_digest": i.TemplateDigest,
		"step_id": i.StepID, "policy_digest": i.PolicyDigest,
		"idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("scheduled step intent %s is required", label)
		}
	}
	if strings.ContainsAny(i.ScheduleID+i.FireKey, "/.>* \t\r\n") {
		return fmt.Errorf("scheduled step identifiers are invalid")
	}
	for label, digest := range map[string]string{
		"template_digest": i.TemplateDigest, "policy_digest": i.PolicyDigest,
	} {
		if !strings.HasPrefix(digest, "sha256:") || len(digest) != len("sha256:")+64 {
			return fmt.Errorf("scheduled step %s is invalid", label)
		}
	}
	return nil
}

func (i ScheduledStepIntent) Validate() error {
	if err := i.ValidateAuthorityRequest(); err != nil {
		return err
	}
	if strings.TrimSpace(i.SubjectID) == "" {
		return fmt.Errorf("scheduled step intent subject_id is required")
	}
	return nil
}

// ScheduledStepAuthority is the exact prepared-run projection returned by
// Session Core before Control signs a step decision. No caller-supplied
// subject is accepted as the source of truth.
type ScheduledStepAuthority struct {
	RunID          string
	ThreadID       string
	OrgID          string
	SubjectID      string
	SpaceRef       string
	ScheduleID     string
	FireKey        string
	TemplateDigest string
	PolicyDigest   string
	StepID         string
	StepIndex      uint32
	IdempotencyKey string
	RunStatus      string
}

func (a ScheduledStepAuthority) Validate() error {
	if strings.TrimSpace(a.SubjectID) == "" {
		return fmt.Errorf("scheduled step authority requires a Session Core subject")
	}
	intent := ScheduledStepIntent{
		OrgID: a.OrgID, SpaceRef: a.SpaceRef, SubjectID: a.SubjectID, RunID: a.RunID,
		ThreadID: a.ThreadID, ScheduleID: a.ScheduleID, FireKey: a.FireKey,
		TemplateDigest: a.TemplateDigest, StepID: a.StepID, StepIndex: a.StepIndex,
		PolicyDigest: a.PolicyDigest, IdempotencyKey: a.IdempotencyKey,
	}
	if err := intent.Validate(); err != nil {
		return fmt.Errorf("invalid scheduled step authority: %w", err)
	}
	if a.StepID != fmt.Sprintf("%s:step:%d", a.RunID, a.StepIndex) {
		return fmt.Errorf("scheduled step authority step_id is invalid")
	}
	if strings.TrimSpace(a.RunStatus) == "" || matchesOneOf(a.RunStatus, "completed", "failed", "cancelled") {
		return fmt.Errorf("scheduled step authority run is terminal or missing")
	}
	return nil
}

func scheduledStepPayloadDigest(evidence PersonalThreadDecisionEvidence, intent ScheduledStepIntent, schema string) string {
	hash := sha256.New()
	hash.Write([]byte(scheduledStepAction + "\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"run_id", intent.RunID},
		{"thread_id", intent.ThreadID}, {"schedule_id", intent.ScheduleID},
		{"fire_key", intent.FireKey}, {"template_digest", intent.TemplateDigest},
		{"step_id", intent.StepID}, {"policy_digest", intent.PolicyDigest},
		{"idempotency_key", intent.IdempotencyKey},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", schema},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"step_index", int64(intent.StepIndex)},
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
	if intent.hasContract() {
		writeScheduledContractBinding(hash, intent.ContractDigest, intent.ContractExpiresAt)
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// hasScheduledContract reports whether a step or run is bound to a stored
// consent contract; a zero digest and expiry keep the legacy binding.
func hasScheduledContract(digest string, expiresAt time.Time) bool {
	return digest != "" || !expiresAt.IsZero()
}

// validateScheduledContract refuses a half-bound contract and an expired one;
// the verifier in execution-core applies the same rule again at each step.
func validateScheduledContract(digest string, expiresAt, now time.Time) error {
	if !hasScheduledContract(digest, expiresAt) {
		return nil
	}
	if !strings.HasPrefix(digest, "sha256:") || len(digest) != len("sha256:")+64 || expiresAt.IsZero() {
		return fmt.Errorf("scheduled contract binding is invalid")
	}
	if !now.Before(expiresAt) {
		return fmt.Errorf("scheduled contract has expired")
	}
	return nil
}

// writeScheduledContractBinding feeds the contract digest and expiry into a
// step or run payload digest. Execution Core recomputes the same bytes.
func writeScheduledContractBinding(h hash.Hash, contractDigest string, expiresAt time.Time) {
	h.Write([]byte("contract_digest"))
	h.Write([]byte{0})
	var length [8]byte
	binary.BigEndian.PutUint64(length[:], uint64(len(contractDigest)))
	h.Write(length[:])
	h.Write([]byte(contractDigest))
	writeDigestRevisions(h, digestRevision{"contract_expires_at", expiresAt.Unix()})
}

func (i ScheduledStepIntent) hasContract() bool {
	return hasScheduledContract(i.ContractDigest, i.ContractExpiresAt)
}

func (i ScheduledStepIntent) validateContract(now time.Time) error {
	return validateScheduledContract(i.ContractDigest, i.ContractExpiresAt, now)
}

// IssueScheduledStepDecision re-resolves the current Space authority for one
// turn. It is distinct from the schedule-fire/run decisions and is never a
// user delegation or a provider/effect authorization.
func IssueScheduledStepDecision(
	evidence PersonalThreadDecisionEvidence,
	intent ScheduledStepIntent,
	decisionRef, nonce string,
	now time.Time,
) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("space role cannot run a scheduled step")
	}
	if !evidence.ScheduleFireEntitled || !evidence.AgentActionEntitled {
		return Decision{}, fmt.Errorf("scheduled step entitlement is not active")
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable scheduled steps")
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("scheduled step intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("scheduled step decision fields are required")
	}
	if err := intent.validateContract(now); err != nil {
		return Decision{}, err
	}
	schema := scheduledStepSchema
	if intent.hasContract() {
		schema = scheduledStepSchemaContract
	}
	decision := newEvidenceDecision(
		evidence, decisionRef, scheduledStepAudience, scheduledStepAction, schema,
		scheduledStepPayloadDigest(evidence, intent, schema), intent.IdempotencyKey, nonce,
		[]string{"schedule:step"}, false, now,
	)
	if intent.hasContract() {
		expiresAt := intent.ContractExpiresAt.UTC()
		decision.ContractDigest = intent.ContractDigest
		decision.ContractExpiresAt = &expiresAt
	}
	return decision, nil
}
