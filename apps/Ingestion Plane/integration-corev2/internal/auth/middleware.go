package auth

import (
	"context"
	"crypto/subtle"
	"errors"
	"fmt"
	"strings"
	"time"

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

// The general integration scopes a service principal must hold to be admitted
// on the bearer path (authenticateBearer).
const (
	IntegrationReadScope  = "integration:read"
	IntegrationWriteScope = "integration:write"
	// IntegrationOTPReadScope is the narrow scope that admits an
	// executor-only caller to mail.otp.read (E3.1-g). It is honored on exactly
	// one route, OTPActionsRoute, and nowhere else.
	IntegrationOTPReadScope = "integration:otp:read"
	// OTPActionsRoute is the registered path of the only route a service
	// principal holding just IntegrationOTPReadScope is admitted on. The
	// service layer then limits it to the mail.otp.read operation.
	OTPActionsRoute = "/api/v1/connections/:id/actions"
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
	// DelegatedUserID is the end user a service principal names itself as
	// acting for, never trusted from the caller's own claim of identity.
	// It is populated only from a verified `acting_user_id` claim on an
	// auth-core-issued scoped service token (see
	// controlplane.AuthClient.VerifyToken) — a bare service credential with
	// no delegation leaves this empty. Never set for an interactive/session
	// principal (PrincipalType == "user"): that principal's own UserID is
	// already the verified end user. Read this only through
	// Principal.EffectiveUserID, never directly.
	DelegatedUserID string `json:"-"`
	// TokenID and TokenExpiresAt are the verified token's own `jti` and
	// `exp`. They are read only to make a delegated token single-use (see
	// single_use.go); nothing else may treat them as identity.
	TokenID        string    `json:"-"`
	TokenExpiresAt time.Time `json:"-"`
}

func (p Principal) HasScope(scope string) bool {
	for _, granted := range p.Scopes {
		if granted == scope {
			return true
		}
	}
	return false
}

// EffectiveUserID returns the human end-user identity this request is
// acting for, or "" when no end user is named. Used to enforce owner ruling
// (5): a personal Microsoft/Google connection (mail, OneDrive/Drive,
// calendar, contacts) is usable only by the user who connected it,
// including when a service acts "for" that user — so a bare service
// credential with no named end user must resolve to "", never to a
// wildcard or to the service's own identity.
//
// isInternalCall must be IsInternalCall(c) for the very same request: it is
// the one signal that tells apart the two paths that both produce
// PrincipalType == "service" but give UserID a different meaning on each.
//
//   - isInternalCall == true (legacy internal-API-key path,
//     syntheticInternalPrincipal): UserID itself IS the delegation — the
//     caller-supplied userId (from X-User-ID / body / query — see
//     requestValue) — unless it is the internal-service sentinel, in which
//     case no end user was named.
//   - isInternalCall == false (a verified bearer token — interactive
//     session or an auth-core-issued scoped service token):
//   - PrincipalType == "user": always its own UserID — a verified end
//     user by construction.
//   - PrincipalType == "service" (e.g. execution-core's scoped ingestion
//     token): DelegatedUserID only, set only from a verified
//     acting_user_id claim. UserID on this path holds the SERVICE's own
//     subject (service_id — see controlplane.AuthClient.VerifyToken),
//     never a human, so it is never used here.
func (p Principal) EffectiveUserID(isInternalCall bool) string {
	if isInternalCall {
		if userID := strings.TrimSpace(p.UserID); userID != "" && userID != internalUserID {
			return userID
		}
		return ""
	}
	if p.PrincipalType == "user" {
		return strings.TrimSpace(p.UserID)
	}
	return strings.TrimSpace(p.DelegatedUserID)
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
			return authenticateBearer(c, cfg, authHeader)
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

// BearerOnly admits a request only with a verified bearer token. It never reads the internal API
// key, so a caller that holds only that key cannot name the user a request acts for. Routes that
// act on one user's own data are guarded with this rather than InternalOrBearer.
func BearerOnly(cfg Config) fiber.Handler {
	return func(c *fiber.Ctx) error {
		authHeader := strings.TrimSpace(c.Get("Authorization"))
		if authHeader == "" {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "Bearer token required")
		}
		return authenticateBearer(c, cfg, authHeader)
	}
}

// authenticateBearer verifies the bearer token in authHeader. A verified token makes its own
// principal the request's principal; every refusal is answered here and returned.
func authenticateBearer(c *fiber.Ctx, cfg Config, authHeader string) error {
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
	// A service principal is admitted only with a general integration scope.
	// Read routes do not check a scope of their own (mutations add
	// integration:write on top), so without this a narrower grant on this
	// audience, such as quarry-edge's integration:otp:read (R-13 PR 1), would
	// read every org-wide route and connector for any organization it can mint
	// for. A route that serves a narrower scope must admit it explicitly when it
	// ships (E3.1-g). Checked before the single-use guard so a refused token is
	// not consumed.
	if principal.PrincipalType == "service" && !principal.HasScope(IntegrationReadScope) && !principal.HasScope(IntegrationWriteScope) &&
		!otpScopeAdmittedOnRoute(c, principal) {
		return writeError(c, fiber.StatusForbidden, "integration_scope_required", "Service principals require integration:read or integration:write")
	}
	// A token that names an acting end user is a complete standalone
	// bearer for that user's personal data, so it is honored exactly
	// once (and refused outright if it carries no id to track).
	if strings.TrimSpace(principal.DelegatedUserID) != "" &&
		!delegatedTokenUse.consume(principal.TokenID, principal.TokenExpiresAt) {
		return writeError(c, fiber.StatusUnauthorized, "token_replayed", "Delegated credential is single-use and was already used")
	}
	c.Locals(internalLocal, false)
	c.Locals(principalLocal, principal)
	return c.Next()
}

// otpScopeAdmittedOnRoute reports whether a service principal that holds only
// integration:otp:read may pass on this request: it may reach exactly
// POST /api/v1/connections/{id}/actions, matched on the registered route and
// method, never on the body. The service layer (actions.refuseOTPOnlyCaller)
// then limits that token to the mail.otp.read operation. Every other route
// keeps answering 403 integration_scope_required.
func otpScopeAdmittedOnRoute(c *fiber.Ctx, principal Principal) bool {
	if !principal.HasScope(IntegrationOTPReadScope) || c.Method() != fiber.MethodPost {
		return false
	}
	route := c.Route()
	return route != nil && route.Path == OTPActionsRoute
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
