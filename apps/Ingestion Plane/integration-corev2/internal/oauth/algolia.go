package oauth

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

var ErrResourceBoundToken = errors.New("resource-bound Algolia tokens cannot be leased")
var errAlgoliaOAuth = errors.New("Algolia authorization is unavailable; reconnect this read-only account")

type AlgoliaOAuthClient struct {
	cfg  config.Config
	http *http.Client
}

func NewAlgoliaOAuthClient(cfg config.Config, client *http.Client) *AlgoliaOAuthClient {
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{Guard: egress.New(), RequestTimeout: 15 * time.Second})
	}
	copy := *client
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	copy.Jar = nil
	copy.Timeout = 15 * time.Second
	return &AlgoliaOAuthClient{cfg: cfg, http: &copy}
}
func (c *AlgoliaOAuthClient) AuthorizationURL(state, redirect, verifier string, scopes []string, _ map[string]string) (string, error) {
	r, err := c.cfg.AlgoliaRegistration()
	if err != nil || redirect != r.RedirectURI || len(scopes) != 1 || scopes[0] != "public" {
		return "", errAlgoliaOAuth
	}
	q := url.Values{"client_id": {r.ClientID}, "redirect_uri": {r.RedirectURI}, "response_type": {"code"}, "state": {state}, "scope": {"public"}, "code_challenge": {CodeChallengeS256(verifier)}, "code_challenge_method": {"S256"}, "resource": {r.Resource}}
	return "https://dashboard.algolia.com/2/oauth/authorize?" + q.Encode(), nil
}
func (c *AlgoliaOAuthClient) ExchangeCode(ctx context.Context, code, redirect, verifier string, _ []string, _ map[string]string) (TokenResult, error) {
	r, err := c.cfg.AlgoliaRegistration()
	if err != nil || redirect != r.RedirectURI {
		return TokenResult{}, errAlgoliaOAuth
	}
	return c.token(ctx, r, url.Values{"grant_type": {"authorization_code"}, "code": {code}, "redirect_uri": {redirect}, "code_verifier": {verifier}})
}
func (c *AlgoliaOAuthClient) Refresh(ctx context.Context, token string, _ []string, _ map[string]string) (TokenResult, error) {
	r, err := c.cfg.AlgoliaRegistration()
	if err != nil {
		return TokenResult{}, errAlgoliaOAuth
	}
	return c.token(ctx, r, url.Values{"grant_type": {"refresh_token"}, "refresh_token": {token}})
}
func (c *AlgoliaOAuthClient) token(ctx context.Context, r config.AlgoliaClientRegistration, q url.Values) (TokenResult, error) {
	q.Set("client_id", r.ClientID)
	q.Set("resource", r.Resource)
	req, err := http.NewRequestWithContext(ctx, "POST", "https://dashboard.algolia.com/2/oauth/token", strings.NewReader(q.Encode()))
	if err != nil {
		return TokenResult{}, errAlgoliaOAuth
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	res, err := c.http.Do(req)
	if err != nil {
		return TokenResult{}, errAlgoliaOAuth
	}
	defer res.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(res.Body, 65537))
	if err != nil || len(raw) > 65536 || webvitals.UniqueJSON(raw) != nil {
		return TokenResult{}, errAlgoliaOAuth
	}
	if res.StatusCode != 200 {
		var failure struct {
			Error string `json:"error"`
		}
		if (res.StatusCode == 400 || res.StatusCode == 401) && json.Unmarshal(raw, &failure) == nil && failure.Error == "invalid_grant" {
			return TokenResult{}, &TokenEndpointError{ProviderKey: "algolia", StatusCode: res.StatusCode, Code: "invalid_grant"}
		}
		return TokenResult{}, errAlgoliaOAuth
	}
	var v struct {
		Access  string `json:"access_token"`
		Refresh string `json:"refresh_token"`
		Type    string `json:"token_type"`
		Expires int64  `json:"expires_in"`
		Scope   string `json:"scope"`
	}
	if json.Unmarshal(raw, &v) != nil || len(v.Access) < 1 || len(v.Access) > 16384 || strings.ContainsAny(v.Access, "\r\n\x00") || len(v.Refresh) > 16384 || strings.ContainsAny(v.Refresh, "\r\n\x00") || !strings.EqualFold(v.Type, "Bearer") || v.Expires < 1 || v.Expires > 86400 || (v.Scope != "" && v.Scope != "public") {
		return TokenResult{}, errAlgoliaOAuth
	}
	return TokenResult{AccessToken: v.Access, RefreshToken: v.Refresh, TokenType: "Bearer", Scope: []string{"public"}, ScopesVerified: true, ExpiresAt: time.Now().UTC().Add(time.Duration(v.Expires) * time.Second)}, nil
}
func (c *AlgoliaOAuthClient) Profile(context.Context, string, map[string]string) (ProviderProfile, error) {
	return ProviderProfile{}, nil
}
func (c *AlgoliaOAuthClient) Revoke(context.Context, string, map[string]string) error {
	return ErrRevocationUnsupported
}

// AlgoliaAccessToken is consumed only by Integration's fixed MCP adapter.
func (s *Service) AlgoliaAccessToken(ctx context.Context, org, id string) (AccessTokenResult, error) {
	c, err := s.repo.GetConnection(ctx, id)
	if err != nil || c.OrganizationID != org || c.ProviderKey != "algolia" || c.Status != "active" || c.DeletedAt != nil {
		return AccessTokenResult{}, store.ErrNotFound
	}
	r, err := s.cfg.AlgoliaRegistration()
	if err != nil || c.ProviderContext["algolia_resource"] != r.Resource || c.ProviderContext["algolia_client_id"] != r.ClientID {
		return AccessTokenResult{}, errAlgoliaOAuth
	}
	return s.accessTokenForConnection(ctx, c)
}

// AlgoliaGrantMatchesRegistration is a local config check, not provider I/O.
func (s *Service) AlgoliaGrantMatchesRegistration(c store.Connection) bool {
	r, err := s.cfg.AlgoliaRegistration()
	return err == nil && c.ProviderContext["algolia_resource"] == r.Resource && c.ProviderContext["algolia_client_id"] == r.ClientID
}

// InvalidateAlgoliaAccessToken reacts only to a provider-observed 401. Sharing
// refresh coordination and the DB advisory lock prevents a stale rejected
// bearer from overwriting a refreshed credential or another grant.
func (s *Service) InvalidateAlgoliaAccessToken(ctx context.Context, org, id, rejected string) error {
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	work := func(ctx context.Context) (store.Connection, string, error) {
		c, err := s.repo.GetConnection(ctx, id)
		if err != nil || c.OrganizationID != org || c.ProviderKey != "algolia" || c.Status != "active" || c.DeletedAt != nil {
			return store.Connection{}, "", store.ErrNotFound
		}
		current, err := s.vault.Decrypt(c.EncryptedAccessToken, []byte(c.ID))
		if err != nil {
			return store.Connection{}, "", err
		}
		if subtle.ConstantTimeCompare([]byte(current), []byte(rejected)) != 1 {
			return c, current, nil
		}
		updated := c
		updated.Status = "needs_refresh"
		updated.UpdatedAt = s.now()
		saved, err := s.repo.UpdateConnectionCredentials(ctx, updated)
		if err != nil {
			return store.Connection{}, "", err
		}
		return saved, "", nil
	}
	for {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		executed := false
		_, _, err := s.refreshSingleflight(ctx, id, func(ctx context.Context) (store.Connection, string, error) {
			executed = true
			if locker, ok := s.repo.(store.ConnectionRefreshLocker); ok {
				var saved store.Connection
				var token string
				err := locker.WithConnectionRefreshLock(ctx, id, func(lockCtx context.Context) error { var e error; saved, token, e = work(lockCtx); return e })
				return saved, token, err
			}
			return work(ctx)
		})
		if !executed {
			continue
		}
		if errors.Is(err, store.ErrNotFound) {
			return nil
		}
		return err
	}
}
