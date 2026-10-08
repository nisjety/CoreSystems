package api

import (
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
)

// capabilityNotGrantedError is a capability-set update that names capabilities
// whose scopes the connection's token was never granted.
type capabilityNotGrantedError struct {
	capabilities []string
}

func (e *capabilityNotGrantedError) Error() string {
	return "these capabilities need provider permissions that were never granted to this connection; reconnect to grant them: " + strings.Join(e.capabilities, ", ")
}

// scopesAllGranted reports whether every scope in required appears in granted.
// Scope names are compared case-insensitively, as everywhere else scopes are
// matched, and a capability that needs no scopes is always granted.
func scopesAllGranted(required, granted []string) bool {
	held := make(map[string]struct{}, len(granted))
	for _, scope := range granted {
		held[strings.ToLower(strings.TrimSpace(scope))] = struct{}{}
	}
	for _, scope := range required {
		if _, ok := held[strings.ToLower(strings.TrimSpace(scope))]; !ok {
			return false
		}
	}
	return true
}

func isProviderAuthRejection(err error) bool {
	_, ok := actions.AsProviderAuthError(err)
	return ok
}

// reconnectRequiredError answers a provider action that the provider refused
// for the connection's credential or scopes. It is a 409 like the other
// "reconnect this connection" answers of this service, with a stable code and
// machine-readable details so a client can offer a reconnect (for example "to
// grant edit access") without parsing the message. The provider's own message
// is never forwarded.
func reconnectRequiredError(c *fiber.Ctx, err error) error {
	rejection, _ := actions.AsProviderAuthError(err)
	message := "The provider refused this request, which usually means the connection was not granted the access it needs. Reconnect the account to grant it, then try again."
	if rejection.Status == fiber.StatusUnauthorized {
		message = "The provider no longer accepts this connection's sign-in. Reconnect the account, then try again."
	}
	details := fiber.Map{
		"reconnectRequired": true,
		"providerStatus":    rejection.Status,
	}
	if rejection.Code != "" {
		details["providerCode"] = rejection.Code
	}
	return apiErrorWithDetails(c, fiber.StatusConflict, "reconnect_required", message, details)
}
