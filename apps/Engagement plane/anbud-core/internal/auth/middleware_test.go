package auth

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/anbud/internal/secret"
)

const apiKey = "internal-api-key-value"

func newApp(cfg Config, requireOrg bool) *fiber.App {
	app := fiber.New(fiber.Config{DisableStartupMessage: true})
	handlers := []fiber.Handler{Middleware(cfg)}
	if requireOrg {
		handlers = append(handlers, RequireOrganization())
	}
	handlers = append(handlers, func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{"org": OrganizationID(c)})
	})
	app.Get("/x", handlers...)
	return app
}

func call(t *testing.T, app *fiber.App, headers map[string]string) (int, string) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/x", nil)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(body)
}

func errorCode(t *testing.T, body string) string {
	t.Helper()
	var envelope struct {
		Error struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(body), &envelope); err != nil {
		t.Fatalf("body is not an error envelope: %q (%v)", body, err)
	}
	if envelope.Error.Message == "" {
		t.Fatalf("error envelope has no message: %q", body)
	}
	return envelope.Error.Code
}

func TestMiddlewareAuthenticatesTheAPIKey(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		cfg        Config
		headers    map[string]string
		wantStatus int
		wantCode   string
	}{
		{"no key", Config{APIKey: secret.Value(apiKey)}, nil, 401, "unauthorized"},
		{"wrong key", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": "nope"}, 401, "unauthorized"},
		{"right key on the default header", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": apiKey}, 200, ""},
		{"right key on the internal header", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-Internal-Api-Key": apiKey}, 200, ""},
		{"right key with stray whitespace", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": "  " + apiKey + " "}, 200, ""},
		{"custom header honoured", Config{APIKey: secret.Value(apiKey), APIKeyHeader: "X-Anbud-Key"}, map[string]string{"X-Anbud-Key": apiKey}, 200, ""},
		{"custom header configured but default sent", Config{APIKey: secret.Value(apiKey), APIKeyHeader: "X-Anbud-Key"}, map[string]string{"X-API-Key": apiKey}, 401, "unauthorized"},
		{"unset server key fails closed even for an empty header", Config{}, map[string]string{"X-API-Key": ""}, 401, "unauthorized"},
		{"unset server key fails closed for any header", Config{}, map[string]string{"X-API-Key": "anything"}, 401, "unauthorized"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			status, body := call(t, newApp(tc.cfg, false), tc.headers)
			if status != tc.wantStatus {
				t.Fatalf("status = %d, want %d (%s)", status, tc.wantStatus, body)
			}
			if tc.wantCode != "" && errorCode(t, body) != tc.wantCode {
				t.Fatalf("error code mismatch: %s", body)
			}
			if strings.Contains(body, apiKey) {
				t.Fatalf("response echoed the key: %s", body)
			}
		})
	}
}

func TestMiddlewareDoesNotRequireAnOrganizationByItself(t *testing.T) {
	t.Parallel()

	status, body := call(t, newApp(Config{APIKey: secret.Value(apiKey)}, false), map[string]string{"X-API-Key": apiKey})
	if status != 200 || !strings.Contains(body, `"org":""`) {
		t.Fatalf("status/body = %d %s", status, body)
	}
}

func TestRequireOrganization(t *testing.T) {
	t.Parallel()

	app := newApp(Config{APIKey: secret.Value(apiKey)}, true)

	status, body := call(t, app, map[string]string{"X-API-Key": apiKey})
	if status != 400 || errorCode(t, body) != "organization_required" {
		t.Fatalf("without org: %d %s", status, body)
	}

	status, body = call(t, app, map[string]string{"X-API-Key": apiKey, "X-Org-ID": "  org_123 "})
	if status != 200 || !strings.Contains(body, `"org":"org_123"`) {
		t.Fatalf("with org: %d %s", status, body)
	}
}

func TestOrganizationHeaderIsValidated(t *testing.T) {
	t.Parallel()

	app := newApp(Config{APIKey: secret.Value(apiKey)}, true)
	for name, org := range map[string]string{
		"space inside":     "bad org",
		"punctuation":      "org;DROP",
		"leading dash":     "-org",
		"path traversal":   "../org",
		"overlong":         strings.Repeat("a", 129),
		"non-ascii letter": "orgé",
	} {
		status, body := call(t, app, map[string]string{"X-API-Key": apiKey, "X-Org-ID": org})
		if status != 400 || errorCode(t, body) != "invalid_organization" {
			t.Errorf("%s: %d %s", name, status, body)
		}
	}
}

func TestOrganizationIsNotTrustedWithoutAValidKey(t *testing.T) {
	t.Parallel()

	status, _ := call(t, newApp(Config{APIKey: secret.Value(apiKey)}, true), map[string]string{"X-Org-ID": "org_123"})
	if status != 401 {
		t.Fatalf("an org header alone must not authenticate (status %d)", status)
	}
}
