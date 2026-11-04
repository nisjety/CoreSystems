// Package spaces defines Control Plane's registered-Space authority contract.
//
// Application owns a Space's identity and lifecycle projection. Control records
// that immutable reference, owns membership/privacy/audience revisions, and
// later issues signed access decisions from this state. This package never
// treats an Application event or a browser request as authorization by itself.
package spaces

import (
	"fmt"
	"net"
	"sort"
	"strings"
	"time"
)

type Kind string

const (
	KindPersonal Kind = "personal"
	KindRoom     Kind = "room"
	KindProject  Kind = "project"
	KindCase     Kind = "case"
	// KindDM is a direct message between exactly two people and KindGroupDM one
	// between three and MaxDMParticipants (WS12-S1). Both have a roster fixed at
	// registration, humans only, and are identified by participant_hash.
	KindDM      Kind = "dm"
	KindGroupDM Kind = "group_dm"
)

// IsDM reports whether the kind is one of the two direct-message kinds.
func (k Kind) IsDM() bool { return k == KindDM || k == KindGroupDM }

// Agent scopes a Space kind implies. Control owns this fact (WS12 D1.1): a
// room-like Space shares its agent with the room, a personal Space keeps a
// personal agent, and a DM carries no agent at all.
const (
	AgentScopePersonal = "personal"
	AgentScopeRoom     = "room"
	AgentScopeNone     = "none"
)

// AgentScope derives the agent scope from the kind. An unknown kind has none:
// no permissive default.
func (k Kind) AgentScope() string {
	switch k {
	case KindPersonal:
		return AgentScopePersonal
	case KindRoom, KindProject, KindCase:
		return AgentScopeRoom
	default:
		return AgentScopeNone
	}
}

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
	// Participants and ParticipantHash travel only for the DM kinds (WS12-S1):
	// the complete, fixed roster and its content-free commitment. Validate
	// requires both for a DM kind, neither for any other, and recomputes the
	// hash, so an Application payload cannot register a roster it did not hash.
	Participants    []string `json:"participants,omitempty"`
	ParticipantHash string   `json:"participant_hash,omitempty"`
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
		return fmt.Errorf("space registration identity is required")
	}
	switch r.Kind {
	case KindPersonal, KindRoom, KindProject, KindCase, KindDM, KindGroupDM:
	default:
		return fmt.Errorf("unknown Space kind %q", r.Kind)
	}
	if r.LifecycleRevision <= 0 {
		return fmt.Errorf("space lifecycle revision must be positive")
	}
	if _, err := r.RegistrationState(); err != nil {
		return err
	}
	return r.validateParticipants()
}

// validateParticipants enforces the DM roster contract: DM kinds carry 2 to 9
// distinct participants including the owner and a hash that recomputes equal;
// every other kind carries neither field.
func (r Registration) validateParticipants() error {
	if !r.Kind.IsDM() {
		if len(r.Participants) != 0 || strings.TrimSpace(r.ParticipantHash) != "" {
			return fmt.Errorf("participants are only valid for direct-message Spaces")
		}
		return nil
	}
	canonical, err := CanonicalParticipants(r.Participants)
	if err != nil {
		return err
	}
	if kind, _ := DMKindForCount(len(canonical)); kind != r.Kind {
		return &DMParticipantInvalidError{Reason: DMReasonCount}
	}
	owner := strings.TrimSpace(r.OwnerPrincipalID)
	if i := sort.SearchStrings(canonical, owner); i >= len(canonical) || canonical[i] != owner {
		return &DMParticipantInvalidError{Reason: DMReasonOwnerMissing}
	}
	hash, err := ParticipantSetHash(canonical...)
	if err != nil {
		return err
	}
	if r.ParticipantHash != hash {
		return fmt.Errorf("participant hash does not match participants")
	}
	return nil
}

// Roster modes of a registered Space (WS04-S4). "managed" rooms take Control
// writes; "org_derived" is the organization room, converged by the
// declarative sync and refused by the Control writer.
const (
	RosterModeManaged    = "managed"
	RosterModeOrgDerived = "org_derived"
)

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
	// RunControlEntitled (WS09-S7) lets a room manager or owner cancel another
	// member's live run. Stopping someone else's turn is a different power from
	// speaking or reading in the room, so it is its own deny-by-default floor
	// and is never inherited from ThreadCreateEntitled or ThreadReadEntitled.
	RunControlEntitled bool `json:"run_control_entitled"`
	// ApprovalGrantEntitled (WS10-S2) lets a room manager record a standing
	// approval rule and lets the rule pass a prompt. A rule waives a prompt, not
	// authority, so it is its own deny-by-default floor and is never inherited
	// from AgentActionEntitled or ThreadCreateEntitled.
	ApprovalGrantEntitled bool `json:"approval_grant_entitled"`
	// DMCreateEntitled (WS12-S1) lets a member open a direct message. A DM also
	// needs ThreadCreateEntitled and ThreadReadEntitled; this bit is its own
	// deny-by-default floor and is never inherited from either.
	DMCreateEntitled bool `json:"dm_create_entitled"`
	// MessageReviseEntitled (WS12-S5) lets a member edit or delete their own
	// message and retract an agent reply they started. MessageModerateEntitled
	// additionally lets a room manager or owner delete another member's
	// message. Both are their own deny-by-default floors, never inherited from
	// ThreadCreateEntitled, and both are inert under Zero Data Retention.
	MessageReviseEntitled   bool `json:"message_revise_entitled"`
	MessageModerateEntitled bool `json:"message_moderate_entitled"`
}

func (p EffectPolicy) Validate() error {
	for name, value := range map[string]string{
		"org_id": p.OrgID, "privacy_policy_ref": p.PrivacyPolicyRef,
		"purpose": p.Purpose, "lawful_basis": p.LawfulBasis,
		"privacy_class": p.PrivacyClass, "retention_class": p.RetentionClass,
		"residency": p.Residency, "deletion_scope": p.DeletionScope,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space effect policy %s is required", name)
		}
	}
	return nil
}

// Sandbox egress tiers (EP-9 E2, decision D-E3). The vocabulary is closed and
// ordered: a higher tier reaches strictly more of the network. Control signs
// at most min(claim, org ceiling), and nothing above off under Zero Data
// Retention. Mirrored by sandbox-manager's authz egress tiers and, from E3, by
// execution-core; the spelling is bound into the capability decision.
const (
	SandboxEgressOff        = "off"
	SandboxEgressRegistries = "registries"
	SandboxEgressAllowlist  = "allowlist"
	// SandboxEgressAll is in the vocabulary so a backend can state what it can
	// enforce, but it is not storable in W3: no org ceiling can be all, so no
	// decision ever grants it.
	SandboxEgressAll = "all"
)

// maxSandboxEgressAllowlistHosts bounds an org's allow-list. Migration 044
// enforces the same cap as a backstop for a writer that bypasses this package.
const maxSandboxEgressAllowlistHosts = 32

// sandboxEgressRank orders the tiers. A value missing here is not a tier;
// callers treat it as off (unknown means off), never as a wider reach.
var sandboxEgressRank = map[string]int{
	SandboxEgressOff: 0, SandboxEgressRegistries: 1, SandboxEgressAllowlist: 2, SandboxEgressAll: 3,
}

// SandboxEgressPolicy is an org's sandbox egress ceiling, stored beside the
// effect policy (space_effect_policies.sandbox_egress_tier and
// sandbox_egress_allowlist). It is a separate type rather than EffectPolicy
// fields because EffectPolicy is compared with == by the effect-policy writer
// and a slice would make it incomparable. Host names only, never content.
type SandboxEgressPolicy struct {
	Tier      string   `json:"sandbox_egress_tier"`
	Allowlist []string `json:"sandbox_egress_allowlist"`
}

// Normalize validates a policy for storage and returns it with the allow-list
// in canonical form (lower-case, de-duplicated, sorted). It refuses all (not
// storable in W3) and every value outside the vocabulary, so a typo is an
// error rather than a silent off. An allow-list may be stored under a lower
// tier; it is only signed into a decision whose granted tier is allowlist.
func (p SandboxEgressPolicy) Normalize() (SandboxEgressPolicy, error) {
	tier := strings.TrimSpace(p.Tier)
	switch tier {
	case SandboxEgressOff, SandboxEgressRegistries, SandboxEgressAllowlist:
	case SandboxEgressAll:
		return SandboxEgressPolicy{}, fmt.Errorf("sandbox egress tier %q is not storable", tier)
	default:
		return SandboxEgressPolicy{}, fmt.Errorf("unknown sandbox egress tier %q", p.Tier)
	}
	if len(p.Allowlist) > maxSandboxEgressAllowlistHosts {
		return SandboxEgressPolicy{}, fmt.Errorf("sandbox egress allow-list exceeds %d hosts", maxSandboxEgressAllowlistHosts)
	}
	hosts := make([]string, 0, len(p.Allowlist))
	seen := make(map[string]struct{}, len(p.Allowlist))
	for _, raw := range p.Allowlist {
		host, err := CanonicalEgressHost(raw)
		if err != nil {
			return SandboxEgressPolicy{}, err
		}
		if _, duplicate := seen[host]; duplicate {
			continue
		}
		seen[host] = struct{}{}
		hosts = append(hosts, host)
	}
	sort.Strings(hosts)
	return SandboxEgressPolicy{Tier: tier, Allowlist: hosts}, nil
}

// CanonicalEgressHost returns the canonical spelling of one allow-list host:
// a lower-case DNS name of at least two labels, with no scheme, port, path,
// wildcard or IP literal. IP literals are refused because an allow-list entry
// must name a host the egress proxy resolves and checks itself (E3 refuses
// private and metadata addresses after resolution); a literal would bypass
// that check. Internationalized names must be given in punycode.
func CanonicalEgressHost(raw string) (string, error) {
	host := strings.TrimSuffix(strings.ToLower(strings.TrimSpace(raw)), ".")
	if host == "" || len(host) > 253 {
		return "", fmt.Errorf("sandbox egress host %q is not a valid host name", raw)
	}
	if net.ParseIP(host) != nil {
		return "", fmt.Errorf("sandbox egress host %q is an IP literal", raw)
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return "", fmt.Errorf("sandbox egress host %q is not a fully qualified host name", raw)
	}
	for _, label := range labels {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return "", fmt.Errorf("sandbox egress host %q is not a valid host name", raw)
		}
		for _, ch := range label {
			if (ch < 'a' || ch > 'z') && (ch < '0' || ch > '9') && ch != '-' {
				return "", fmt.Errorf("sandbox egress host %q is not a valid host name", raw)
			}
		}
	}
	// An all-digit last label is a numeric address in another spelling
	// (for example 127.1), which ParseIP does not catch.
	if strings.Trim(labels[len(labels)-1], "0123456789") == "" {
		return "", fmt.Errorf("sandbox egress host %q is an IP literal", raw)
	}
	return host, nil
}

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
	// State is the registration state the index resolved: "active" or
	// "archived" (WS04-S7). Archived rooms stay listed and readable.
	State string `json:"state"`
	// AgentScope is Control's derived agent scope for the kind ("personal",
	// "room" or "none"; WS12-S1).
	AgentScope string `json:"agent_scope"`
	// Participants is the fixed roster of a DM kind, omitted for every other
	// kind. Ids and display names only, never an email.
	Participants []DMParticipant `json:"participants,omitempty"`
}

// DMParticipant is one person of a direct message as shown to another
// participant. Same sanitization as RosterMember: no email.
type DMParticipant struct {
	SubjectID   string `json:"subject_id"`
	DisplayName string `json:"display_name"`
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
	// RosterMode is optional. Only the organization-room sync sends
	// "org_derived"; Control sets it once on the Space row and never clears it
	// (WS04-S4). Absent leaves the Space's mode unchanged.
	RosterMode string `json:"roster_mode,omitempty"`
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
		return fmt.Errorf("space reference is required")
	}
	if len(m.Members) > maxSpaceMembers {
		return fmt.Errorf("space membership exceeds %d subjects", maxSpaceMembers)
	}
	if mode := strings.TrimSpace(m.RosterMode); mode != "" && mode != RosterModeOrgDerived {
		return fmt.Errorf("unknown Space roster mode %q", m.RosterMode)
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
			return fmt.Errorf("space member subject id is required")
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

// CurrentMembership is a current Control-owned membership fact. It is useful
// to a resolver only as one component of effective access: callers must still
// intersect recipient, policy, and owner-resource authorization before an
// effect. The database result is deliberately not a signed access decision.
type CurrentMembership struct {
	SpaceRef  string            `json:"space_ref"`
	OrgID     string            `json:"org_id"`
	SubjectID string            `json:"subject_id"`
	Kind      Kind              `json:"kind"`
	Role      string            `json:"role"`
	Revisions AuthorityRevision `json:"revisions"`
	// RecipientReady is true when the subject is in the Space's registered
	// recipient audience at the current audience revision (always true for a
	// personal Space). Membership without it cannot yet read or post through a
	// shared decision, so a client shows "access is being prepared" (WS04-S4).
	RecipientReady bool `json:"recipient_ready"`
	// RosterMode is "managed" (Control writers) or "org_derived" (the
	// organization room, whose roster follows the organization).
	RosterMode string `json:"roster_mode,omitempty"`
	// State is "active" or, for ResolveReadableUserMembership only, "archived"
	// (WS04-S7). ArchivedAt is set when State is "archived".
	State      string     `json:"state,omitempty"`
	ArchivedAt *time.Time `json:"archived_at,omitempty"`
	// Visibility is "private" or "org_public" and JoinRole the role a self-join
	// receives (WS04-S8). Both are additive facts for the settings UI; neither
	// grants access by itself.
	Visibility string `json:"visibility,omitempty"`
	JoinRole   string `json:"join_role,omitempty"`
	// HistoryMode is "full" or "since_join" (WS04-S11): a stored room fact for
	// the settings UI. Whether a floor is actually signed also depends on the
	// SPACE_HISTORY_WINDOW_CLAIM rollout switch.
	HistoryMode string `json:"history_mode,omitempty"`
}

func (m CurrentMembership) Validate() error {
	if strings.TrimSpace(m.SpaceRef) == "" || strings.TrimSpace(m.OrgID) == "" || strings.TrimSpace(m.SubjectID) == "" {
		return fmt.Errorf("current Space membership identity is required")
	}
	switch m.Kind {
	case KindPersonal, KindRoom, KindProject, KindCase, KindDM, KindGroupDM:
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
		return fmt.Errorf("space authority revisions must be positive")
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
