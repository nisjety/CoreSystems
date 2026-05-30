package platform

import (
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

type ControlPlanePolicyConfig struct {
	Service *ControlPlaneService
}

// ControlPlanePolicyMiddleware enriches the authenticated principal with cached
// billing/org policy data. Failures are logged and the request continues with
// the bare auth principal.
func ControlPlanePolicyMiddleware(cfg ControlPlanePolicyConfig) fiber.Handler {
	return func(c *fiber.Ctx) error {
		principal := GetPrincipal(c)
		if principal == nil || principal.OrganizationID == "" || principal.Tier == "internal" || cfg.Service == nil {
			return c.Next()
		}

		enriched := principal.Clone()
		policyResolved := false

		account, accountErr := cfg.Service.GetAccount(c.UserContext(), principal.OrganizationID)
		if accountErr != nil {
			log.Warn().Err(accountErr).
				Str("org_id", principal.OrganizationID).
				Str("path", c.Path()).
				Msg("control-plane billing account enrichment failed")
		} else if account != nil {
			policyResolved = true
			if strings.TrimSpace(account.Plan) != "" {
				enriched.Plan = account.Plan
				enriched.Tier = account.Plan
			}
			enriched.Credits = account.Credits
			enriched.FeatureFlags = cloneBoolMap(account.FeatureFlags)
			enriched.QuotaLimits = cloneFloatMap(account.QuotaLimits)
			enriched.Metadata = cloneAnyMap(account.Metadata)
			enriched.Entitlements = cloneBoolMap(account.Entitlements)
		}

		entitlements, entErr := cfg.Service.GetEntitlements(c.UserContext(), principal.OrganizationID)
		if entErr != nil {
			log.Warn().Err(entErr).
				Str("org_id", principal.OrganizationID).
				Str("path", c.Path()).
				Msg("control-plane entitlement enrichment failed")
		} else if entitlements != nil {
			policyResolved = true
			if enriched.Entitlements == nil {
				enriched.Entitlements = make(map[string]bool, len(entitlements.Entitlements))
			}
			for _, entitlement := range entitlements.Entitlements {
				enriched.Entitlements[entitlement.Key] = entitlement.Enabled
			}
		}

		enriched.PolicyResolved = policyResolved
		enriched.ZDRMode = inferZDRMode(enriched)
		c.Locals(PrincipalContextKey, enriched)
		if strings.TrimSpace(enriched.Plan) != "" {
			c.Set("X-Org-Plan", enriched.Plan)
		}
		if enriched.ZDRMode {
			c.Set("X-ZDR-Mode", "true")
		}
		return c.Next()
	}
}

func RequireEntitlement(feature string, principal *Principal, controlPlaneConfigured bool) error {
	if !controlPlaneConfigured || principal == nil || principal.Tier == "internal" || !principal.PolicyResolved {
		return nil
	}
	present, enabled := principal.EntitlementDecision(feature)
	if !present || enabled {
		return nil
	}
	return fiber.NewError(fiber.StatusForbidden, "feature is not enabled for this organization")
}
