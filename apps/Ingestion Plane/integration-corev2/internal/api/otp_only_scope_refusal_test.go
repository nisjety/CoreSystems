package api

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

// A service token holding only integration:otp:read is admitted on the actions
// route so that it can reach mail.otp.read. Any other operation must be refused
// before the connection's provider token is acquired: acquiring that token can
// refresh and persist it, and a caller the operation refuses must not cause
// that. The refusal does not depend on INTEGRATION_OTP_READ.

// noProviderTransport fails the test if the actions service reaches a provider.
type noProviderTransport struct{ t *testing.T }

func (n noProviderTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	n.t.Errorf("provider %s was called for an operation an OTP-only token must be refused", req.URL.Host)
	return nil, errors.New("provider calls are disabled in tests")
}

func TestOTPOnlyServiceTokenRefusedBeforeProviderTokenForOtherOperations(t *testing.T) {
	for _, otpReadEnabled := range []bool{false, true} {
		cfg, repo, service := testOAuthStack(t)
		vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
		if err != nil {
			t.Fatalf("NewVault error: %v", err)
		}
		// An org-wide (non-personal) connection whose access token has expired
		// and that has no refresh token: acquiring its provider token fails, so a
		// call that reaches the token step answers with that failure instead of
		// the executor refusal.
		const connectionID = "conn-org-slack-otp-only"
		encryptedAccessToken, err := vault.Encrypt("access-token", []byte(connectionID))
		if err != nil {
			t.Fatalf("Encrypt error: %v", err)
		}
		if _, err := repo.UpsertConnection(t.Context(), store.Connection{
			ID:                   connectionID,
			ProviderKey:          "slack",
			ConnectorType:        "slack",
			OrganizationID:       "org-1",
			WorkspaceID:          "org-1",
			UserID:               "owner-1",
			Status:               "active",
			Capabilities:         []string{"channels.read"},
			EncryptedAccessToken: encryptedAccessToken,
			AccessTokenExpiresAt: time.Now().Add(-time.Hour),
		}); err != nil {
			t.Fatalf("UpsertConnection error: %v", err)
		}
		otp := actions.NewService(cfg, &http.Client{Transport: noProviderTransport{t: t}})
		otp.SetOTPReadEnabled(otpReadEnabled)
		app := NewServer(ServerConfig{
			Config: cfg, Repo: repo, OAuth: service, Actions: otp,
			Auth: fakeVerifier{principal: auth.Principal{
				UserID: "service:quarry-edge", OrganizationID: "org-1", PrincipalType: "service",
				Scopes: []string{"integration:otp:read"},
			}},
		})
		req := httptest.NewRequest("POST", "/api/v1/connections/"+connectionID+"/actions", strings.NewReader(`{"operation":"channels.list"}`))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer otp-only-test-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		body, err := io.ReadAll(resp.Body)
		resp.Body.Close()
		if err != nil {
			t.Fatalf("read response body: %v", err)
		}
		var decoded struct {
			Error struct {
				Code string `json:"code"`
			} `json:"error"`
		}
		if err := json.Unmarshal(body, &decoded); err != nil {
			t.Fatalf("decode API error: %v", err)
		}
		if resp.StatusCode != fiber.StatusForbidden || decoded.Error.Code != "otp_caller_not_allowed" {
			t.Fatalf("flag=%v: status = %d, code = %q, want 403 otp_caller_not_allowed (the provider token step ran before the refusal): %s",
				otpReadEnabled, resp.StatusCode, decoded.Error.Code, body)
		}
		stored, err := repo.GetConnection(t.Context(), connectionID)
		if err != nil {
			t.Fatalf("GetConnection error: %v", err)
		}
		if stored.EncryptedAccessToken != encryptedAccessToken {
			t.Fatalf("flag=%v: the stored provider token changed during a refused call", otpReadEnabled)
		}
	}
}
