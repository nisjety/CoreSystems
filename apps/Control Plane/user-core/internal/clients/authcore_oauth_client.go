// Package clients — auth-core OAuth token client.
//
// G41 (velion-gap.md §8.30 / Slice D): closes the "Graph enrichment worker"
// gap by giving user-core a thin client to exchange an opaque tokenRef
// for a short-lived OAuth access token. The handler that consumes
// `auth.user.provider_linked` events then uses that access token to fetch
// the user's full profile from the provider's identity API (Microsoft
// Graph for `provider=microsoft`, Google's userinfo endpoint for
// `provider=google`, etc.) and soft-update the local user row.
//
// This client wraps `POST ${AUTH_SERVICE_URL}/internal/oauth/token` (see
// auth-core's `internal/internal-oauth.controller.ts`). The HTTP handler
// path already implements the same exchange via `fetchAuthCoreTokenByRef`
// in `internal/http/handlers.go` — this file extracts that logic into a
// reusable client so the NATS event-handler path can share it.
package clients

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// AuthCoreOAuthClient calls Auth Core with User Core's scoped service identity.
// Goroutine-safe (`http.Client` is).
type AuthCoreOAuthClient struct {
	baseURL           string
	serviceCredential AuthInternalClientCredential
	httpClient        *http.Client
}

// AuthCoreTokenResult mirrors the JSON returned by `POST /internal/oauth/token`.
type AuthCoreTokenResult struct {
	Found             bool   `json:"found"`
	TokenRef          string `json:"tokenRef"`
	Provider          string `json:"provider"`
	ProviderAccountID string `json:"providerAccountId"`
	UserID            string `json:"userId"`
	AccessToken       string `json:"access_token"`
	RefreshToken      string `json:"refresh_token"`
	Scope             string `json:"scope"`
	ExpiresAt         string `json:"expires_at"`
	Error             string `json:"error,omitempty"`
}

// ErrTokenNotFound is returned when auth-core responds 200 with `found=false`.
var ErrTokenNotFound = errors.New("auth-core: token reference not found")

// NewAuthCoreOAuthClient returns nil when either `baseURL` or credential is
// empty so callers can keep the dependency optional without nil checks.
func NewAuthCoreOAuthClient(baseURL string, serviceCredential AuthInternalClientCredential) *AuthCoreOAuthClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" || serviceCredential.Token == "" {
		return nil
	}
	return &AuthCoreOAuthClient{
		baseURL:           baseURL,
		serviceCredential: serviceCredential,
		httpClient: &http.Client{
			// Auth-core's `POST /internal/oauth/token` is a single DB read +
			// JSON decode — 5s is generous. The NATS handler path swallows
			// errors as best-effort, so a slow auth-core just means the
			// profile enrichment skips this round; the provider-link row
			// still persists.
			Timeout: 5 * time.Second,
		},
	}
}

// RefreshTokenByRef calls auth-core's `POST /internal/oauth/refresh`. Used
// when a previously-issued access token has expired (Graph returns 401)
// and the caller wants to retry once with a freshly-exchanged token.
//
// Mirrors `GetTokenByRef` envelope: returns `*AuthCoreTokenResult` populated
// from auth-core's `{refreshed, tokenRef, provider, access_token, ...}`
// shape (the refresh route uses `refreshed` instead of `found` — we coerce
// to a consistent return type so callers don't branch on the wire format).
func (c *AuthCoreOAuthClient) RefreshTokenByRef(ctx context.Context, tokenRef string) (*AuthCoreTokenResult, error) {
	tokenRef = strings.TrimSpace(tokenRef)
	if tokenRef == "" {
		return nil, errors.New("tokenRef is required")
	}

	body, err := json.Marshal(map[string]string{"tokenRef": tokenRef})
	if err != nil {
		return nil, fmt.Errorf("marshal refresh request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/internal/oauth/refresh", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("build refresh request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	c.setServiceHeaders(req)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call auth-core refresh: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read refresh response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("auth-core refresh %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}

	// auth-core's `/internal/oauth/refresh` returns either
	//   { refreshed: true, tokenRef, provider, ..., access_token, refresh_token, expires_at }
	// or
	//   { refreshed: false, code, error }
	// — see the controller in `auth-core/src/internal/internal-oauth.controller.ts`.
	var raw struct {
		Refreshed         bool   `json:"refreshed"`
		TokenRef          string `json:"tokenRef"`
		Provider          string `json:"provider"`
		ProviderAccountID string `json:"providerAccountId"`
		UserID            string `json:"userId"`
		AccessToken       string `json:"access_token"`
		RefreshToken      string `json:"refresh_token"`
		Scope             string `json:"scope"`
		ExpiresAt         string `json:"expires_at"`
		Code              string `json:"code,omitempty"`
		Error             string `json:"error,omitempty"`
	}
	if err := json.Unmarshal(respBody, &raw); err != nil {
		return nil, fmt.Errorf("decode refresh response: %w", err)
	}

	if !raw.Refreshed {
		detail := raw.Error
		if detail == "" {
			detail = raw.Code
		}
		return nil, fmt.Errorf("auth-core refresh rejected: %s", detail)
	}
	if strings.TrimSpace(raw.AccessToken) == "" {
		return nil, errors.New("auth-core: refreshed response missing access_token")
	}

	return &AuthCoreTokenResult{
		Found:             true,
		TokenRef:          raw.TokenRef,
		Provider:          raw.Provider,
		ProviderAccountID: raw.ProviderAccountID,
		UserID:            raw.UserID,
		AccessToken:       raw.AccessToken,
		RefreshToken:      raw.RefreshToken,
		Scope:             raw.Scope,
		ExpiresAt:         raw.ExpiresAt,
	}, nil
}

// GetTokenByRef exchanges an opaque tokenRef for a short-lived access token.
// Returns `ErrTokenNotFound` when auth-core says found=false; any HTTP / decode
// failure is wrapped with %w.
func (c *AuthCoreOAuthClient) GetTokenByRef(ctx context.Context, tokenRef string) (*AuthCoreTokenResult, error) {
	tokenRef = strings.TrimSpace(tokenRef)
	if tokenRef == "" {
		return nil, errors.New("tokenRef is required")
	}

	body, err := json.Marshal(map[string]string{"tokenRef": tokenRef})
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/internal/oauth/token", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	c.setServiceHeaders(req)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call auth-core: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("auth-core %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}

	var result AuthCoreTokenResult
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}

	if !result.Found {
		return nil, ErrTokenNotFound
	}
	if strings.TrimSpace(result.AccessToken) == "" {
		return nil, errors.New("auth-core: access_token missing in response")
	}

	return &result, nil
}

func (c *AuthCoreOAuthClient) setServiceHeaders(req *http.Request) {
	req.Header.Set("X-Service-Credential-Id", c.serviceCredential.CredentialID)
	req.Header.Set("X-Service-Principal", c.serviceCredential.Principal)
	req.Header.Set("X-Service-Auth", c.serviceCredential.Token)
}
