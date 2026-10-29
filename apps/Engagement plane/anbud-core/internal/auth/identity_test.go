package auth

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/anbud/internal/secret"
)

// identityApp answers with the organisation and the user the middleware recorded.
func identityApp(cfg Config) *fiber.App {
	app := fiber.New(fiber.Config{DisableStartupMessage: true})
	app.Get("/x", Middleware(cfg), func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{"org": OrganizationID(c), "user": UserID(c)})
	})
	return app
}

func TestUserIDIsRecordedForAnAuthenticatedRequest(t *testing.T) {
	t.Parallel()

	status, body := call(t, identityApp(Config{APIKey: secret.Value(apiKey)}),
		map[string]string{"X-API-Key": apiKey, "X-Org-ID": "org_1", "X-User-ID": "  user_42 "})
	if status != 200 || !strings.Contains(body, `"user":"user_42"`) || !strings.Contains(body, `"org":"org_1"`) {
		t.Fatalf("status/body = %d %s", status, body)
	}
}

// The user is informational: it says who uploaded something. A value that is not an identifier is
// left out, and the request goes on, because refusing every request over an audit field would take
// the whole service down for one gateway that sends another shape of id.
func TestUserIDThatIsNotAnIdentifierIsLeftOut(t *testing.T) {
	t.Parallel()

	app := identityApp(Config{APIKey: secret.Value(apiKey)})
	for name, user := range map[string]string{
		"space inside": "bad user",
		"punctuation":  "user;DROP",
		"leading dash": "-user",
		"overlong":     strings.Repeat("a", 129),
		"non-ascii":    "brukerø",
		"an email":     "a@b.no",
	} {
		status, body := call(t, app, map[string]string{"X-API-Key": apiKey, "X-User-ID": user})
		if status != 200 || !strings.Contains(body, `"user":""`) {
			t.Errorf("%s: %d %s", name, status, body)
		}
	}
}

func TestUserIDIsNotRecordedWithoutAValidKey(t *testing.T) {
	t.Parallel()

	status, _ := call(t, identityApp(Config{APIKey: secret.Value(apiKey)}), map[string]string{"X-User-ID": "user_42"})
	if status != 401 {
		t.Fatalf("a user header alone must not authenticate (status %d)", status)
	}
}

// Authenticated is the check the server's header hook makes before it accepts a large body, so it
// must agree with the middleware on every request: the same headers, the same answer. Each case also
// says what the answer is, because agreeing is not enough: the two share one reading of the headers.
func TestAuthenticatedAgreesWithTheMiddleware(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		cfg     Config
		headers map[string]string
		want    bool
	}{
		{"no key", Config{APIKey: secret.Value(apiKey)}, nil, false},
		{"wrong key", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": "nope"}, false},
		{"right key on the default header", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": apiKey}, true},
		{"right key on the internal header", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-Internal-Api-Key": apiKey}, true},
		// The configured header is read first, and the internal one only when it is empty: a wrong key
		// on the first is not rescued by a right one on the second, nor spoiled by a wrong one.
		{"wrong key on the default, right on the internal", Config{APIKey: secret.Value(apiKey)},
			map[string]string{"X-API-Key": "nope", "X-Internal-Api-Key": apiKey}, false},
		{"right key on the default, wrong on the internal", Config{APIKey: secret.Value(apiKey)},
			map[string]string{"X-API-Key": apiKey, "X-Internal-Api-Key": "nope"}, true},
		{"empty default, right on the internal", Config{APIKey: secret.Value(apiKey)},
			map[string]string{"X-API-Key": "  ", "X-Internal-Api-Key": apiKey}, true},
		{"right key with stray whitespace", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": "  " + apiKey + " "}, true},
		{"custom header honoured", Config{APIKey: secret.Value(apiKey), APIKeyHeader: "X-Anbud-Key"}, map[string]string{"X-Anbud-Key": apiKey}, true},
		{"custom header configured but default sent", Config{APIKey: secret.Value(apiKey), APIKeyHeader: "X-Anbud-Key"}, map[string]string{"X-API-Key": apiKey}, false},
		{"unset server key, empty header", Config{}, map[string]string{"X-API-Key": ""}, false},
		{"unset server key, any header", Config{}, map[string]string{"X-API-Key": "anything"}, false},
		{"a longer key with the right prefix", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": apiKey + "x"}, false},
		{"a shorter key that is a prefix", Config{APIKey: secret.Value(apiKey)}, map[string]string{"X-API-Key": apiKey[:len(apiKey)-1]}, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			req := httptest.NewRequest(http.MethodGet, "/x", nil)
			for k, v := range tc.headers {
				req.Header.Set(k, v)
			}
			resp, err := identityApp(tc.cfg).Test(req)
			if err != nil {
				t.Fatalf("app.Test: %v", err)
			}
			defer resp.Body.Close()
			if passed := resp.StatusCode == http.StatusOK; passed != tc.want {
				t.Errorf("the middleware answered %d, want passed = %v", resp.StatusCode, tc.want)
			}
			if got := tc.cfg.Authenticated(func(name string) string { return req.Header.Get(name) }); got != tc.want {
				t.Errorf("Authenticated = %v, want %v", got, tc.want)
			}
		})
	}
}

// callWithLines is call with a header sent as several lines.
func callWithLines(t *testing.T, app *fiber.App, headers map[string]string, repeated string, lines ...string) (int, string) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/x", nil)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	for _, v := range lines {
		req.Header.Add(repeated, v)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(body)
}

// The tenant is read from the first line of its header, and a gateway that adds its line to the client's does not
// control which is first. A request that names a tenant twice is refused, whatever the lines say, and a user who is
// named twice is not named.
func TestATenantNamedTwiceIsRefusedAndAUserNamedTwiceIsLeftOut(t *testing.T) {
	t.Parallel()

	app := identityApp(Config{APIKey: secret.Value(apiKey)})
	key := map[string]string{"X-API-Key": apiKey}
	for name, lines := range map[string][]string{
		"two tenants":                {"org_a", "org_b"},
		"the same tenant twice":      {"org_a", "org_a"},
		"an empty line and a tenant": {"", "org_a"},
		"a tenant and an empty line": {"org_a", ""},
		"three lines":                {"org_a", "org_a", "org_a"},
	} {
		status, body := callWithLines(t, app, key, "X-Org-ID", lines...)
		if status != 400 || errorCode(t, body) != "invalid_organization" {
			t.Errorf("%s: %d %s, want 400 invalid_organization", name, status, body)
		}
	}
	// One line is as it was.
	if status, body := callWithLines(t, app, key, "X-Org-ID", "org_a"); status != 200 || !strings.Contains(body, `"org":"org_a"`) {
		t.Errorf("one tenant: %d %s", status, body)
	}

	for name, lines := range map[string][]string{
		"two users":           {"user_1", "user_2"},
		"the same user twice": {"user_1", "user_1"},
		"a user and an empty": {"user_1", ""},
	} {
		status, body := callWithLines(t, app, key, "X-User-ID", lines...)
		if status != 200 || !strings.Contains(body, `"user":""`) {
			t.Errorf("%s: %d %s, want the request to go on and no user recorded", name, status, body)
		}
	}
	if status, body := callWithLines(t, app, key, "X-User-ID", "user_1"); status != 200 || !strings.Contains(body, `"user":"user_1"`) {
		t.Errorf("one user: %d %s", status, body)
	}
}

func TestLinesInHeadCountsTheHeaderLinesOfTheHeadAndNothingElse(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		raw  string
		want int
	}{
		"none":                                {"Host: x\r\nX-Api-Key: k\r\n\r\n", 0},
		"one":                                 {"Host: x\r\nX-Org-ID: org-a\r\n\r\n", 1},
		"any case":                            {"x-org-id: a\r\nX-ORG-ID: b\r\n", 2},
		"space before the colon":              {"X-Org-ID : org-a\r\n", 1},
		"a fold is not a line":                {"X-Zdr: false\r\n true\r\nX-Org-ID: a\r\n", 1},
		"a fold that holds the name":          {"X-Foo: bar\r\n X-Org-ID: org-a\r\n", 0},
		"a tab fold that holds it":            {"X-Foo: bar\r\n\tX-Org-ID: org-a\r\n", 0},
		"the empty line ends it":              {"Host: x\r\n\r\nX-Org-ID: a\r\n", 0},
		"bare newlines":                       {"X-Org-ID: a\nX-Org-ID: b\n", 2},
		"another header that begins the same": {"X-Org-ID-Extra: a\r\n", 0},
		"a value that holds the name":         {"X-Other: X-Org-ID: a\r\n", 0},
		"empty":                               {"", 0},
	} {
		if got := linesInHead([]byte(tc.raw), "X-Org-ID"); got != tc.want {
			t.Errorf("%s: %d lines, want %d", name, got, tc.want)
		}
	}
}
