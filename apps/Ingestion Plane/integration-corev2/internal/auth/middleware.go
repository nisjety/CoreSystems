package auth

import (
	"context"
	"crypto/subtle"
	"errors"
	"fmt"
	"strings"

	"github.com/gofiber/fiber/v2"
)

const (
	OrganizationIDHeader = "X-Org-ID"
	UserIDHeader         = "X-User-ID"
)

const (
	principalLocal  = "verevon.principal"
	internalLocal   = "verevon.internal_call"
	planLocal       = "verevon.org_plan"
	defaultRole     = "member"
	internalUserID  = "internal-service"
	internalRole    = "service"
	internalAPIName = "x-internal-api-key"
)

type Config struct {
	APIKey               string
	APIKeyHeader         string
	TokenVerifier        TokenVerifier
	AllowLegacyTenantKey bool
}

type Principal struct {
	UserID         string   `json:"userId"`
	OrganizationID string   `json:"organizationId"`
	WorkspaceID    string   `json:"workspaceId"`
	Role           string   `json:"role"`
	Email          string   `json:"email"`
	PrincipalType  string   `json:"principalType"`
	Scopes         []string `json:"scopes"`
}

func (p Principal) HasScope(scope string) bool {
	for _, granted := range p.Scopes {
		if granted == scope {
			return true
		}
	}
	return false
}

type TokenVerifier interface {
	VerifyToken(ctx context.Context, token string) (Principal, error)
}

type OrgPlanClient interface {
	GetOrgPlan(ctx context.Context, orgID, userID string) (OrgPlan, error)
}

type OrgPlan struct {
	Plan         string           `json:"plan"`
	Quotas       map[string]Quota `json:"quotas"`
	Entitlements map[string]bool  `json:"entitlements"`
	Raw          map[string]any   `json:"raw,omitempty"`
}

type Quota struct {
	Key         string `json:"key"`
	Value       int64  `json:"value"`
	Limit       int64  `json:"limit"`
	ResetPeriod string `json:"resetPeriod"`
}

type Error struct {
	Status  int
	Code    string
	Message string
}

func (e Error) Error() string {
	return e.Message
}

func NewError(status int, code, message string) Error {
	return Error{Status: status, Code: code, Message: message}
}

func InternalOnly(cfg Config) fiber.Handler {
	header := strings.TrimSpace(cfg.APIKeyHeader)
	if header == "" {
		header = "X-Internal-API-Key"
	}
	return func(c *fiber.Ctx) error {
		provided := strings.TrimSpace(c.Get(header))
		if provided == "" {
			provided = strings.TrimSpace(c.Get("x-internal-api-key"))
		}
		if provided == "" || cfg.APIKey == "" {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "authentication required")
		}
		if subtle.ConstantTimeCompare([]byte(provided), []byte(cfg.APIKey)) != 1 {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "invalid API key")
		}
		c.Locals(internalLocal, true)
		c.Locals(principalLocal, syntheticInternalPrincipal(c))
		return c.Next()
	}
}

func InternalOrBearer(cfg Config) fiber.Handler {
	return func(c *fiber.Ctx) error {
		authHeader := strings.TrimSpace(c.Get("Authorization"))
		if authHeader != "" {
			if !strings.HasPrefix(authHeader, "Bearer ") {
				return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Authorization header must use Bearer scheme")
			}
			token := strings.TrimSpace(strings.TrimPrefix(authHeader, "Bearer "))
			if token == "" {
				return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Bearer token is empty")
			}
			if cfg.TokenVerifier == nil {
				return writeError(c, fiber.StatusServiceUnavailable, "auth_core_unconfigured", "auth-core verifier is not configured")
			}
			principal, err := cfg.TokenVerifier.VerifyToken(c.UserContext(), token)
			if err != nil {
				var authErr Error
				if errors.As(err, &authErr) {
					return writeError(c, authErr.Status, authErr.Code, authErr.Message)
				}
				return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Token verification failed")
			}
			if strings.TrimSpace(principal.UserID) == "" || strings.TrimSpace(principal.OrganizationID) == "" {
				return writeError(c, fiber.StatusServiceUnavailable, "auth_core_incomplete_response", "auth-core response is missing required principal fields")
			}
			if strings.TrimSpace(principal.Role) == "" {
				principal.Role = defaultRole
			}
			if strings.TrimSpace(principal.PrincipalType) == "" {
				principal.PrincipalType = "user"
			}
			c.Locals(internalLocal, false)
			c.Locals(principalLocal, principal)
			return c.Next()
		}

		provided := providedInternalKey(c, cfg)
		if provided == "" {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Authentication required. Provide an Authorization header or x-internal-api-key header.")
		}
		if cfg.APIKey == "" || subtle.ConstantTimeCompare([]byte(provided), []byte(cfg.APIKey)) != 1 {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Invalid internal API key")
		}
		if !cfg.AllowLegacyTenantKey {
			return writeError(c, fiber.StatusForbidden, "scoped_service_token_required", "Tenant APIs require a verified user or audience-scoped service token")
		}
		c.Locals(internalLocal, true)
		c.Locals(principalLocal, syntheticInternalPrincipal(c))
		return c.Next()
	}
}

func RequirePlan(orgClient OrgPlanClient, minimumPlan string) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if IsInternalCall(c) {
			return c.Next()
		}
		if orgClient == nil {
			return writeError(c, fiber.StatusServiceUnavailable, "org_core_unconfigured", "org-core client is not configured")
		}
		principal, ok := PrincipalFromContext(c)
		if !ok {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Authentication required")
		}
		if principal.OrganizationID == "" {
			return writeError(c, fiber.StatusForbidden, "PLAN_REQUIRED", "Organization context is missing from principal")
		}
		plan, err := orgClient.GetOrgPlan(c.UserContext(), principal.OrganizationID, principal.UserID)
		if err != nil {
			var authErr Error
			if errors.As(err, &authErr) {
				return writeError(c, authErr.Status, authErr.Code, authErr.Message)
			}
			return writeError(c, fiber.StatusBadGateway, "org_core_error", "Unable to fetch organization plan")
		}
		c.Locals(planLocal, plan)
		if planRank(plan.Plan) < planRank(minimumPlan) {
			message := fmt.Sprintf("This feature requires a %s plan or higher", minimumPlan)
			return c.Status(fiber.StatusForbidden).JSON(fiber.Map{
				"success": false,
				"error": fiber.Map{
					"code":         "PLAN_REQUIRED",
					"message":      message,
					"requiredPlan": minimumPlan,
					"currentPlan":  plan.Plan,
				},
			})
		}
		return c.Next()
	}
}

func AssertOrgAccess(c *fiber.Ctx, targetOrgID string) error {
	if IsInternalCall(c) {
		return nil
	}
	principal, ok := PrincipalFromContext(c)
	if !ok {
		return NewError(fiber.StatusUnauthorized, "unauthorized", "Request is not authenticated")
	}
	if strings.TrimSpace(targetOrgID) == "" {
		return NewError(fiber.StatusForbidden, "forbidden", "Organization context is required")
	}
	if principal.OrganizationID != targetOrgID {
		return NewError(fiber.StatusForbidden, "forbidden", "Access to this organization is not allowed")
	}
	return nil
}

func PrincipalFromContext(c *fiber.Ctx) (Principal, bool) {
	principal, ok := c.Locals(principalLocal).(Principal)
	return principal, ok
}

func IsInternalCall(c *fiber.Ctx) bool {
	internal, _ := c.Locals(internalLocal).(bool)
	return internal
}

func providedInternalKey(c *fiber.Ctx, cfg Config) string {
	header := strings.TrimSpace(cfg.APIKeyHeader)
	if header == "" {
		header = "X-Internal-API-Key"
	}
	provided := strings.TrimSpace(c.Get(header))
	if provided == "" {
		provided = strings.TrimSpace(c.Get(internalAPIName))
	}
	return provided
}

func syntheticInternalPrincipal(c *fiber.Ctx) Principal {
	return Principal{
		UserID:         firstNonEmpty(requestValue(c, "userId"), internalUserID),
		OrganizationID: requestValue(c, "organizationId"),
		WorkspaceID:    requestValue(c, "workspaceId"),
		Role:           internalRole,
		Email:          requestValue(c, "userEmail"),
		PrincipalType:  "service",
		Scopes:         []string{"integration:read", "integration:write"},
	}
}

func requestValue(c *fiber.Ctx, key string) string {
	var body map[string]any
	if len(c.Body()) > 0 {
		_ = c.BodyParser(&body)
	}
	if value, ok := body[key].(string); ok && strings.TrimSpace(value) != "" {
		return strings.TrimSpace(value)
	}
	if value := strings.TrimSpace(c.Query(key)); value != "" {
		return value
	}
	headerName := "X-" + strings.TrimSuffix(strings.TrimSuffix(key, "Id"), "ID") + "-ID"
	if strings.EqualFold(key, "organizationId") {
		headerName = OrganizationIDHeader
	}
	if strings.EqualFold(key, "userId") {
		headerName = UserIDHeader
	}
	return strings.TrimSpace(c.Get(headerName))
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func planRank(plan string) int {
	switch strings.ToLower(strings.TrimSpace(plan)) {
	case "starter", "essential":
		return 1
	case "pro", "advanced":
		return 2
	case "enterprise", "expert", "custom":
		return 3
	default:
		return 0
	}
}

func writeError(c *fiber.Ctx, status int, code, message string) error {
	if status <= 0 {
		status = fiber.StatusInternalServerError
	}
	return c.Status(status).JSON(fiber.Map{
		"success": false,
		"error": fiber.Map{
			"code":    code,
			"message": message,
		},
	})
}
