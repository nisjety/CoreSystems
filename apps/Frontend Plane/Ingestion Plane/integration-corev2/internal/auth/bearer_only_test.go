package auth

import (
	"context"
	"io"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"
)

// stubTokenVerifier stands in for auth-core's verifier only: the bearer path itself (the
// Authorization parsing, the principal checks and the refusals) is the real code under test.
type stubTokenVerifier struct {
	principal Principal
	err       error
	calls     int
}

func (v *stubTokenVerifier) VerifyToken(context.Context, string) (Principal, error) {
	v.calls++
	return v.principal, v.err
}

func bearerOnlyTestApp(cfg Config) *fiber.App {
	app := fiber.New()
	app.Get("/guarded", BearerOnly(cfg), func(c *fiber.Ctx) error {
		principal, ok := PrincipalFromContext(c)
		if !ok {
			return c.SendStatus(fiber.StatusInternalServerError)
		}
		return c.JSON(fiber.Map{"userId": principal.UserID, "internal": IsInternalCall(c)})
	})
	return app
}

func bearerOnlyRequest(t *testing.T, app *fiber.App, headers map[string]string) (int, string) {
	t.Helper()
	req := httptest.NewRequest(fiber.MethodGet, "/guarded", nil)
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(raw)
}

func TestBearerOnlyRefusesTheInternalKeyWithoutAToken(t *testing.T) {
	verifier := &stubTokenVerifier{principal: Principal{UserID: "user-1", OrganizationID: "org-1", PrincipalType: "user"}}
	app := bearerOnlyTestApp(Config{APIKey: "internal-secret", APIKeyHeader: "X-Internal-API-Key", TokenVerifier: verifier, AllowLegacyTenantKey: true})

	status, body := bearerOnlyRequest(t, app, map[string]string{"X-Internal-API-Key": "internal-secret", "X-User-ID": "user-1"})
	if status != fiber.StatusUnauthorized {
		t.Fatalf("internal key without a bearer token: status %d, want 401 (body %s)", status, body)
	}
	if verifier.calls != 0 {
		t.Fatalf("the verifier was consulted for a request with no bearer token")
	}
}

func TestBearerOnlyRefusesARequestWithNoCredential(t *testing.T) {
	verifier := &stubTokenVerifier{principal: Principal{UserID: "user-1", OrganizationID: "org-1", PrincipalType: "user"}}
	app := bearerOnlyTestApp(Config{APIKey: "internal-secret", APIKeyHeader: "X-Internal-API-Key", TokenVerifier: verifier})

	status, _ := bearerOnlyRequest(t, app, nil)
	if status != fiber.StatusUnauthorized {
		t.Fatalf("no credential: status %d, want 401", status)
	}
}

func TestBearerOnlyAcceptsAVerifiedUserTokenAndIsNeverAnInternalCall(t *testing.T) {
	verifier := &stubTokenVerifier{principal: Principal{UserID: "user-1", OrganizationID: "org-1", PrincipalType: "user"}}
	app := bearerOnlyTestApp(Config{APIKey: "internal-secret", APIKeyHeader: "X-Internal-API-Key", TokenVerifier: verifier})

	status, body := bearerOnlyRequest(t, app, map[string]string{"Authorization": "Bearer token-1"})
	if status != fiber.StatusOK {
		t.Fatalf("verified token: status %d, body %s", status, body)
	}
	if !strings.Contains(body, `"userId":"user-1"`) || !strings.Contains(body, `"internal":false`) {
		t.Fatalf("verified token should yield the token's own user as a non-internal call, got %s", body)
	}
}

func TestBearerOnlyIgnoresAnInternalKeyThatAcompaniesAToken(t *testing.T) {
	verifier := &stubTokenVerifier{principal: Principal{UserID: "user-1", OrganizationID: "org-1", PrincipalType: "user"}}
	app := bearerOnlyTestApp(Config{APIKey: "internal-secret", APIKeyHeader: "X-Internal-API-Key", TokenVerifier: verifier})

	status, body := bearerOnlyRequest(t, app, map[string]string{"Authorization": "Bearer token-1", "X-Internal-API-Key": "internal-secret", "X-User-ID": "someone-else"})
	if status != fiber.StatusOK {
		t.Fatalf("token with an internal key: status %d, body %s", status, body)
	}
	if strings.Contains(body, "someone-else") || !strings.Contains(body, `"internal":false`) {
		t.Fatalf("the internal key must never name the user, got %s", body)
	}
}

func TestBearerOnlyRefusesAnUnverifiedToken(t *testing.T) {
	verifier := &stubTokenVerifier{err: NewError(fiber.StatusUnauthorized, "unauthorized", "Token verification failed")}
	app := bearerOnlyTestApp(Config{APIKey: "internal-secret", APIKeyHeader: "X-Internal-API-Key", TokenVerifier: verifier})

	status, _ := bearerOnlyRequest(t, app, map[string]string{"Authorization": "Bearer forged"})
	if status != fiber.StatusUnauthorized {
		t.Fatalf("unverified token: status %d, want 401", status)
	}
}
