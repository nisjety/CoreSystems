package auth

import (
	"context"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
)

func TestInternalOrBearerRejectsSharedKeyForTenantRoutesByDefault(t *testing.T) {
	app := fiber.New()
	app.Get("/tenant", InternalOrBearer(Config{APIKey: "synthetic-shared-key"}), func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusNoContent)
	})
	request := httptest.NewRequest("GET", "/tenant", nil)
	request.Header.Set("X-Internal-API-Key", "synthetic-shared-key")
	request.Header.Set("X-Org-ID", "org-forged")

	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	if response.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.StatusCode, fiber.StatusForbidden)
	}
}

func TestInternalOrBearerAcceptsVerifiedTenantPrincipal(t *testing.T) {
	app := fiber.New()
	app.Get("/tenant", InternalOrBearer(Config{
		APIKey: "synthetic-shared-key",
		TokenVerifier: staticVerifier{principal: Principal{
			UserID:         "user-test",
			OrganizationID: "org-test",
		}},
	}), func(c *fiber.Ctx) error {
		principal, ok := PrincipalFromContext(c)
		if !ok || principal.OrganizationID != "org-test" {
			return c.SendStatus(fiber.StatusInternalServerError)
		}
		return c.SendStatus(fiber.StatusNoContent)
	})
	request := httptest.NewRequest("GET", "/tenant", nil)
	request.Header.Set("Authorization", "Bearer verified-token")

	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	if response.StatusCode != fiber.StatusNoContent {
		t.Fatalf("status = %d, want %d", response.StatusCode, fiber.StatusNoContent)
	}
}

// A token that names an acting end user is a complete standalone bearer for
// that user's personal data. Auth Core stamps every one with a unique jti and
// this middleware must honor each exactly once, or a captured copy is
// replayable for the rest of its lifetime. The package-level guard is shared
// by every InternalOrBearer instance, so each test uses ids of its own.
func delegatedTenantApp(principal Principal) *fiber.App {
	app := fiber.New()
	app.Get("/tenant", InternalOrBearer(Config{
		APIKey:        "synthetic-shared-key",
		TokenVerifier: staticVerifier{principal: principal},
	}), func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusNoContent)
	})
	return app
}

func tenantStatus(t *testing.T, app *fiber.App) int {
	t.Helper()
	request := httptest.NewRequest("GET", "/tenant", nil)
	request.Header.Set("Authorization", "Bearer verified-token")
	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	return response.StatusCode
}

func delegatedPrincipal(tokenID string) Principal {
	return Principal{
		UserID:          "service:model-execution",
		OrganizationID:  "org-test",
		PrincipalType:   "service",
		DelegatedUserID: "user-1",
		TokenID:         tokenID,
		TokenExpiresAt:  time.Now().Add(90 * time.Second),
	}
}

func TestInternalOrBearerHonorsADelegatedTokenExactlyOnce(t *testing.T) {
	app := delegatedTenantApp(delegatedPrincipal("mw-once-1"))
	if got := tenantStatus(t, app); got != fiber.StatusNoContent {
		t.Fatalf("first use status = %d, want %d", got, fiber.StatusNoContent)
	}
	if got := tenantStatus(t, app); got != fiber.StatusUnauthorized {
		t.Fatalf("replayed delegated token status = %d, want %d", got, fiber.StatusUnauthorized)
	}
}

func TestInternalOrBearerRefusesADelegatedTokenThatCarriesNoID(t *testing.T) {
	app := delegatedTenantApp(delegatedPrincipal(""))
	if got := tenantStatus(t, app); got != fiber.StatusUnauthorized {
		t.Fatalf("id-less delegated token status = %d, want %d", got, fiber.StatusUnauthorized)
	}
}

func TestInternalOrBearerAcceptsDistinctDelegatedTokensIndependently(t *testing.T) {
	for _, id := range []string{"mw-distinct-1", "mw-distinct-2"} {
		if got := tenantStatus(t, delegatedTenantApp(delegatedPrincipal(id))); got != fiber.StatusNoContent {
			t.Fatalf("token %s status = %d, want %d", id, got, fiber.StatusNoContent)
		}
	}
}

// Regression pin: tokens that name no acting user (interactive sessions and
// plain org-scoped service tokens, none of which carry a jti) are unaffected.
func TestInternalOrBearerDoesNotLimitTokensThatNameNoActingUser(t *testing.T) {
	for name, principal := range map[string]Principal{
		"interactive user":    {UserID: "user-1", OrganizationID: "org-test", PrincipalType: "user"},
		"plain service token": {UserID: "service:model-execution", OrganizationID: "org-test", PrincipalType: "service"},
	} {
		app := delegatedTenantApp(principal)
		for call := 0; call < 3; call++ {
			if got := tenantStatus(t, app); got != fiber.StatusNoContent {
				t.Fatalf("%s call %d status = %d, want %d", name, call, got, fiber.StatusNoContent)
			}
		}
	}
}

type staticVerifier struct {
	principal Principal
}

func (v staticVerifier) VerifyToken(context.Context, string) (Principal, error) {
	return v.principal, nil
}

// Owner ruling (security-personal-connections, 2026-09-28): EffectiveUserID
// is the single place that decides "who is this request acting for" for the
// personal-connection ownership gate (internal/api.assertConnectionAccess).
// Every branch of that decision is pinned here, including the isInternalCall
// split — PrincipalType alone cannot tell apart the legacy internal-API-key
// path (where UserID IS the delegation) from a verified service-principal
// bearer token (where UserID is the service's own identity, never a human).
func TestPrincipalEffectiveUserID(t *testing.T) {
	cases := []struct {
		name           string
		principal      Principal
		isInternalCall bool
		want           string
	}{
		{
			name:           "interactive session principal always uses its own UserID",
			principal:      Principal{PrincipalType: "user", UserID: "user-1"},
			isInternalCall: false,
			want:           "user-1",
		},
		{
			name:           "bearer service principal with a verified acting_user_id delegation uses it",
			principal:      Principal{PrincipalType: "service", UserID: "service:execution-core", DelegatedUserID: "user-1"},
			isInternalCall: false,
			want:           "user-1",
		},
		{
			name:           "bearer service principal never falls back to its own service identity",
			principal:      Principal{PrincipalType: "service", UserID: "service:execution-core"},
			isInternalCall: false,
			want:           "",
		},
		{
			name:           "legacy internal-key synthetic principal with a caller-supplied userId is delegated",
			principal:      Principal{PrincipalType: "service", UserID: "user-1"},
			isInternalCall: true,
			want:           "user-1",
		},
		{
			name:           "legacy internal-key synthetic principal with no userId names no one (the sentinel)",
			principal:      Principal{PrincipalType: "service", UserID: internalUserID},
			isInternalCall: true,
			want:           "",
		},
		{
			name: "a stray DelegatedUserID is ignored on the legacy internal-key path " +
				"(that path's delegation is UserID itself, never this field)",
			principal:      Principal{PrincipalType: "service", UserID: internalUserID, DelegatedUserID: "user-1"},
			isInternalCall: true,
			want:           "",
		},
		{
			name:           "an unauthenticated/zero-value principal names no one",
			principal:      Principal{},
			isInternalCall: false,
			want:           "",
		},
		{
			name:           "whitespace-only UserID on a user principal is treated as empty",
			principal:      Principal{PrincipalType: "user", UserID: "   "},
			isInternalCall: false,
			want:           "",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.principal.EffectiveUserID(tc.isInternalCall); got != tc.want {
				t.Fatalf("EffectiveUserID(%v) = %q, want %q", tc.isInternalCall, got, tc.want)
			}
		})
	}
}
