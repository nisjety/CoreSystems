package api

import (
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Stable error codes for the owner-ruling-5 personal-connection gate, so
// callers (gateway BFF, execution-core, the SPA) can branch on `code`
// rather than parse `message`. Kept distinct from "forbidden" (wrong org)
// and "connection_not_found" so a caller can tell "this connection isn't
// yours" apart from every other 403/404 this API already returns.
const (
	// personalConnectionForbiddenCode is returned when the caller names a
	// real end user, but it is not the user who connected this personal
	// connection.
	personalConnectionForbiddenCode = "personal_connection_forbidden"
	// personalConnectionNoDelegatedUserCode is returned when the caller is
	// a service call that names no end user at all (a bare service
	// credential, or a scoped service token with no delegation) trying to
	// reach a personal connection. Distinct from the above so a caller can
	// tell "wrong user" apart from "no user named at all".
	personalConnectionNoDelegatedUserCode = "personal_connection_no_delegated_user"
)

// assertConnectionAccess enforces owner ruling (5): a personal Microsoft/
// Google connection (mail, OneDrive/Drive, calendar, contacts) is usable
// ONLY by the user who connected it, including when an agent acts "for"
// that user — never another org member, an org admin/owner, or a service
// call that names no end user. Org-wide connectors (Slack, GitHub, Notion,
// Shopify, ...) are unaffected: they keep exactly the org-level check that
// ran here before this function existed.
//
// The acting user is the ordinary bearer/session identity
// (Principal.EffectiveUserID). Every route that loads a *specific*
// connection for something other than executing a provider action — get,
// status, capabilities, discovery/token access, disconnect, sync,
// inbox-sync, inbox-history — calls this instead of the bare
// auth.AssertOrgAccess it replaces.
//
// executeConnectionAction (the /actions route, shared by the picker's
// browse/content and execution-core's execute_provider_action) does NOT
// use this directly: a WRITE's acting user is its cryptographically
// verified write-attestation ActorID, not the bearer principal (which for
// a write is typically a presenting service, e.g. conversation-core or
// execution-core, never the human). It calls assertPersonalConnectionOwner
// itself with the right identity for each case — see the comments there.
func assertConnectionAccess(c *fiber.Ctx, connection store.Connection) error {
	if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
		return err
	}
	principal, ok := auth.PrincipalFromContext(c)
	actingUserID := ""
	if ok {
		actingUserID = principal.EffectiveUserID(auth.IsInternalCall(c))
	}
	return assertPersonalConnectionOwner(connection, actingUserID, ok)
}

// assertPersonalConnectionOwner is the owner-ruling-5 comparison itself,
// factored out of assertConnectionAccess so a caller that resolves the
// acting user differently (see executeConnectionAction) can still share
// the one piece of logic that actually decides "does this identity own
// this personal connection". `authenticated` distinguishes "no principal
// on the request at all" (unauthenticated — shouldn't normally happen,
// every route here sits behind internalOrBearerAuth) from "a principal
// exists but names no end user" (the fail-closed service-call case).
func assertPersonalConnectionOwner(connection store.Connection, actingUserID string, authenticated bool) error {
	if !providers.IsPersonalProvider(connection.ProviderKey) {
		return nil
	}
	if !authenticated {
		return auth.NewError(fiber.StatusUnauthorized, "unauthorized", "Request is not authenticated")
	}
	actingUserID = strings.TrimSpace(actingUserID)
	if actingUserID == "" {
		return auth.NewError(fiber.StatusForbidden, personalConnectionNoDelegatedUserCode,
			"This is a personal connection. A service call must name the acting user to read or act through it.")
	}
	if actingUserID != strings.TrimSpace(connection.UserID) {
		return auth.NewError(fiber.StatusForbidden, personalConnectionForbiddenCode,
			"This personal connection belongs to a different user.")
	}
	return nil
}

// filterPersonalConnectionsForCaller drops, from a connection LISTING
// (never from a single by-id lookup, which uses assertConnectionAccess
// instead), every personal connection that does not belong to the caller.
// Org-wide connections pass through unchanged. A caller with no named end
// user (see auth.Principal.EffectiveUserID) sees no personal connections at
// all — the same fail-closed rule assertConnectionAccess applies, just
// expressed as omission instead of a 403 (the right shape for a list:
// "these are the connections you may use", not an error).
func filterPersonalConnectionsForCaller(c *fiber.Ctx, connections []store.Connection) []store.Connection {
	effectiveUserID := ""
	if principal, ok := auth.PrincipalFromContext(c); ok {
		effectiveUserID = principal.EffectiveUserID(auth.IsInternalCall(c))
	}
	filtered := make([]store.Connection, 0, len(connections))
	for _, connection := range connections {
		if providers.IsPersonalProvider(connection.ProviderKey) &&
			(effectiveUserID == "" || effectiveUserID != strings.TrimSpace(connection.UserID)) {
			continue
		}
		filtered = append(filtered, connection)
	}
	return filtered
}
