package controlplane

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/oauth"
)

// AuthOAuthClient asks auth-core to refresh a Better Auth sign-in credential on
// integration-corev2's behalf.
//
// Control Plane owns the Microsoft sign-in identity: the refresh token minted
// at login belongs to auth-core's Azure app registration and can only be
// redeemed with that app's client secret. integration-corev2 therefore never
// stores that refresh token; it stores the Better Auth account row id as the
// connection's `control_plane_token_ref` and, when its own provider refresh
// fails or is absent, asks auth-core's `/internal/oauth/refresh` for a fresh
// delegated access token. auth-core authorizes the call with a scoped service
// principal (`oauth:token:refresh`) — see AUTH_INTERNAL_SERVICE_CREDENTIALS in
// the Control Plane compose.
type AuthOAuthClient struct {
	baseURL      string
	credentialID string
	principal    string
	serviceToken string
	httpClient   *http.Client
}

// NewAuthOAuthClient returns nil when the scoped credential is not configured
// so callers can treat "no Control Plane token source" as a plain nil.
func NewAuthOAuthClient(cfg config.Config, httpClient *http.Client) *AuthOAuthClient {
	token := strings.TrimSpace(cfg.AuthCoreOAuthServiceToken)
	base := strings.TrimRight(strings.TrimSpace(cfg.AuthCoreURL), "/")
	if token == "" || base == "" {
		return nil
	}
	return &AuthOAuthClient{
		baseURL:      base,
		credentialID: firstNonBlank(cfg.AuthCoreOAuthCredentialID, "integration-core-primary"),
		principal:    firstNonBlank(cfg.AuthCoreOAuthPrincipal, "integration-core"),
		serviceToken: token,
		httpClient:   withDefaultClient(httpClient),
	}
}

type authOAuthRefreshResponse struct {
	Refreshed   bool   `json:"refreshed"`
	AccessToken string `json:"access_token"`
	ExpiresAt   string `json:"expires_at"`
	Scope       string `json:"scope"`
	Code        string `json:"code"`
	Error       string `json:"error"`
}

// RefreshDelegatedToken implements oauth.ControlPlaneTokenSource.
func (c *AuthOAuthClient) RefreshDelegatedToken(ctx context.Context, tokenRef string) (oauth.DelegatedToken, error) {
	tokenRef = strings.TrimSpace(tokenRef)
	if c == nil {
		return oauth.DelegatedToken{}, oauth.ErrControlPlaneTokenSourceUnavailable
	}
	if tokenRef == "" {
		return oauth.DelegatedToken{}, errors.New("control plane token ref is required")
	}
	body, err := json.Marshal(map[string]string{"tokenRef": tokenRef})
	if err != nil {
		return oauth.DelegatedToken{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/internal/oauth/refresh", bytes.NewReader(body))
	if err != nil {
		return oauth.DelegatedToken{}, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	req.Header.Set("x-service-credential-id", c.credentialID)
	req.Header.Set("x-service-principal", c.principal)
	req.Header.Set("x-service-auth", c.serviceToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return oauth.DelegatedToken{}, fmt.Errorf("auth-core oauth refresh request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		// The principal itself is rejected: a configuration fault, not a
		// provider outcome. Surface it as unavailable so callers keep their
		// own error rather than flipping the connection to needs_refresh.
		return oauth.DelegatedToken{}, fmt.Errorf("%w: auth-core returned HTTP %d", oauth.ErrControlPlaneTokenSourceUnavailable, resp.StatusCode)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return oauth.DelegatedToken{}, fmt.Errorf("auth-core oauth refresh returned HTTP %d", resp.StatusCode)
	}
	var payload authOAuthRefreshResponse
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return oauth.DelegatedToken{}, fmt.Errorf("decode auth-core oauth refresh: %w", err)
	}
	if !payload.Refreshed || strings.TrimSpace(payload.AccessToken) == "" {
		code := firstNonBlank(payload.Code, "refresh_failed")
		return oauth.DelegatedToken{}, &oauth.ControlPlaneRefreshError{Code: code, Detail: payload.Error}
	}
	expiresAt, err := time.Parse(time.RFC3339, strings.TrimSpace(payload.ExpiresAt))
	if err != nil || expiresAt.IsZero() {
		// Microsoft access tokens live about an hour; a missing expiry must
		// never be read as "never expires".
		expiresAt = time.Now().UTC().Add(55 * time.Minute)
	}
	return oauth.DelegatedToken{
		AccessToken: payload.AccessToken,
		ExpiresAt:   expiresAt,
		Scopes:      oauth.SplitGrantedScopes(payload.Scope),
	}, nil
}

func firstNonBlank(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}
