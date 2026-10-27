package spaces

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"

	"github.com/jackc/pgx/v5"
)

// DenialCode is the closed, public vocabulary Control uses to say WHY a Space
// decision was refused. It is carried in-band on the refused request (never on
// a push frame) next to the unchanged `error` string, and is pinned by the
// golden testdata/space-denial-codes.golden.json.
type DenialCode string

const (
	// DenialMembershipRequired covers a non-member, an unknown Space and a
	// non-active org member. They are indistinguishable by construction, so a
	// non-member never learns whether the Space or the policy exists.
	DenialMembershipRequired DenialCode = "space_membership_required"
	// DenialNotActive: the caller is a member but the Space registration is not active.
	DenialNotActive DenialCode = "space_not_active"
	// DenialKindUnsupported: the decision does not serve this Space kind.
	DenialKindUnsupported DenialCode = "space_kind_unsupported"
	// DenialPolicyNotProvisioned: the org has no effect-policy row (deny when absent).
	DenialPolicyNotProvisioned DenialCode = "space_policy_not_provisioned"
	// DenialEntitlementOff: the org's policy has the required entitlement off.
	DenialEntitlementOff DenialCode = "space_entitlement_off"
	// DenialRoleBelowFloor: the caller's Space role is below the effect's floor.
	DenialRoleBelowFloor DenialCode = "space_role_below_floor"
	// DenialZDRForbidsEffect: the Space's Zero Data Retention posture forbids the effect.
	DenialZDRForbidsEffect DenialCode = "space_zdr_forbids_effect"
	// DenialAudienceNotCurrent: the caller is not in the Space's current recipient audience.
	DenialAudienceNotCurrent DenialCode = "space_audience_not_current"
	// DenialAuthorityIncomplete: authority evidence is incomplete for a reason
	// no other code names; an operator has to look.
	DenialAuthorityIncomplete DenialCode = "space_authority_incomplete"
	// DenialAuthorityUnavailable: Control could not resolve authority (fails closed).
	DenialAuthorityUnavailable DenialCode = "space_authority_unavailable"
)

// Who can resolve a denial (`resolvable_by`).
const (
	ResolvableByNone         = "none"
	ResolvableBySpaceManager = "space_manager"
	ResolvableByOrgAdmin     = "org_admin"
	ResolvableByRetry        = "retry"
	ResolvableByOperator     = "operator"
)

// DenialSpec is one row of the denial vocabulary: the HTTP status, a coarse
// class, who can resolve it, and the ONLY detail keys a body of this code may
// carry. Details never hold attestation values, resource ids or tokens.
type DenialSpec struct {
	Code         DenialCode
	Status       int
	Class        string
	ResolvableBy string
	DetailKeys   []string
	Retryable    bool
}

// denialCatalogue lists every code in fixed order. Order and content are part
// of the contract and of the golden.
var denialCatalogue = []DenialSpec{
	{DenialMembershipRequired, http.StatusForbidden, "membership", ResolvableByNone, nil, false},
	{DenialNotActive, http.StatusForbidden, "lifecycle", ResolvableBySpaceManager, []string{"state"}, false},
	{DenialKindUnsupported, http.StatusForbidden, "kind", ResolvableByNone, []string{"kind"}, false},
	{DenialPolicyNotProvisioned, http.StatusForbidden, "policy", ResolvableByOrgAdmin, nil, false},
	{DenialEntitlementOff, http.StatusForbidden, "entitlement", ResolvableByOrgAdmin, []string{"entitlement"}, false},
	{DenialRoleBelowFloor, http.StatusForbidden, "role", ResolvableBySpaceManager, []string{"required_roles"}, false},
	{DenialZDRForbidsEffect, http.StatusForbidden, "zdr", ResolvableByNone, []string{"effect"}, false},
	{DenialAudienceNotCurrent, http.StatusForbidden, "audience", ResolvableByRetry, nil, true},
	{DenialAuthorityIncomplete, http.StatusForbidden, "authority", ResolvableByOperator, nil, false},
	{DenialAuthorityUnavailable, http.StatusServiceUnavailable, "availability", ResolvableByRetry, nil, true},
}

// DenialCatalogue returns a copy of the vocabulary in fixed order.
func DenialCatalogue() []DenialSpec {
	out := make([]DenialSpec, len(denialCatalogue))
	copy(out, denialCatalogue)
	return out
}

func denialSpecFor(code DenialCode) (DenialSpec, bool) {
	for _, spec := range denialCatalogue {
		if spec.Code == code {
			return spec, true
		}
	}
	return DenialSpec{}, false
}

// Denial is a typed authority refusal. It implements error with the message the
// refusal always had, so wrapping it with %w keeps every existing err.Error()
// assertion true while errors.As reaches the code.
type Denial struct {
	Code DenialCode
	// Entitlement is the closed entitlement catalogue key (space_entitlement_off).
	Entitlement string
	// RequiredRoles are the Space roles that would pass the floor (space_role_below_floor).
	RequiredRoles []string
	// State is the registration state (space_not_active).
	State string
	// Kind is the Space kind (space_kind_unsupported). The plan's struct sketch
	// omits it while its detail table names it, so it is carried here.
	Kind string
	// Effect names the effect a ZDR posture forbids (space_zdr_forbids_effect).
	Effect string

	message string
}

// Error returns the original, unchanged refusal text.
func (d *Denial) Error() string {
	if d == nil {
		return ""
	}
	return d.message
}

// Status is the HTTP status the vocabulary assigns the code.
func (d *Denial) Status() int {
	if spec, ok := denialSpecFor(d.Code); ok {
		return spec.Status
	}
	return http.StatusForbidden
}

// ResolvableBy says who can resolve the denial.
func (d *Denial) ResolvableBy() string {
	if spec, ok := denialSpecFor(d.Code); ok {
		return spec.ResolvableBy
	}
	return ResolvableByOperator
}

// Details returns only the allowlisted, non-empty detail keys of the code.
func (d *Denial) Details() map[string]any {
	out := map[string]any{}
	spec, ok := denialSpecFor(d.Code)
	if !ok {
		return out
	}
	for _, key := range spec.DetailKeys {
		switch key {
		case "entitlement":
			if d.Entitlement != "" {
				out[key] = d.Entitlement
			}
		case "required_roles":
			if len(d.RequiredRoles) > 0 {
				out[key] = append([]string(nil), d.RequiredRoles...)
			}
		case "state":
			if d.State != "" {
				out[key] = d.State
			}
		case "kind":
			if d.Kind != "" {
				out[key] = d.Kind
			}
		case "effect":
			if d.Effect != "" {
				out[key] = d.Effect
			}
		}
	}
	return out
}

// AsDenial finds a typed refusal anywhere in err's chain.
func AsDenial(err error) (*Denial, bool) {
	var denial *Denial
	if errors.As(err, &denial) && denial != nil {
		return denial, true
	}
	return nil, false
}

func newDenial(code DenialCode, message string) *Denial {
	return &Denial{Code: code, message: message}
}

func entitlementOffDenial(entitlementKey, message string) *Denial {
	d := newDenial(DenialEntitlementOff, message)
	d.Entitlement = entitlementKey
	return d
}

func roleBelowFloorDenial(message string, requiredRoles ...string) *Denial {
	d := newDenial(DenialRoleBelowFloor, message)
	d.RequiredRoles = append([]string(nil), requiredRoles...)
	return d
}

func zdrForbidsDenial(effect, message string) *Denial {
	d := newDenial(DenialZDRForbidsEffect, message)
	d.Effect = effect
	return d
}

func kindUnsupportedDenial(kind, message string) *Denial {
	d := newDenial(DenialKindUnsupported, message)
	d.Kind = kind
	return d
}

func authorityIncompleteDenial(message string) *Denial {
	return newDenial(DenialAuthorityIncomplete, message)
}

// AuthorityUnavailableDenial is the typed form of "Control could not resolve
// authority"; it is the only retryable 503 of the vocabulary.
func AuthorityUnavailableDenial(message string) *Denial {
	return newDenial(DenialAuthorityUnavailable, message)
}

// ExpectKind says which Space kind a decision serves, so a refusal on the other
// kind is named honestly (space_kind_unsupported) instead of collapsing.
type ExpectKind string

const (
	// ExpectAny serves both personal and shared Spaces.
	ExpectAny ExpectKind = "any"
	// ExpectPersonal serves personal Spaces only.
	ExpectPersonal ExpectKind = "personal"
	// ExpectShared serves shared (non-personal) Spaces only.
	ExpectShared ExpectKind = "shared"
	// ExpectMembershipOnly is for the membership resolver: no policy or
	// audience is required, so only the membership and lifecycle codes apply.
	ExpectMembershipOnly ExpectKind = "membership_only"
)

// denialFlags are the facts DiagnoseDenial reads in one query. They only label a
// refusal the authoritative query already issued; they can never grant.
type denialFlags struct {
	OrgMember         bool
	SpaceFound        bool
	SpaceMember       bool
	SpaceKind         string
	RegistrationState string
	HasPolicy         bool
	InAudience        bool
}

// classifyDenial is pure. Order matters and is the anti-enumeration rule: a
// non-member (not an org member, Space not found in this org, or not an active
// Space member) always gets space_membership_required, before anything about
// the Space or the policy can be told.
func classifyDenial(flags denialFlags, expect ExpectKind) *Denial {
	if !flags.OrgMember || !flags.SpaceFound || !flags.SpaceMember {
		return newDenial(DenialMembershipRequired, "current Space membership required")
	}
	if flags.RegistrationState != "active" {
		d := newDenial(DenialNotActive, "Space is not active")
		d.State = flags.RegistrationState
		return d
	}
	if expect == ExpectMembershipOnly {
		// The membership query already passed every check this mode makes; only
		// a race can land here, and it is a membership refusal.
		return newDenial(DenialMembershipRequired, "current Space membership required")
	}
	personal := flags.SpaceKind == string(KindPersonal)
	if (expect == ExpectPersonal && !personal) || (expect == ExpectShared && personal) {
		return kindUnsupportedDenial(flags.SpaceKind, "Space kind is not served by this decision")
	}
	if !flags.HasPolicy {
		return newDenial(DenialPolicyNotProvisioned, "organization Space effect policy is not provisioned")
	}
	if !personal && !flags.InAudience {
		return newDenial(DenialAudienceNotCurrent, "caller is not in the current Space recipient audience")
	}
	return authorityIncompleteDenial("Space authority is incomplete")
}

// DiagnoseDenial labels a refusal whose authoritative evidence query returned no
// row (ErrNoCurrentMembership). One read, scoped to the caller's org, then the
// pure classifier. It never returns nil without an error.
func (r *Repository) DiagnoseDenial(ctx context.Context, spaceRef, orgID, subjectID string, expect ExpectKind) (*Denial, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("space authority repository unavailable")
	}
	spaceRef, orgID, subjectID = strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID)
	var flags denialFlags
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_kind, s.registration_state,
		       EXISTS (SELECT 1 FROM user_org_memberships u
		               WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active'),
		       EXISTS (SELECT 1 FROM space_memberships m
		               WHERE m.space_ref=s.space_ref AND m.subject_type='user'
		                 AND m.subject_id=$3 AND m.active=TRUE),
		       EXISTS (SELECT 1 FROM space_effect_policies p WHERE p.org_id=s.org_id),
		       EXISTS (SELECT 1 FROM space_authority_revisions r
		               JOIN space_recipient_audiences a
		                 ON a.space_ref=r.space_ref AND a.revision=r.recipient_audience_revision
		               JOIN space_recipient_audience_members am
		                 ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=$3
		               WHERE r.space_ref=s.space_ref)
		FROM registered_spaces s
		WHERE s.space_ref=$1 AND s.org_id=$2`,
		spaceRef, orgID, subjectID,
	).Scan(&flags.SpaceKind, &flags.RegistrationState, &flags.OrgMember, &flags.SpaceMember, &flags.HasPolicy, &flags.InAudience)
	if errors.Is(err, pgx.ErrNoRows) {
		// Unknown Space in this org: indistinguishable from a non-member.
		return classifyDenial(denialFlags{}, expect), nil
	}
	if err != nil {
		return nil, fmt.Errorf("diagnose Space denial: %w", err)
	}
	flags.SpaceFound = true
	return classifyDenial(flags, expect), nil
}

// denialGolden is the on-disk shape of testdata/space-denial-codes.golden.json.
type denialGolden struct {
	Version int                `json:"version"`
	Codes   []denialGoldenCode `json:"codes"`
}

type denialGoldenCode struct {
	Code         string   `json:"code"`
	Status       int      `json:"status"`
	Class        string   `json:"class"`
	ResolvableBy string   `json:"resolvable_by"`
	DetailsKeys  []string `json:"details_keys"`
	Retryable    bool     `json:"retryable"`
}

// DenialCodesGolden renders the vocabulary as the canonical golden bytes (LF,
// two-space indent, trailing newline) and its sha256 pin. The golden test
// compares these bytes to the checked-in file and rewrites it under
// UPDATE_CONTRACT_GOLDEN=1.
func DenialCodesGolden() (golden []byte, sha256Hex string, err error) {
	doc := denialGolden{Version: 1}
	for _, spec := range denialCatalogue {
		keys := append([]string{}, spec.DetailKeys...)
		doc.Codes = append(doc.Codes, denialGoldenCode{
			Code: string(spec.Code), Status: spec.Status, Class: spec.Class,
			ResolvableBy: spec.ResolvableBy, DetailsKeys: keys, Retryable: spec.Retryable,
		})
	}
	var buf bytes.Buffer
	encoder := json.NewEncoder(&buf)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(doc); err != nil {
		return nil, "", err
	}
	sum := sha256.Sum256(buf.Bytes())
	return buf.Bytes(), hex.EncodeToString(sum[:]), nil
}
