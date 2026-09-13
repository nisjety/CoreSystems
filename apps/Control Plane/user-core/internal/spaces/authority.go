// Package spaces defines Control Plane's registered-Space authority contract.
//
// Application owns a Space's identity and lifecycle projection. Control records
// that immutable reference, owns membership/privacy/audience revisions, and
// later issues signed access decisions from this state. This package never
// treats an Application event or a browser request as authorization by itself.
package spaces

import (
	"fmt"
	"strings"
)

type Kind string

const (
	KindPersonal Kind = "personal"
	KindRoom     Kind = "room"
	KindProject  Kind = "project"
	KindCase     Kind = "case"
)

// SpaceLifecycle is Application's canonical lifecycle vocabulary. Control
// records the mapped access state, but never lets an unrecognized producer
// value become an authorization state.
type SpaceLifecycle string

const (
	LifecyclePendingRegistration SpaceLifecycle = "pending_registration"
	LifecycleActive              SpaceLifecycle = "active"
	LifecycleSuspended           SpaceLifecycle = "suspended"
	LifecycleDeleting            SpaceLifecycle = "deleting"
	LifecycleDeleted             SpaceLifecycle = "deleted"
	LifecycleFailedRegistration  SpaceLifecycle = "failed_registration"
)

// Registration is the Application-issued immutable reference that Control
// accepts only through its future authenticated registration endpoint.
type Registration struct {
	SpaceRef          string         `json:"space_ref"`
	OrgID             string         `json:"org_id"`
	OwnerPrincipalID  string         `json:"owner_principal_id"`
	Kind              Kind           `json:"kind"`
	Lifecycle         SpaceLifecycle `json:"lifecycle"`
	LifecycleRevision int64          `json:"lifecycle_revision"`
}

// RegistrationState maps an Application lifecycle event into Control's
// fail-closed authorization state. Pending registration is active only after
// Control has accepted the immutable reference; failed registration cannot
// revoke a previously registered Space by itself.
func (r Registration) RegistrationState() (string, error) {
	switch r.Lifecycle {
	case LifecyclePendingRegistration, LifecycleActive:
		return "active", nil
	case LifecycleSuspended:
		return "suspended", nil
	case LifecycleDeleting:
		return "deleting", nil
	case LifecycleDeleted:
		return "deleted", nil
	case LifecycleFailedRegistration:
		return "active", nil
	default:
		return "", fmt.Errorf("unknown Space lifecycle %q", r.Lifecycle)
	}
}

// RecipientAudienceRegistration is Application's product-level participant
// snapshot. Control recomputes the hash and checks every principal against
// current Space/org membership before it records the snapshot; this request
// itself is never an authority decision or browser payload.
type RecipientAudienceRegistration struct {
	SpaceRef     string   `json:"space_ref"`
	OrgID        string   `json:"org_id"`
	AudienceRef  string   `json:"audience_ref"`
	AudienceHash string   `json:"audience_hash"`
	Revision     int64    `json:"revision"`
	Recipients   []string `json:"recipients"`
}

func (r RecipientAudienceRegistration) Validate() error {
	for name, value := range map[string]string{
		"space_ref": r.SpaceRef, "org_id": r.OrgID,
		"audience_ref": r.AudienceRef, "audience_hash": r.AudienceHash,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("recipient audience %s is required", name)
		}
	}
	if r.Revision <= 0 || len(r.Recipients) == 0 || len(r.Recipients) > 512 {
		return fmt.Errorf("recipient audience revision or recipient count is invalid")
	}
	hash, err := RecipientAudienceHash(r.Recipients...)
	if err != nil {
		return err
	}
	if r.AudienceHash != hash {
		return fmt.Errorf("recipient audience hash does not match recipients")
	}
	return nil
}

func (r Registration) Validate() error {
	if strings.TrimSpace(r.SpaceRef) == "" || strings.TrimSpace(r.OrgID) == "" || strings.TrimSpace(r.OwnerPrincipalID) == "" {
		return fmt.Errorf("Space registration identity is required")
	}
	switch r.Kind {
	case KindPersonal, KindRoom, KindProject, KindCase:
	default:
		return fmt.Errorf("unknown Space kind %q", r.Kind)
	}
	if r.LifecycleRevision <= 0 {
		return fmt.Errorf("Space lifecycle revision must be positive")
	}
	if _, err := r.RegistrationState(); err != nil {
		return err
	}
	return nil
}

type AuthorityChange string

const (
	ChangeMembership        AuthorityChange = "membership"
	ChangePrivacy           AuthorityChange = "privacy"
	ChangeRecipientAudience AuthorityChange = "recipient_audience"
	ChangeEntitlement       AuthorityChange = "entitlement"
)

// AuthorityRevision travels in every future Space access decision. Authority
// always advances for any effective-access change; the component revision
// pinpoints why cached or resumed work must be rejected.
type AuthorityRevision struct {
	Authority         int64 `json:"authority_revision"`
	Membership        int64 `json:"membership_revision"`
	Privacy           int64 `json:"privacy_revision"`
	RecipientAudience int64 `json:"recipient_audience_revision"`
	Entitlement       int64 `json:"entitlement_revision"`
}

// EffectPolicy is Control's processing and entitlement floor for the first
// scoped Model effect. It is written only by a dedicated Control workload; it
// is never reconstructed from a browser request or a Space projection.
type EffectPolicy struct {
	OrgID                       string `json:"org_id"`
	PrivacyPolicyRef            string `json:"privacy_policy_ref"`
	Purpose                     string `json:"purpose"`
	LawfulBasis                 string `json:"lawful_basis"`
	PrivacyClass                string `json:"privacy_class"`
	ThirdPartyProcessingAllowed bool   `json:"third_party_processing_allowed"`
	RetentionClass              string `json:"retention_class"`
	Residency                   string `json:"residency"`
	DeletionScope               string `json:"deletion_scope"`
	ZeroDataRetention           bool   `json:"zero_data_retention"`
	ThreadCreateEntitled        bool   `json:"thread_create_entitled"`
	RetrievalReadEntitled       bool   `json:"retrieval_read_entitled"`
	ImportWriteEntitled         bool   `json:"import_write_entitled"`
	// AgentActionEntitled is a separate, deny-by-default floor for a Model run
	// to request a target-specific owner action. It does not authorize an
	// owner-plane resource; that owner rechecks its resource at effect time.
	AgentActionEntitled bool `json:"agent_action_entitled"`
	// ScheduleFireEntitled is deliberately separate from thread creation. A
	// recurring effect must be explicitly allowed at *each* fire; a schedule
	// cannot inherit an old chat/creation entitlement.
	ScheduleFireEntitled bool `json:"schedule_fire_entitled"`
	// ThreadReadEntitled admits a current member/recipient of a SHARED Space to
	// that Space's whole conversation record, not only the threads they
	// themselves own. That is a disclosure of other people's turns, so it is
	// its own deny-by-default class and never inherited from
	// ThreadCreateEntitled: being allowed to speak in a room is not the same
	// permission as reading what everyone else said in it.
	ThreadReadEntitled bool `json:"thread_read_entitled"`
	// SandboxCapabilityEntitled is a standing, deny-by-default floor for a
	// Space to have its agents acquire an execution sandbox lease at all. It
	// is independent of every other entitlement here: a Space able to create
	// threads, fire schedules, or read history must not automatically gain
	// its own agents' compute. See
	// apps/Frontend Plane/verevonv3/docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md.
	SandboxCapabilityEntitled bool `json:"sandbox_capability_entitled"`
	// ProcessRegistryEntitled is deliberately separate from
	// SandboxCapabilityEntitled, for the same reason ScheduleFireEntitled is
	// separate from ThreadCreateEntitled: acquiring a sandbox is bounded work
	// a caller waits on, while a background process keeps running after the
	// turn that started it, retains its own output, and can be reattached to
	// later. A Space allowed the first is not thereby allowed the second. See
	// apps/Frontend Plane/verevonv3/docs/S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md §4.
	ProcessRegistryEntitled bool `json:"process_registry_entitled"`
}

func (p EffectPolicy) Validate() error {
	for name, value := range map[string]string{
		"org_id": p.OrgID, "privacy_policy_ref": p.PrivacyPolicyRef,
		"purpose": p.Purpose, "lawful_basis": p.LawfulBasis,
		"privacy_class": p.PrivacyClass, "retention_class": p.RetentionClass,
		"residency": p.Residency, "deletion_scope": p.DeletionScope,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("Space effect policy %s is required", name)
		}
	}
	return nil
}

// CurrentMembership is a current Control-owned membership fact. It is useful
// to a resolver only as one component of effective access: callers must still
// intersect recipient, policy, and owner-resource authorization before an
// effect. The database result is deliberately not a signed access decision.
// RosterMember is one participant of a Space as shown to another participant.
//
// `SubjectType` distinguishes a person from a service/agent identity, so the UI
// can say which is which instead of implying every row is a colleague.
// `DisplayName` may be empty — a membership can exist before its user
// projection does, and an empty name is more honest than substituting the
// opaque id as though it were one.
type RosterMember struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	Role        string `json:"role"`
	Revision    int64  `json:"revision"`
	DisplayName string `json:"display_name"`
}

// SpaceIndexEntry is one Space a subject may see, with the role they hold in
// it. Deliberately narrow: an index answers "which rooms are mine and what am
// I in them", and anything more — audiences, policies, decisions — belongs to
// the per-Space reads that check authority again for that specific use.
type SpaceIndexEntry struct {
	SpaceRef string `json:"space_ref"`
	OrgID    string `json:"org_id"`
	Kind     Kind   `json:"kind"`
	Role     string `json:"role"`
}

// MemberGrant is one subject's place in a Space. `service` covers agent and
// workload identities, which the product model allows as Space members but
// never infers — an identity is a member because it was granted, not because
// it appeared in an action catalog.
type MemberGrant struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	Role        string `json:"role"`
}

// MembershipReplacement declares the complete intended membership of a Space.
//
// Declarative rather than add/remove, because the caller that knows the answer
// is the one holding the source roster (an organization's active users). An
// incremental API would make the two drift the moment a single call is lost;
// a full set converges on every call.
//
// Absence therefore means revocation. That is the point, and it is also the
// sharp edge — see Repository.ReplaceMemberships for the one subject it
// refuses to revoke.
type MembershipReplacement struct {
	SpaceRef string        `json:"space_ref"`
	Members  []MemberGrant `json:"members"`
	// Which subject kinds this replacement speaks for. Convergence deactivates
	// undeclared members ONLY within these kinds.
	//
	// Absent means "this is the entire roster", which is what a caller that owns
	// every subject kind wants. But the organization-roster sync is not such a
	// caller: org-core knows people and nothing else, so an unscoped replacement
	// from it revokes every agent bound to the room as a side effect of a human
	// roster converging. That is the same shape as the owner-demotion bug fixed
	// above — a declarative sync from a source that only knows part of the
	// truth, applied as if it knew all of it.
	//
	// Callers that manage one kind must say so: `["user"]`.
	ManagedSubjectTypes []string `json:"managed_subject_types,omitempty"`
}

// managedSubjectTypeSet resolves the kinds this replacement may deactivate.
// Nil means every kind.
func (m MembershipReplacement) managedSubjectTypeSet() map[string]struct{} {
	if len(m.ManagedSubjectTypes) == 0 {
		return nil
	}
	managed := make(map[string]struct{}, len(m.ManagedSubjectTypes))
	for _, subjectType := range m.ManagedSubjectTypes {
		managed[strings.TrimSpace(subjectType)] = struct{}{}
	}
	return managed
}

const maxSpaceMembers = 5000

func (m MembershipReplacement) Validate() error {
	if strings.TrimSpace(m.SpaceRef) == "" {
		return fmt.Errorf("Space reference is required")
	}
	if len(m.Members) > maxSpaceMembers {
		return fmt.Errorf("Space membership exceeds %d subjects", maxSpaceMembers)
	}
	seen := make(map[string]struct{}, len(m.Members))
	for _, member := range m.Members {
		subjectType := strings.TrimSpace(member.SubjectType)
		subjectID := strings.TrimSpace(member.SubjectID)
		role := strings.TrimSpace(member.Role)
		if subjectType != "user" && subjectType != "service" {
			return fmt.Errorf("unknown Space member subject type %q", member.SubjectType)
		}
		if subjectID == "" {
			return fmt.Errorf("Space member subject id is required")
		}
		switch role {
		case "viewer", "editor", "manager", "owner":
		default:
			return fmt.Errorf("unknown Space member role %q", member.Role)
		}
		key := subjectType + "\x00" + subjectID
		if _, duplicate := seen[key]; duplicate {
			// Two rows for one subject would make the resulting role depend on
			// iteration order, so the caller must resolve it rather than us.
			return fmt.Errorf("duplicate Space member %s:%s", subjectType, subjectID)
		}
		seen[key] = struct{}{}
	}
	managed := m.managedSubjectTypeSet()
	for _, subjectType := range m.ManagedSubjectTypes {
		if trimmed := strings.TrimSpace(subjectType); trimmed != "user" && trimmed != "service" {
			return fmt.Errorf("unknown managed subject type %q", subjectType)
		}
	}
	if managed != nil {
		// Declaring a member of a kind you do not manage would insert a row that
		// the very same call refuses to converge, so the roster would drift by
		// design. Rejecting it keeps the scope honest.
		for _, member := range m.Members {
			subjectType := strings.TrimSpace(member.SubjectType)
			if _, ok := managed[subjectType]; !ok {
				return fmt.Errorf(
					"member subject type %q is outside the declared managed scope",
					subjectType,
				)
			}
		}
	}
	return nil
}

type CurrentMembership struct {
	SpaceRef  string            `json:"space_ref"`
	OrgID     string            `json:"org_id"`
	SubjectID string            `json:"subject_id"`
	Kind      Kind              `json:"kind"`
	Role      string            `json:"role"`
	Revisions AuthorityRevision `json:"revisions"`
}

func (m CurrentMembership) Validate() error {
	if strings.TrimSpace(m.SpaceRef) == "" || strings.TrimSpace(m.OrgID) == "" || strings.TrimSpace(m.SubjectID) == "" {
		return fmt.Errorf("current Space membership identity is required")
	}
	switch m.Kind {
	case KindPersonal, KindRoom, KindProject, KindCase:
	default:
		return fmt.Errorf("unknown Space kind %q", m.Kind)
	}
	switch m.Role {
	case "viewer", "editor", "manager", "owner":
	default:
		return fmt.Errorf("unknown Space membership role %q", m.Role)
	}
	return m.Revisions.Validate()
}

func (r AuthorityRevision) Validate() error {
	if r.Authority <= 0 || r.Membership <= 0 || r.Privacy <= 0 || r.RecipientAudience <= 0 || r.Entitlement <= 0 {
		return fmt.Errorf("Space authority revisions must be positive")
	}
	return nil
}

func (r AuthorityRevision) Advance(change AuthorityChange) (AuthorityRevision, error) {
	if err := r.Validate(); err != nil {
		return AuthorityRevision{}, err
	}
	next := r
	next.Authority++
	switch change {
	case ChangeMembership:
		next.Membership++
	case ChangePrivacy:
		next.Privacy++
	case ChangeRecipientAudience:
		next.RecipientAudience++
	case ChangeEntitlement:
		next.Entitlement++
	default:
		return AuthorityRevision{}, fmt.Errorf("unknown Space authority change %q", change)
	}
	return next, nil
}
