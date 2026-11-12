package api

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// mail.otp.read is dark while INTEGRATION_OTP_READ is off: its OTP owner rule
// must not run, so a dark call answers the same whatever acting_user_id it
// names, and an admitted dark call gets the unsupported-operation answer. While
// the flag is on, the OTP owner rule still refuses a mismatched acting user.

// otpExecutorPrincipal is the executor's narrow service token: integration:otp:read
// only, with no end user of its own (actions.refuseOTPOnlyCaller).
func otpExecutorPrincipal() auth.Principal {
	return auth.Principal{
		UserID: "service:quarry-edge", OrganizationID: "org-1", PrincipalType: "service",
		Scopes: []string{"integration:otp:read"},
	}
}

// newOTPReadTestApp serves the actions route for principal with mail.otp.read
// set to otpReadEnabled. Microsoft Graph points at a local stub; the returned
// func reports whether the provider was reached.
func newOTPReadTestApp(t *testing.T, principal auth.Principal, otpReadEnabled bool, connections ...store.Connection) (*fiber.App, func() bool) {
	t.Helper()
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"value":[]}`))
	}))
	t.Cleanup(providerServer.Close)

	cfg, repo, service := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	for _, connection := range connections {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("UpsertConnection error: %v", err)
		}
	}
	otp := actions.NewService(cfg, providerServer.Client())
	otp.SetOTPReadEnabled(otpReadEnabled)
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth: fakeVerifier{principal: principal}, Actions: otp,
	})
	return app, func() bool { return providerCalled }
}

func postOTPReadAction(t *testing.T, app *fiber.App, connectionID, body string) *http.Response {
	t.Helper()
	req := httptest.NewRequest("POST", "/api/v1/connections/"+connectionID+"/actions", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer otp-test-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	return resp
}

// With the flag off, the executor's calls get the ordinary personal-connection
// answer that any other operation gets, and never the otp_owner_mismatch code
// that would single the dark operation out.
func TestMailOTPReadDarkWhileFlagOffIgnoresActingUser(t *testing.T) {
	app, providerCalled := newOTPReadTestApp(t, otpExecutorPrincipal(), false,
		personalMicrosoftConnection("conn-private", "owner-1"))
	bodies := map[string]string{
		"no acting user":    `{"operation":"mail.otp.read"}`,
		"wrong acting user": `{"operation":"mail.otp.read","params":{"acting_user_id":"someone-else"}}`,
		"owner acting user": `{"operation":"mail.otp.read","params":{"acting_user_id":"owner-1"}}`,
	}
	for name, body := range bodies {
		resp := postOTPReadAction(t, app, "conn-private", body)
		if resp.StatusCode != fiber.StatusForbidden {
			t.Fatalf("%s: status = %d, want 403", name, resp.StatusCode)
		}
		if code := readAPIErrorCode(t, resp); code != personalConnectionNoDelegatedUserCode {
			t.Fatalf("%s: error code = %q, want %q", name, code, personalConnectionNoDelegatedUserCode)
		}
	}
	if providerCalled() {
		t.Fatal("provider was reached for a dark mail.otp.read")
	}
}

// An owner whose connection does not grant mail.read still gets the dark
// unsupported-operation answer, not the capability answer, once the owner rule
// admits the call.
func TestMailOTPReadDarkAnswersUnsupportedOnceOwnerIsAdmitted(t *testing.T) {
	mailless := personalMicrosoftConnection("conn-mailless", "owner-1")
	mailless.Capabilities = []string{"profile.read"}
	owner := auth.Principal{UserID: "owner-1", OrganizationID: "org-1", Role: "member", PrincipalType: "user"}
	app, providerCalled := newOTPReadTestApp(t, owner, false, mailless)

	resp := postOTPReadAction(t, app, "conn-mailless", `{"operation":"mail.otp.read"}`)
	if resp.StatusCode != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != "operation_not_supported" {
		t.Fatalf("error code = %q, want operation_not_supported", code)
	}
	if providerCalled() {
		t.Fatal("provider was reached for a dark mail.otp.read")
	}
}

// While the flag is on, the executor owner rule still refuses a mismatched
// acting user before the provider is reached.
func TestMailOTPReadLiveKeepsOwnerRuleForMismatchedActingUser(t *testing.T) {
	app, providerCalled := newOTPReadTestApp(t, otpExecutorPrincipal(), true,
		personalMicrosoftConnection("conn-private", "owner-1"))

	resp := postOTPReadAction(t, app, "conn-private", `{"operation":"mail.otp.read","params":{"acting_user_id":"someone-else"}}`)
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != otpOwnerMismatchCode {
		t.Fatalf("error code = %q, want %q", code, otpOwnerMismatchCode)
	}
	if providerCalled() {
		t.Fatal("provider was reached for a mismatched mail.otp.read acting user")
	}
}
