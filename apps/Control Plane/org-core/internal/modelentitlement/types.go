// Package modelentitlement is org-core's chat model policy (ADR-0005): which
// models an organisation, and each role inside it, may use.
//
// org-core stores, validates, resolves and explains the policy. It never
// enforces it: inference-core pulls the resolved answer and is the only
// enforcement point. The policy holds model ids and actor ids and nothing else,
// so it carries no content and needs no retention class (ZDR clause of WS10).
package modelentitlement

import "time"

const (
	// ScopeOrg and ScopeRole are the two policy layers. A plan or group layer
	// is deferred (WS10 D7); the storage key already accepts only these two.
	ScopeOrg  = "org"
	ScopeRole = "role"

	AllowModeAll    = "all"
	AllowModeListed = "listed"

	// The states a resolution reports. StateNone is explicit and means
	// unrestricted; StateMemberNotFound means a named user is not an active
	// member of an organisation that has a policy, and gets nothing.
	StateNone           = "none"
	StateOrg            = "org"
	StateRole           = "role"
	StateMemberNotFound = "member_not_found"

	// LayerOrgDefault and LayerRoleOverride name the one layer a resolution
	// came from (WS10 D9: a role override replaces the default, never merges).
	LayerOrgDefault   = "org_default"
	LayerRoleOverride = "role_override"

	// ReasonNotInAllowList and ReasonExplicitlyDenied are the refusal reasons
	// the Model Plane vocabulary (mp-contracts model_entitlement::Reason) uses
	// for a model a policy does not permit.
	ReasonNotInAllowList   = "not_in_allow_list"
	ReasonExplicitlyDenied = "explicitly_denied"

	// ProviderPrefix marks an entry that selects every model of one provider.
	ProviderPrefix = "provider:"
)

// SurfaceDefault is the model and effort a surface (for example "chat") starts
// with when the caller names none.
type SurfaceDefault struct {
	Model  string `json:"model,omitempty"`
	Effort string `json:"effort,omitempty"`
}

// Policy is one complete policy for one scope. Allow and Deny hold exact model
// ids or "provider:<id>" selectors, lower-cased; there are no patterns.
type Policy struct {
	AllowMode      string                    `json:"allow_mode"`
	Allow          []string                  `json:"allow"`
	Deny           []string                  `json:"deny"`
	MaxEffort      string                    `json:"max_effort,omitempty"`
	MinPrivacyTier string                    `json:"min_privacy_tier,omitempty"`
	Defaults       map[string]SurfaceDefault `json:"defaults,omitempty"`
}

// PolicySet is everything stored for one organisation. A zero Revision means
// nothing was ever written.
type PolicySet struct {
	OrganizationID string            `json:"organization_id"`
	Revision       int64             `json:"revision"`
	OrgDefault     *Policy           `json:"org_default"`
	Roles          map[string]Policy `json:"roles"`
	UpdatedAt      *time.Time        `json:"updated_at"`
	UpdatedBy      string            `json:"updated_by"`
}

// PutRequest replaces the whole policy set. ExpectedRevision is a pointer so a
// body that omits it is rejected instead of reading as "first write".
type PutRequest struct {
	ExpectedRevision *int64            `json:"expected_revision"`
	OrgDefault       *Policy           `json:"org_default"`
	Roles            map[string]Policy `json:"roles"`
}

// Source names the layer a resolution came from.
type Source struct {
	Layer string `json:"layer"`
	Role  string `json:"role,omitempty"`
}

// Resolution is the answer for one caller: what applies, from which layer, at
// which revision. It is the body of the machine route and of explain.
type Resolution struct {
	State     string   `json:"state"`
	Revision  int64    `json:"revision"`
	Role      string   `json:"role"`
	CanManage bool     `json:"can_manage"`
	Policy    *Policy  `json:"policy"`
	Sources   []Source `json:"sources"`
}

// Caller identifies who a resolution is for. UserID empty is a service call and
// gets the organisation default only. Role is the user's active membership role
// and is empty when UserID names someone who is not an active member.
type Caller struct {
	UserID string
	Role   string
}
