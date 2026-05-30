package platform

import (
	"context"
	"crypto/subtle"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// PrincipalContextKey is the Fiber locals key for the resolved principal.
const PrincipalContextKey = "principal"

// AuthBridgeConfig configures the auth bridge middleware.
type AuthBridgeConfig struct {
	// AuthClient (optional). If nil, only static API key auth is available.
	AuthClient *AuthClient

	// Static API key — legacy Quarry auth. Still accepted for backward compat
	// and for service-to-service calls that don't carry a Bearer token.
	StaticAPIKey       string
	StaticAPIKeyHeader string // default "X-API-Key"
}

// AuthBridgeMiddleware returns a Fiber handler that resolves the caller's identity.
//
// Auth modes, tried in order:
//  1. Authorization: Bearer <token> → verified against auth-core → full Principal
//  2. X-API-Key / x-internal-api-key → constant-time compare against StaticAPIKey
//     → synthetic Principal with Tier="internal"
//
// On success the resolved *Principal is stored in c.Locals(PrincipalContextKey).
// On failure a 401 JSON error is returned.
func AuthBridgeMiddleware(cfg AuthBridgeConfig) fiber.Handler {
	header := cfg.StaticAPIKeyHeader
	if header == "" {
		header = "X-API-Key"
	}

	return func(c *fiber.Ctx) error {
		// ── Mode 1: Bearer token ──
		authHeader := c.Get("Authorization")
		if strings.HasPrefix(authHeader, "Bearer ") {
			token := strings.TrimSpace(authHeader[len("Bearer "):])
			if token == "" {
				return writeAuthError(c, "Bearer token is empty")
			}

			if cfg.AuthClient == nil {
				return writeAuthError(c, "Bearer auth not configured (AUTH_CORE_URL missing)")
			}

			ctx, cancel := context.WithTimeout(c.UserContext(), 5*time.Second)
			defer cancel()

			principal, err := cfg.AuthClient.VerifyToken(ctx, token)
			if err != nil {
				log.Warn().Err(err).
					Str("event", "auth_bearer_denied").
					Str("path", c.Path()).
					Str("ip", c.IP()).
					Msg("Bearer token verification failed")
				return writeAuthError(c, "invalid or expired token")
			}

			log.Info().
				Str("event", "auth_bearer_granted").
				Str("user_id", principal.UserID).
				Str("org_id", principal.OrganizationID).
				Str("tier", principal.Tier).
				Str("path", c.Path()).
				Msg("Bearer authenticated")

			c.Locals(PrincipalContextKey, principal)
			return c.Next()
		}

		// ── Mode 2: Static API key (legacy + internal) ──
		provided := strings.TrimSpace(c.Get(header))
		if provided == "" {
			// Also accept x-internal-api-key (for cross-service calls from integration-core etc.)
			provided = strings.TrimSpace(c.Get("x-internal-api-key"))
		}

		if provided != "" && cfg.StaticAPIKey != "" {
			if subtle.ConstantTimeCompare([]byte(provided), []byte(cfg.StaticAPIKey)) == 1 {
				log.Info().
					Str("event", "auth_apikey_granted").
					Str("path", c.Path()).
					Str("ip", c.IP()).
					Msg("Static API key authenticated")

				// Build a synthetic principal for static-key callers.
				// OrgID and UserID come from request body if available, otherwise "internal".
				orgID := "internal"
				userID := "internal-service"

				c.Locals(PrincipalContextKey, &Principal{
					UserID:         userID,
					OrganizationID: orgID,
					Role:           "service",
					Tier:           "internal", // internal calls bypass tier limits
				})
				return c.Next()
			}

			log.Warn().
				Str("event", "auth_apikey_denied").
				Str("path", c.Path()).
				Str("ip", c.IP()).
				Msg("Static API key validation failed")
			return writeAuthError(c, "invalid API key")
		}

		return writeAuthError(c, "authentication required — provide Authorization: Bearer <token> or X-API-Key header")
	}
}

// GetPrincipal extracts the resolved principal from Fiber context.
// Returns nil if the request is not authenticated (should never happen after
// the auth bridge middleware).
func GetPrincipal(c *fiber.Ctx) *Principal {
	if p, ok := c.Locals(PrincipalContextKey).(*Principal); ok {
		return p
	}
	return nil
}

func writeAuthError(c *fiber.Ctx, msg string) error {
	return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
		"success":   false,
		"error":     msg,
		"requestId": c.GetRespHeader("X-Request-ID"),
	})
}
