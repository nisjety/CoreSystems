package api

import (
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
)

// The acting user that mail.otp.read reads from params.acting_user_id is an
// executor-only delegation: the owner rule that honors it belongs to a service
// token holding integration:otp:read. A service token without that scope (here
// integration:read, as data-plane and model-plane hold) names no end user, so the
// body field is not a delegation. The ordinary owner rule must refuse it before
// the connection's provider token is acquired, even while INTEGRATION_OTP_READ
// is on.
func TestMailOTPReadOwnerRuleNeedsOTPScopedService(t *testing.T) {
	readOnlyService := auth.Principal{
		UserID: "service:data-plane", OrganizationID: "org-1", PrincipalType: "service",
		Scopes: []string{"integration:read"},
	}
	app, providerCalled := newOTPReadTestApp(t, readOnlyService, true,
		personalMicrosoftConnection("conn-private", "owner-1"))

	resp := postOTPReadAction(t, app, "conn-private",
		`{"operation":"mail.otp.read","params":{"acting_user_id":"owner-1"}}`)
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != personalConnectionNoDelegatedUserCode {
		t.Fatalf("error code = %q, want %q", code, personalConnectionNoDelegatedUserCode)
	}
	if providerCalled() {
		t.Fatal("provider was reached for mail.otp.read from a service token without integration:otp:read")
	}
}
