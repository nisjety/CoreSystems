package org

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// IdentityProviderAdmin is the seam to auth-core's internal SSO administration
// (WS05-S7). auth-core owns the sso_provider rows, the sign-in evidence and the
// two-factor flag; org-core owns the policy and the domain proof, and asks
// auth-core for the rest. *AuthCoreIdentityClient satisfies it; tests use a fake.
type IdentityProviderAdmin interface {
	ListProviders(ctx context.Context, orgID string) ([]SSOProviderRecord, error)
	RegisterProvider(ctx context.Context, req SSOProviderRegistration) error
	DeleteProvider(ctx context.Context, orgID, providerID, actorID string) error
	HasRecentSignIn(ctx context.Context, orgID, userID string, within time.Duration) (bool, error)
	MemberFacts(ctx context.Context, orgID string, userIDs []string) ([]SSOMemberFact, error)
	RevokeSessions(ctx context.Context, orgID, actorID string, domains, exemptUserIDs []string) (int, error)
}

// SSOProviderRecord is one registered provider as auth-core reports it. It never
// carries the client secret.
type SSOProviderRecord struct {
	ProviderID string `json:"providerId"`
	Domain     string `json:"domain"`
	Issuer     string `json:"issuer"`
	Type       string `json:"type"`
}

// SSOProviderRegistration is the write-only request to register an OIDC provider.
type SSOProviderRegistration struct {
	OrganizationID string `json:"organizationId"`
	ActorUserID    string `json:"actorUserId"`
	ProviderID     string `json:"providerId"`
	Domain         string `json:"domain"`
	Issuer         string `json:"issuer"`
	ClientID       string `json:"clientId"`
	ClientSecret   string `json:"clientSecret"`
}

// SSOMemberFact is auth-core's view of one member: the role in its member table
// and whether the user has two-factor authentication on.
type SSOMemberFact struct {
	UserID           string `json:"userId"`
	Role             string `json:"role"`
	TwoFactorEnabled bool   `json:"twoFactorEnabled"`
}

var (
	// ErrSSOProviderExists is auth-core's 409 provider_exists.
	ErrSSOProviderExists = errors.New("sso provider already exists")
	// ErrSSOInvalidIssuer is auth-core's 400 invalid_issuer.
	ErrSSOInvalidIssuer = errors.New("sso issuer is not acceptable")
	// ErrSSOProviderNotFound is auth-core's 404 provider_not_found.
	ErrSSOProviderNotFound = errors.New("sso provider not found")
	// ErrSSOUnavailable means auth-core (or its credential) could not be used.
	ErrSSOUnavailable = errors.New("sso administration is unavailable")
)

const (
	authCoreIdentityTimeout = 5 * time.Second
	authCoreIdentityMaxBody = 1 << 20
	authCoreIdentityHeader  = "x-sso-admin-token"
)

// AuthCoreIdentityClient talks to auth-core's /api/v1/internal/sso surface with
// the dedicated ORG_CORE_IDENTITY_SERVICE_TOKEN credential (the pattern of the
// org-group grant surface); it is not the org-core service token.
type AuthCoreIdentityClient struct {
	baseURL string
	token   string
	http    *http.Client
}

// NewAuthCoreIdentityClient returns nil when either value is missing, so the
// SSO administration stays unavailable (503) instead of calling anonymously.
func NewAuthCoreIdentityClient(baseURL, token string) *AuthCoreIdentityClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	token = strings.TrimSpace(token)
	if baseURL == "" || token == "" {
		return nil
	}
	return &AuthCoreIdentityClient{baseURL: baseURL, token: token, http: &http.Client{Timeout: authCoreIdentityTimeout}}
}

func (c *AuthCoreIdentityClient) do(ctx context.Context, method, path string, query url.Values, body any, out any) error {
	return c.doAt(ctx, "/api/v1/internal/sso", method, path, query, body, out)
}

// doAt is do against another internal auth-core surface that shares the
// credential (the SCIM membership routes, WS05-S8).
func (c *AuthCoreIdentityClient) doAt(ctx context.Context, prefix, method, path string, query url.Values, body any, out any) error {
	target := c.baseURL + prefix + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("encode sso request: %w", err)
		}
		reader = bytes.NewReader(encoded)
	}
	req, err := http.NewRequestWithContext(ctx, method, target, reader)
	if err != nil {
		return fmt.Errorf("build sso request: %w", err)
	}
	req.Header.Set("accept", "application/json")
	req.Header.Set(authCoreIdentityHeader, c.token)
	if body != nil {
		req.Header.Set("content-type", "application/json")
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrSSOUnavailable, err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, authCoreIdentityMaxBody))
	if err != nil {
		return fmt.Errorf("%w: read response", ErrSSOUnavailable)
	}
	if resp.StatusCode >= 200 && resp.StatusCode < 300 {
		if out == nil || len(raw) == 0 {
			return nil
		}
		if err := json.Unmarshal(raw, out); err != nil {
			return fmt.Errorf("%w: malformed response", ErrSSOUnavailable)
		}
		return nil
	}
	var problem struct {
		Code string `json:"code"`
	}
	_ = json.Unmarshal(raw, &problem)
	switch {
	case resp.StatusCode == http.StatusConflict && problem.Code == "provider_exists":
		return ErrSSOProviderExists
	case resp.StatusCode == http.StatusBadRequest && problem.Code == "invalid_issuer":
		return ErrSSOInvalidIssuer
	case resp.StatusCode == http.StatusNotFound && problem.Code == "provider_not_found":
		return ErrSSOProviderNotFound
	case resp.StatusCode == http.StatusConflict && problem.Code == "last_owner":
		return ErrSCIMLastOwner
	case resp.StatusCode == http.StatusConflict && problem.Code == "membership_conflict":
		return ErrSCIMMembershipConflict
	}
	return fmt.Errorf("%w: auth-core answered %d", ErrSSOUnavailable, resp.StatusCode)
}

func (c *AuthCoreIdentityClient) ListProviders(ctx context.Context, orgID string) ([]SSOProviderRecord, error) {
	var out struct {
		Providers []SSOProviderRecord `json:"providers"`
	}
	if err := c.do(ctx, http.MethodGet, "/providers", url.Values{"organizationId": {orgID}}, nil, &out); err != nil {
		return nil, err
	}
	return out.Providers, nil
}

func (c *AuthCoreIdentityClient) RegisterProvider(ctx context.Context, req SSOProviderRegistration) error {
	return c.do(ctx, http.MethodPost, "/providers", nil, req, nil)
}

func (c *AuthCoreIdentityClient) DeleteProvider(ctx context.Context, orgID, providerID, actorID string) error {
	return c.do(ctx, http.MethodDelete, "/providers/"+url.PathEscape(providerID),
		url.Values{"organizationId": {orgID}, "actorUserId": {actorID}}, nil, nil)
}

func (c *AuthCoreIdentityClient) HasRecentSignIn(ctx context.Context, orgID, userID string, within time.Duration) (bool, error) {
	var out struct {
		Recent bool `json:"recent"`
	}
	query := url.Values{
		"organizationId": {orgID}, "userId": {userID},
		"withinSeconds": {fmt.Sprintf("%d", int(within.Seconds()))},
	}
	if err := c.do(ctx, http.MethodGet, "/evidence", query, nil, &out); err != nil {
		return false, err
	}
	return out.Recent, nil
}

func (c *AuthCoreIdentityClient) MemberFacts(ctx context.Context, orgID string, userIDs []string) ([]SSOMemberFact, error) {
	var out struct {
		Members []SSOMemberFact `json:"members"`
	}
	body := map[string]any{"organizationId": orgID, "userIds": userIDs}
	if err := c.do(ctx, http.MethodPost, "/member-facts", nil, body, &out); err != nil {
		return nil, err
	}
	return out.Members, nil
}

// RevokeSessions asks auth-core to end the sessions of every member whose email
// is on one of the domains, except the exempt users, in resumable batches.
func (c *AuthCoreIdentityClient) RevokeSessions(ctx context.Context, orgID, actorID string, domains, exemptUserIDs []string) (int, error) {
	total, cursor := 0, ""
	for batch := 0; batch < 200; batch++ {
		var out struct {
			Revoked    int    `json:"revoked"`
			NextCursor string `json:"nextCursor"`
		}
		body := map[string]any{
			"organizationId": orgID, "actorUserId": actorID, "domains": domains,
			"exemptUserIds": exemptUserIDs, "afterUserId": cursor, "limit": 100,
		}
		if err := c.do(ctx, http.MethodPost, "/revoke-sessions", nil, body, &out); err != nil {
			return total, err
		}
		total += out.Revoked
		if out.NextCursor == "" {
			return total, nil
		}
		cursor = out.NextCursor
	}
	return total, nil
}
