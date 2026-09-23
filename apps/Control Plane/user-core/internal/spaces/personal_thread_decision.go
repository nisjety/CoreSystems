package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"hash"
	"slices"
	"sort"
	"strings"
	"time"
)

const (
	personalThreadCreateAction   = "model.thread.create"
	personalThreadCreateAudience = "model-plane"
	personalThreadCreateSchema   = "sha256:thread-create-v1"
	threadAppendAction           = "model.thread.append"
	threadAppendSchema           = "sha256:thread-append-v1"
	personalDecisionLifetime     = 2 * time.Minute
)

// PrivacyPolicySnapshot is the Control-owned policy material resolved before a
// decision is issued. It is deliberately complete: issuing with an unknown
// policy would turn a cache miss or deployment gap into an implicit allow.
type PrivacyPolicySnapshot struct {
	PolicyRef         string
	Purpose           string
	LawfulBasis       string
	PrivacyClass      string
	ThirdPartyAllowed bool
	RetentionClass    string
	Residency         string
	DeletionScope     string
	ZeroDataRetention bool
}

// RecipientAudienceHash is a content-free, deterministic commitment to the
// server-resolved recipient principal set. It deliberately uses identities,
// not display names/emails, and length-prefixes each member so distinct lists
// cannot collide by concatenation. Application's future room/case resolver
// uses the same function only after Control has authorized each member.
func RecipientAudienceHash(recipientSubjectIDs ...string) (string, error) {
	if len(recipientSubjectIDs) == 0 {
		return "", fmt.Errorf("recipient audience must not be empty")
	}
	canonical := make([]string, 0, len(recipientSubjectIDs))
	seen := make(map[string]struct{}, len(recipientSubjectIDs))
	for _, subjectID := range recipientSubjectIDs {
		subjectID = strings.TrimSpace(subjectID)
		if subjectID == "" {
			return "", fmt.Errorf("recipient audience contains an empty subject")
		}
		if _, exists := seen[subjectID]; exists {
			return "", fmt.Errorf("recipient audience contains a duplicate subject")
		}
		seen[subjectID] = struct{}{}
		canonical = append(canonical, subjectID)
	}
	sort.Strings(canonical)
	hash := sha256.New()
	hash.Write([]byte("recipient-audience/v1\x00"))
	for _, subjectID := range canonical {
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(subjectID)))
		hash.Write(length[:])
		hash.Write([]byte(subjectID))
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil)), nil
}

func (p PrivacyPolicySnapshot) Validate() error {
	for label, value := range map[string]string{
		"privacy_policy_ref": p.PolicyRef,
		"purpose":            p.Purpose,
		"lawful_basis":       p.LawfulBasis,
		"privacy_class":      p.PrivacyClass,
		"retention_class":    p.RetentionClass,
		"residency":          p.Residency,
		"deletion_scope":     p.DeletionScope,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space privacy policy %s is required", label)
		}
	}
	return nil
}

// PersonalThreadDecisionEvidence is assembled only from authoritative Control
// resolvers. For this first vertical, a personal Space has exactly its acting
// member as recipient. Linked documents, connectors, and shared Spaces have
// different owners/audiences and must use their own issuer path.
type PersonalThreadDecisionEvidence struct {
	Membership                CurrentMembership
	RecipientAudienceRef      string
	RecipientAudienceHash     string
	RecipientSubjectID        string
	Privacy                   PrivacyPolicySnapshot
	ThreadCreateEntitled      bool
	ThreadReadEntitled        bool
	RetrievalReadEntitled     bool
	ImportWriteEntitled       bool
	AgentActionEntitled       bool
	ScheduleFireEntitled      bool
	SandboxCapabilityEntitled bool
	ProcessRegistryEntitled   bool
	ResourceAuthorizationRef  string
}

func (e PersonalThreadDecisionEvidence) validatePersonalAuthority() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind != KindPersonal {
		return fmt.Errorf("personal decision requires a personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "viewer", "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot access a personal Space", e.Membership.Role)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" || e.RecipientSubjectID != e.Membership.SubjectID {
		return fmt.Errorf("personal Space recipient audience must be exactly the current subject")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
		return fmt.Errorf("personal Space resource authorization is required")
	}
	return e.Privacy.Validate()
}

func (e PersonalThreadDecisionEvidence) Validate() error {
	if err := e.validatePersonalAuthority(); err != nil {
		return err
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot create a thread", e.Membership.Role)
	}
	if !e.ThreadCreateEntitled {
		return fmt.Errorf("thread creation entitlement is not active")
	}
	return nil
}

// ValidateForSharedThread keeps the existing evidence shape reusable while
// making its actor/audience rules explicit. Unlike a personal Space, the
// recipient set is a Control-acknowledged Application snapshot and the actor
// must be one member of it; the actor is not treated as the sole recipient.
func (e PersonalThreadDecisionEvidence) ValidateForSharedThread() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return fmt.Errorf("shared thread decision requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot create a shared thread", e.Membership.Role)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return fmt.Errorf("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" || !e.ThreadCreateEntitled {
		return fmt.Errorf("shared Space thread authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateForSharedRetrieval mirrors ValidateForSharedThread's actor/audience
// rules but for a read effect: any active member (not only editor/manager/
// owner) may retrieve, matching the personal-Space retrieval role floor in
// validatePersonalAuthority.
func (e PersonalThreadDecisionEvidence) ValidateForSharedRetrieval() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return fmt.Errorf("shared retrieval decision requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "viewer", "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot retrieve in a shared Space", e.Membership.Role)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return fmt.Errorf("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" || !e.RetrievalReadEntitled {
		return fmt.Errorf("shared Space retrieval authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateForSharedThreadRead authorizes reading a shared Space's whole
// conversation record — every member's turns, not only the caller's own.
//
// The role floor is `viewer`, matching shared retrieval: reading the room is
// what a viewer is for. What it does NOT relax is the audience join. Control
// resolves this evidence only when the caller appears in the Space's CURRENT
// recipient audience, so a removed participant stops being able to read the
// moment their membership or the audience revision changes. The reading
// service never needs its own copy of the participant list.
func (e PersonalThreadDecisionEvidence) ValidateForSharedThreadRead() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	if e.Membership.Kind == KindPersonal {
		return fmt.Errorf("shared thread read decision requires a non-personal Space")
	}
	if !matchesOneOf(e.Membership.Role, "viewer", "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot read a shared Space", e.Membership.Role)
	}
	if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
		return fmt.Errorf("shared Space recipient audience is required")
	}
	if strings.TrimSpace(e.ResourceAuthorizationRef) == "" || !e.ThreadReadEntitled {
		return fmt.Errorf("shared Space thread read authority is incomplete")
	}
	return e.Privacy.Validate()
}

// ValidateForRetrieval keeps retrieval authorization distinct from durable
// thread creation. A deployment must opt into both effect classes explicitly.
func (e PersonalThreadDecisionEvidence) ValidateForRetrieval() error {
	if err := e.validatePersonalAuthority(); err != nil {
		return err
	}
	if !e.RetrievalReadEntitled {
		return fmt.Errorf("retrieval entitlement is not active")
	}
	return nil
}

// ValidateForImport is intentionally separate from thread creation and
// retrieval. An import persists external content and therefore needs its own
// explicit entitlement before an Ingestion worker can receive any authority.
func (e PersonalThreadDecisionEvidence) ValidateForImport() error {
	if err := e.validatePersonalAuthority(); err != nil {
		return err
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot import content", e.Membership.Role)
	}
	if !e.ImportWriteEntitled {
		return fmt.Errorf("import entitlement is not active")
	}
	return nil
}

// ValidateForAgentAction is intentionally independent of a thread-create
// decision. A Model run can request a target-specific owner action only when
// the current Control policy permits it; the target owner must still authorize
// its own resource immediately before the effect.
func (e PersonalThreadDecisionEvidence) ValidateForAgentAction() error {
	if e.Membership.Kind == KindPersonal {
		if err := e.validatePersonalAuthority(); err != nil {
			return err
		}
	} else if err := e.ValidateForSharedThread(); err != nil {
		return err
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return fmt.Errorf("space role %q cannot request an agent action", e.Membership.Role)
	}
	if !e.AgentActionEntitled {
		return fmt.Errorf("agent action entitlement is not active")
	}
	if e.Privacy.ZeroDataRetention {
		return fmt.Errorf("zero data retention forbids durable owner actions")
	}
	return nil
}

// PersonalThreadDecisionRequest contains operation-bound fields received from
// the verified gateway path. The issuer accepts no actor, organization, role,
// recipient, or policy selected by a browser/request body.
type PersonalThreadDecisionRequest struct {
	DecisionRef    string
	SessionKey     string
	IdempotencyKey string
	Nonce          string
}

// ThreadAppendDecisionRequest contains no content. The BFF provides only an
// SHA-256 commitment to the exact message bytes; Control binds it into the
// decision while Session Core recomputes the commitment before persistence.
// This preserves ZDR minimization while avoiding a browser-chosen authority
// digest.
type ThreadAppendDecisionRequest struct {
	DecisionRef    string
	ThreadID       string
	ContentDigest  string
	IdempotencyKey string
	Nonce          string
}

func (r ThreadAppendDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "thread_id": r.ThreadID,
		"content_digest": r.ContentDigest, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("thread append decision %s is required", label)
		}
	}
	if !strings.HasPrefix(r.ContentDigest, "sha256:") || len(r.ContentDigest) != len("sha256:")+64 {
		return fmt.Errorf("thread append content digest is invalid")
	}
	return nil
}

func threadAppendPayloadDigest(evidence PersonalThreadDecisionEvidence, request ThreadAppendDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.thread.append\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"thread_id", request.ThreadID}, {"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"content_digest", request.ContentDigest},
		{"action_schema_hash", threadAppendSchema}, {"idempotency_key", request.IdempotencyKey},
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

// IssueThreadAppendDecision is target-specific and intentionally distinct
// from creation: an old thread:create token cannot authorize a new message
// after a recipient audience has changed.
func IssueThreadAppendDecision(evidence PersonalThreadDecisionEvidence, request ThreadAppendDecisionRequest, now time.Time) (Decision, error) {
	if evidence.Membership.Kind == KindPersonal {
		if err := evidence.Validate(); err != nil {
			return Decision{}, err
		}
	} else if err := evidence.ValidateForSharedThread(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("thread append decision request is invalid")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, personalThreadCreateAudience, threadAppendAction, threadAppendSchema,
		threadAppendPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"thread:append"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}

func (r PersonalThreadDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref":    r.DecisionRef,
		"idempotency_key": r.IdempotencyKey,
		"nonce":           r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("personal thread decision %s is required", label)
		}
	}
	return nil
}

// personalThreadCreatePayloadDigest is the shared effect encoding that the
// Model Plane verifies before it persists a scoped thread. The gateway cannot
// provide a digest: Control derives it from authenticated Control evidence and
// the one client-selected effect field (session key). Every field is length
// prefixed so concatenation remains unambiguous.
func personalThreadCreatePayloadDigest(evidence PersonalThreadDecisionEvidence, request PersonalThreadDecisionRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.thread.create\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"session_key", request.SessionKey},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", personalThreadCreateSchema},
		{"idempotency_key", request.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	hash.Write([]byte("authority_revision\x00"))
	var revision [8]byte
	binary.BigEndian.PutUint64(revision[:], uint64(evidence.Membership.Revisions.Authority))
	hash.Write(revision[:])
	hash.Write([]byte("recipient_audience_revision\x00"))
	binary.BigEndian.PutUint64(revision[:], uint64(evidence.Membership.Revisions.RecipientAudience))
	hash.Write(revision[:])
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssuePersonalThreadCreateDecision constructs the only first-slice decision
// that Control can fully resolve today. The returned value still has to be
// signed by Control's deployment-managed SigningKey before it crosses planes.
func IssuePersonalThreadCreateDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalThreadDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.Validate(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("personal thread decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, personalThreadCreateAudience, personalThreadCreateAction, personalThreadCreateSchema,
		personalThreadCreatePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"thread:create"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}

// IssueSharedThreadCreateDecision uses the same exact Model effect digest as
// personal creation, but only after Control has resolved a current shared
// recipient audience. It deliberately accepts no caller-selected recipient
// fields; those come from the registered audience snapshot in Repository.
func IssueSharedThreadCreateDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalThreadDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedThread(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("shared thread decision request is invalid")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, personalThreadCreateAudience, personalThreadCreateAction, personalThreadCreateSchema,
		personalThreadCreatePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"thread:create"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}

func matchesOneOf(value string, allowed ...string) bool {
	return slices.Contains(allowed, value)
}

// digestRevision is one named integer bound into a payload digest.
type digestRevision struct {
	name  string
	value int64
}

// writeDigestRevisions binds revisions into a payload digest as name, NUL
// separator and 8-byte big-endian value so every issuer commits to the same
// encoding.
func writeDigestRevisions(h hash.Hash, revisions ...digestRevision) {
	for _, revision := range revisions {
		h.Write([]byte(revision.name))
		h.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		h.Write(encoded[:])
	}
}

// newEvidenceDecision assembles the common evidence-derived fields of every
// short-lived Space decision so each issuer only binds its operation-specific
// action, audience, payload digest, and permissions.
func newEvidenceDecision(
	evidence PersonalThreadDecisionEvidence,
	decisionRef, audience, actionID, schema, payloadDigest, idempotencyKey, nonce string,
	permissions []string,
	zeroDataRetention bool,
	now time.Time,
) Decision {
	return Decision{
		DecisionRef:               strings.TrimSpace(decisionRef),
		OrgID:                     evidence.Membership.OrgID,
		SpaceRef:                  evidence.Membership.SpaceRef,
		SubjectID:                 evidence.Membership.SubjectID,
		ServiceAudience:           audience,
		ActionID:                  actionID,
		ActionSchemaHash:          schema,
		PayloadDigest:             strings.TrimSpace(payloadDigest),
		IdempotencyKey:            strings.TrimSpace(idempotencyKey),
		RecipientAudienceRef:      strings.TrimSpace(evidence.RecipientAudienceRef),
		RecipientAudienceHash:     strings.TrimSpace(evidence.RecipientAudienceHash),
		PrivacyPolicyRef:          strings.TrimSpace(evidence.Privacy.PolicyRef),
		ResourceAuthorizationRef:  strings.TrimSpace(evidence.ResourceAuthorizationRef),
		AuthorityRevision:         evidence.Membership.Revisions.Authority,
		MembershipRevision:        evidence.Membership.Revisions.Membership,
		PrivacyRevision:           evidence.Membership.Revisions.Privacy,
		RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		EntitlementRevision:       evidence.Membership.Revisions.Entitlement,
		Permissions:               permissions,
		Purpose:                   strings.TrimSpace(evidence.Privacy.Purpose),
		LawfulBasis:               strings.TrimSpace(evidence.Privacy.LawfulBasis),
		PrivacyClass:              strings.TrimSpace(evidence.Privacy.PrivacyClass),
		ThirdPartyAllowed:         evidence.Privacy.ThirdPartyAllowed,
		RetentionClass:            strings.TrimSpace(evidence.Privacy.RetentionClass),
		Residency:                 strings.TrimSpace(evidence.Privacy.Residency),
		DeletionScope:             strings.TrimSpace(evidence.Privacy.DeletionScope),
		ZeroDataRetention:         zeroDataRetention,
		IssuedAt:                  now.UTC(),
		ExpiresAt:                 now.UTC().Add(personalDecisionLifetime),
		Nonce:                     strings.TrimSpace(nonce),
	}
}
