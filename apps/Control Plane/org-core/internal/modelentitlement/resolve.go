package modelentitlement

import "strings"

// Resolve answers what applies to one caller. It is pure: no clock, no store,
// and no ZDR input, so a policy reads the same whatever the organisation's
// retention posture is (golden row z_policy_survives_zdr_toggle).
//
//   - A caller with no user id (a service call) gets the organisation default
//     only; role overrides are about people.
//   - A named user who is not an active member gets nothing, but only when the
//     organisation has a policy at all. An organisation that never wrote one
//     stays unrestricted, so a membership projection that lags auth-core cannot
//     lock a member out of an organisation nobody restricted.
//   - A role override replaces the organisation default completely.
func Resolve(set *PolicySet, caller Caller) Resolution {
	resolution := Resolution{State: StateNone, Sources: []Source{}}
	if set != nil {
		resolution.Revision = set.Revision
	}
	if caller.UserID == "" {
		if set != nil && set.OrgDefault != nil {
			resolution.State = StateOrg
			resolution.Policy = set.OrgDefault
			resolution.Sources = []Source{{Layer: LayerOrgDefault}}
		}
		return resolution
	}

	role := NormalizeRole(caller.Role)
	if role == "" {
		if hasPolicy(set) {
			resolution.State = StateMemberNotFound
			resolution.Policy = &Policy{AllowMode: AllowModeListed, Allow: []string{}, Deny: []string{}}
		}
		return resolution
	}
	resolution.Role = role
	resolution.CanManage = role == "owner" || role == "admin"

	if set == nil {
		return resolution
	}
	if override, ok := set.Roles[role]; ok {
		resolution.State = StateRole
		resolution.Policy = &override
		resolution.Sources = []Source{{Layer: LayerRoleOverride, Role: role}}
		return resolution
	}
	if set.OrgDefault != nil {
		resolution.State = StateOrg
		resolution.Policy = set.OrgDefault
		resolution.Sources = []Source{{Layer: LayerOrgDefault}}
	}
	return resolution
}

func hasPolicy(set *PolicySet) bool {
	return set != nil && (set.OrgDefault != nil || len(set.Roles) > 0)
}

// Permits reports whether the policy lets a model be used and, when it does
// not, why. provider is the provider that serves the model and may be empty
// when the caller does not know it; a provider selector then matches nothing.
//
// Deny wins over allow, matching is exact on the lower-cased id, and
// "provider:<id>" selects every model of one provider. In "all" mode everything
// the deny list does not name is permitted.
func (p Policy) Permits(model, provider string) (bool, string) {
	model = strings.ToLower(strings.TrimSpace(model))
	selector := ""
	if provider = strings.ToLower(strings.TrimSpace(provider)); provider != "" {
		selector = ProviderPrefix + provider
	}
	if containsExact(p.Deny, model) || (selector != "" && containsExact(p.Deny, selector)) {
		return false, ReasonExplicitlyDenied
	}
	// Anything but an explicit "all" is treated as listed, so a row with an
	// unknown mode narrows access instead of opening it.
	if p.AllowMode == AllowModeAll {
		return true, ""
	}
	if containsExact(p.Allow, model) || (selector != "" && containsExact(p.Allow, selector)) {
		return true, ""
	}
	return false, ReasonNotInAllowList
}
