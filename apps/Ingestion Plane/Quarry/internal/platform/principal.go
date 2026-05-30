package platform

import "strings"

// Principal represents the resolved identity plus org policy context of a caller.
// Policy fields are enriched after auth via billing-core and org-core lookups.
type Principal struct {
	UserID         string `json:"userId"`
	OrganizationID string `json:"organizationId"`
	WorkspaceID    string `json:"workspaceId"`
	Role           string `json:"role"`
	Email          string `json:"email"`
	Tier           string `json:"tier"`
	Plan           string `json:"plan,omitempty"`
	Credits        int64  `json:"credits,omitempty"`
	ZDRMode        bool   `json:"zdrMode,omitempty"`
	PolicyResolved bool   `json:"policyResolved,omitempty"`

	Entitlements map[string]bool        `json:"entitlements,omitempty"`
	FeatureFlags map[string]bool        `json:"featureFlags,omitempty"`
	QuotaLimits  map[string]float64     `json:"quotaLimits,omitempty"`
	Metadata     map[string]interface{} `json:"metadata,omitempty"`
}

func (p *Principal) Clone() *Principal {
	if p == nil {
		return nil
	}
	cloned := *p
	cloned.Entitlements = cloneBoolMap(p.Entitlements)
	cloned.FeatureFlags = cloneBoolMap(p.FeatureFlags)
	cloned.QuotaLimits = cloneFloatMap(p.QuotaLimits)
	cloned.Metadata = cloneAnyMap(p.Metadata)
	return &cloned
}

func (p *Principal) HasEntitlement(key string) bool {
	_, enabled := p.EntitlementDecision(key)
	return enabled
}

func (p *Principal) EntitlementDecision(key string) (bool, bool) {
	if p == nil {
		return false, false
	}
	key = strings.TrimSpace(key)
	if key == "" {
		return false, false
	}
	if enabled, ok := p.Entitlements[key]; ok {
		return true, enabled
	}
	if enabled, ok := p.FeatureFlags[key]; ok {
		return true, enabled
	}
	return false, false
}

func (p *Principal) MetadataInt(keys ...string) int {
	if p == nil {
		return 0
	}
	for _, key := range keys {
		if value := principalIntFromAny(p.Metadata[strings.TrimSpace(key)]); value > 0 {
			return value
		}
	}
	return 0
}

func (p *Principal) QuotaInt(keys ...string) int {
	if p == nil {
		return 0
	}
	for _, key := range keys {
		if value := int(p.QuotaLimits[strings.TrimSpace(key)]); value > 0 {
			return value
		}
	}
	return 0
}

func inferZDRMode(principal *Principal) bool {
	if principal == nil {
		return false
	}
	if principal.HasEntitlement("feature.zdr") || principal.HasEntitlement("feature.zero_data_retention") {
		return true
	}
	if truthy(principal.Metadata["zdr"]) || truthy(principal.Metadata["zdr_mode"]) || truthy(principal.Metadata["zero_data_retention"]) {
		return true
	}
	return false
}

func principalIntFromAny(value interface{}) int {
	switch typed := value.(type) {
	case int:
		return typed
	case int8:
		return int(typed)
	case int16:
		return int(typed)
	case int32:
		return int(typed)
	case int64:
		return int(typed)
	case uint:
		return int(typed)
	case uint8:
		return int(typed)
	case uint16:
		return int(typed)
	case uint32:
		return int(typed)
	case uint64:
		return int(typed)
	case float32:
		return int(typed)
	case float64:
		return int(typed)
	case string:
		if strings.TrimSpace(typed) == "" {
			return 0
		}
		sign := 1
		raw := strings.TrimSpace(typed)
		if strings.HasPrefix(raw, "-") {
			sign = -1
			raw = strings.TrimPrefix(raw, "-")
		}
		value := 0
		for _, ch := range raw {
			if ch < '0' || ch > '9' {
				return 0
			}
			value = value*10 + int(ch-'0')
		}
		return sign * value
	default:
		return 0
	}
}

func truthy(value interface{}) bool {
	switch typed := value.(type) {
	case bool:
		return typed
	case string:
		switch strings.ToLower(strings.TrimSpace(typed)) {
		case "1", "true", "yes", "enabled", "on":
			return true
		default:
			return false
		}
	case int:
		return typed != 0
	case int64:
		return typed != 0
	case float64:
		return typed != 0
	default:
		return false
	}
}
