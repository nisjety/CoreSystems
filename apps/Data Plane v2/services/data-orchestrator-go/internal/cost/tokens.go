package cost

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

// The bearer the forwarder presents to cost-core.
//
// cost-core pins every write to the organization in the token, and the forwarder serves
// every organization, so there is one token per organization: asked of Auth Core, which
// confirms the organization exists in its directory before signing, for the one scope
// cost:write on the one audience cost-core. The registry row is `data-orchestrator`,
// `tenancy: directory`, retention `persistent` (apps/Control Plane/config/README.md, "The
// Data Plane principal bundle").
//
// The token's claims are checked here against what was asked for, so a mint that answered
// with another organization's token, or with a broader one, is never used. Auth Core stays
// the issuer and cost-core the cryptographic verifier: this is the bounded-response profile
// a client can check without the key.

const (
	costAudience = "cost-core"
	costScope    = "cost:write"
	// mintReason is recorded in Auth Core's audit trail of every issued token.
	mintReason = "usage forward"

	maxTokenLifetime     = 5 * time.Minute
	maxTokenResponseSize = 64 * 1024
	maxTokenBytes        = 8192
	maxMintErrorBody     = 4096
	tokenClockLeeway     = 30 * time.Second
	minServiceKeyBytes   = 32
)

// errTokenUnavailable is every failure to obtain a usable bearer for an organization. The
// forwarder records it as the class token_unavailable and tries again later; it carries no
// detail of Auth Core's answer, which can name the organization.
var errTokenUnavailable = errors.New("cost-core bearer unavailable")

// TokenSource hands out the bearer for one organization.
type TokenSource interface {
	Token(ctx context.Context, orgID string) (string, error)
	// Invalidate forgets the organization's cached bearer (cost-core refused it).
	Invalidate(orgID string)
}

// AuthCoreTokens mints and caches the per-organization bearer.
type AuthCoreTokens struct {
	url       string
	serviceID string
	apiKey    string
	issuer    string
	client    *http.Client
	now       func() time.Time

	mu    sync.Mutex
	cache map[string]cachedToken
}

type cachedToken struct {
	token     string
	reuseTill time.Time
}

// refuseRedirect makes a client treat a redirect as the answer it is. The mint carries the
// principal's credential in a custom header, and Go forwards custom headers to whatever host a
// redirect names (it strips only Authorization and cookies across domains), so following one
// would hand the credential to another address. The forwarder's client refuses too: a redirect
// that ends at a 2xx page (a login page, a proxy's default answer) would otherwise read as
// cost-core accepting a row it never saw, and the spend would be marked forwarded and lost.
func refuseRedirect(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }

// NewAuthCoreTokens builds the client. The API key is the deployment-owned credential of the
// principal (DATA_ORCHESTRATOR_COST_SERVICE_API_KEY); it is sent in a header to Auth Core and
// to nobody else, and is never logged.
func NewAuthCoreTokens(url, serviceID, apiKey, issuer string, client *http.Client) (*AuthCoreTokens, error) {
	url = strings.TrimSpace(url)
	serviceID = strings.TrimSpace(serviceID)
	if url == "" || serviceID == "" || len(strings.TrimSpace(apiKey)) < minServiceKeyBytes {
		return nil, errors.New("cost-core token client needs a token URL, a service id and a service key of at least 32 characters")
	}
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second, CheckRedirect: refuseRedirect}
	}
	return &AuthCoreTokens{
		url:       url,
		serviceID: serviceID,
		apiKey:    strings.TrimSpace(apiKey),
		issuer:    strings.TrimSpace(issuer),
		client:    client,
		now:       time.Now,
		cache:     make(map[string]cachedToken),
	}, nil
}

// Token returns a bearer for the organization, from the cache while it is fresh.
//
// Fresh is until a quarter of its lifetime (at most a minute) before it expires: a bearer
// that is about to expire is never handed to a request that may take seconds. A failed mint
// is not cached: the next call asks again, and the forwarder's own backoff is what spaces
// them.
func (a *AuthCoreTokens) Token(ctx context.Context, orgID string) (string, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", errTokenUnavailable
	}
	now := a.now()
	a.mu.Lock()
	if cached, ok := a.cache[orgID]; ok && now.Before(cached.reuseTill) {
		a.mu.Unlock()
		return cached.token, nil
	}
	a.mu.Unlock()

	token, reuseTill, err := a.mint(ctx, orgID)
	if err != nil {
		return "", err
	}
	a.mu.Lock()
	// Entries that can no longer be reused are dropped when one is added, so the map holds
	// only the organizations active within a token's lifetime.
	for org, cached := range a.cache {
		if !now.Before(cached.reuseTill) {
			delete(a.cache, org)
		}
	}
	a.cache[orgID] = cachedToken{token: token, reuseTill: reuseTill}
	a.mu.Unlock()
	return token, nil
}

func (a *AuthCoreTokens) Invalidate(orgID string) {
	a.mu.Lock()
	delete(a.cache, strings.TrimSpace(orgID))
	a.mu.Unlock()
}

type mintResponse struct {
	Token string `json:"token"`
}

func (a *AuthCoreTokens) mint(ctx context.Context, orgID string) (string, time.Time, error) {
	body, err := json.Marshal(map[string]any{
		"orgId":  orgID,
		"scopes": []string{costScope},
		"reason": mintReason,
	})
	if err != nil {
		return "", time.Time{}, errTokenUnavailable
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.url, bytes.NewReader(body))
	if err != nil {
		return "", time.Time{}, errTokenUnavailable
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json")
	request.Header.Set("x-service-id", a.serviceID)
	request.Header.Set("x-service-api-key", a.apiKey)
	response, err := a.client.Do(request)
	if err != nil {
		return "", time.Time{}, errTokenUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusCreated {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxMintErrorBody))
		return "", time.Time{}, errTokenUnavailable
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, maxTokenResponseSize+1))
	if err != nil || len(raw) > maxTokenResponseSize {
		return "", time.Time{}, errTokenUnavailable
	}
	var minted mintResponse
	if err := json.Unmarshal(raw, &minted); err != nil || minted.Token == "" || len(minted.Token) > maxTokenBytes {
		return "", time.Time{}, errTokenUnavailable
	}
	issuedAt, expiresAt, err := a.checkClaims(minted.Token, orgID)
	if err != nil {
		return "", time.Time{}, errTokenUnavailable
	}
	lifetime := expiresAt.Sub(issuedAt)
	margin := min(lifetime/4, time.Minute)
	return minted.Token, expiresAt.Add(-margin), nil
}

type bearerClaims struct {
	Issuer        string          `json:"iss"`
	Audience      json.RawMessage `json:"aud"`
	Subject       string          `json:"sub"`
	PrincipalType string          `json:"principal_type"`
	OrgID         string          `json:"org_id"`
	Scopes        []string        `json:"scopes"`
	IssuedAt      int64           `json:"iat"`
	ExpiresAt     int64           `json:"exp"`
}

// checkClaims reads the bearer's payload and checks it is the token that was asked for. It
// does not verify the signature: it cannot (it has no key, by design) and cost-core does.
func (a *AuthCoreTokens) checkClaims(token, orgID string) (time.Time, time.Time, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return time.Time{}, time.Time{}, errTokenUnavailable
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return time.Time{}, time.Time{}, errTokenUnavailable
	}
	var claims bearerClaims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return time.Time{}, time.Time{}, errTokenUnavailable
	}
	var audiences []string
	if err := json.Unmarshal(claims.Audience, &audiences); err != nil {
		var single string
		if err := json.Unmarshal(claims.Audience, &single); err != nil {
			return time.Time{}, time.Time{}, errTokenUnavailable
		}
		audiences = []string{single}
	}
	hasScope := false
	for _, scope := range claims.Scopes {
		hasScope = hasScope || scope == costScope
	}
	issuedAt, expiresAt := time.Unix(claims.IssuedAt, 0), time.Unix(claims.ExpiresAt, 0)
	now := a.now()
	if len(audiences) != 1 || audiences[0] != costAudience ||
		claims.PrincipalType != "service" || claims.Subject != "service:"+a.serviceID ||
		claims.OrgID != orgID || !hasScope || len(claims.Scopes) != 1 ||
		(a.issuer != "" && claims.Issuer != a.issuer) ||
		claims.IssuedAt <= 0 || claims.ExpiresAt <= claims.IssuedAt ||
		expiresAt.Sub(issuedAt) > maxTokenLifetime ||
		issuedAt.After(now.Add(tokenClockLeeway)) || !expiresAt.After(now) {
		return time.Time{}, time.Time{}, fmt.Errorf("%w: claims", errTokenUnavailable)
	}
	return issuedAt, expiresAt, nil
}
