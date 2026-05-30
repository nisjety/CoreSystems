package billing

import "strings"

var defaultEntitlementsByPlan = map[string]map[string]bool{
	"free": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
	"trial": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
	"hobby": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
	"standard": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
	"pro": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
	"enterprise": {
		"feature.chat":       true,
		"feature.api_keys":   true,
		"feature.audit_logs": true,
		"feature.sso":        false,
	},
}

var defaultQuotaLimitsByPlan = map[string]map[string]float64{
	"free": {
		"api_calls":  1000,
		"users":      5,
		"storage_mb": 1000,
	},
	"trial": {
		"api_calls":  1000,
		"users":      5,
		"storage_mb": 1000,
	},
	"hobby": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"standard": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"pro": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"enterprise": {
		"api_calls":  100000,
		"users":      -1,
		"storage_mb": -1,
	},
}

func normalizePlan(plan string) string {
	normalized := strings.ToLower(strings.TrimSpace(plan))
	switch normalized {
	case "free", "trial", "hobby", "standard", "pro", "enterprise":
		return normalized
	case "essential":
		return "hobby"
	case "advanced":
		return "standard"
	case "expert":
		return "pro"
	case "custom":
		return "enterprise"
	default:
		return "free"
	}
}

func billablePlan(plan string) string {
	normalized := normalizePlan(plan)
	switch normalized {
	case "hobby", "standard", "pro", "enterprise":
		return normalized
	default:
		return "free"
	}
}

func copyBoolMap(input map[string]bool) map[string]bool {
	out := make(map[string]bool, len(input))
	for key, value := range input {
		out[key] = value
	}
	return out
}

func copyFloatMap(input map[string]float64) map[string]float64 {
	out := make(map[string]float64, len(input))
	for key, value := range input {
		out[key] = value
	}
	return out
}

func defaultEntitlementsForPlan(plan string) map[string]bool {
	template, ok := defaultEntitlementsByPlan[normalizePlan(plan)]
	if !ok {
		template = defaultEntitlementsByPlan["free"]
	}
	return copyBoolMap(template)
}

func defaultQuotaLimitsForPlan(plan string) map[string]float64 {
	template, ok := defaultQuotaLimitsByPlan[normalizePlan(plan)]
	if !ok {
		template = defaultQuotaLimitsByPlan["free"]
	}
	return copyFloatMap(template)
}

func stripePlanAmountNOK(plan string) int64 {
	switch billablePlan(plan) {
	case "hobby":
		return 29900
	case "standard":
		return 99900
	case "pro":
		return 149900
	case "enterprise":
		return 249900
	default:
		return 0
	}
}

func stripePlanDisplayName(plan string) string {
	switch billablePlan(plan) {
	case "hobby":
		return "Velion Essential"
	case "standard":
		return "Velion Advanced"
	case "pro":
		return "Velion Expert"
	case "enterprise":
		return "Velion Custom"
	default:
		return "Velion Free"
	}
}
