// Package servicetoken mints short-lived, audience-bound Auth Core service
// JWTs for a Model Plane service's own outbound calls.
//
// It is used by any service that must call a sibling on its OWN behalf rather
// than while proxying a user request — capability-core's learning consumer and
// orchestrator-core's Temporal activities are both of that shape. A Temporal
// activity is the clearest case: its context comes from the worker, not from a
// gRPC server handler, so there is no inbound credential to forward and the
// only credential it can present is one it minted itself.
//
// # Why this exists
//
// Every backend authenticates every RPC, and the only credential a service can
// present for itself is an Auth Core service JWT. Reading one from the
// environment cannot keep working: auth-core issues plane tokens with a
// 300-second default TTL (`PLANE_TOKEN_TTL_SESSION_CORE_SECONDS` /
// `PLANE_TOKEN_TTL_INFERENCE_CORE_SECONDS` in auth-core's
// `convex-token.service.ts`, floor 60s). A pasted token therefore starts
// answering Unauthenticated roughly five minutes after the operator pasted it
// and never recovers — the work keeps being dispatched and silently never runs.
// Minting on demand and refreshing ahead of expiry is the only arrangement that
// stays live.
//
// # What this CANNOT do
//
// The minted token's subject is the SERVICE principal: auth-core's
// `issueInternalToken` sets `userId: principal.subject` and its request body
// accepts only `orgId`, `scopes` and `reason` — there is no user-delegation
// field. A service token therefore proves "this service, in this org", never
// "this service acting as user X". Backends that require the caller's identity
// to equal a specific end user (execution-core's `authorize`, which compares
// `req.user_id` to the caller's own, and its run-ownership resolution) cannot
// be satisfied by anything this package mints, and that is deliberate: a
// background workflow must not silently wield a user's privileges.
//
// # Mint contract
//
// Verified against auth-core's `src/auth/plane-token.controller.ts`
// (`issueInternalToken`) and `src/auth/convex-token.service.ts`
// (`issuePlaneToken`):
//
//	POST {AuthCoreURL}/api/{audience}/internal-token
//	  Content-Type:      application/json
//	  x-service-id:      <service principal id>
//	  x-service-api-key: <service principal credential>
//
//	{"orgId": "...", "scopes": ["..."], "reason": "..."}
//
//	200 → {"token","expiresAt","expiresInSeconds","issuer","audience"}
//
// The `{audience}` path segment is the plane-audience slug — `session-core`,
// `inference-core`. auth-core's `exactAudience()` throws unless the configured
// audience string equals that slug, so the response `audience` is always
// exactly the slug and is safe to assert.
//
// Request fields can only NARROW the deployment allowlist, never widen it: the
// service principal's registry entry owns the ceiling on audiences, orgs and
// scopes. Asking for a scope outside it is refused outright (403) rather than
// silently narrowed, so a wrong scope list surfaces as an error instead of a
// quietly under-privileged token.
//
// Neither an `x-zdr` header nor a `zdr` body field may be sent — the controller
// answers 400 for either, because retention posture is chosen by deployment
// policy (the principal's `retentionByAudience`), not by the caller.
//
// # Caching
//
// Tokens are cached per (org, scope set). Keying by org alone would hand a
// read-scoped token to a caller that asked for write scopes; keying by scope
// set alone would hand one org's token to another org, which session-core
// rejects outright — its `authorize_org` requires the request's org_id to equal
// the token's own org_id exactly.
package servicetoken

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	// RefreshSkew is how far ahead of the issued expiry a cached token is
	// re-minted. Refreshing on a margin — rather than on the first
	// Unauthenticated — is the point: a token that expires mid-flight produces a
	// failed unit of work, and for the learning consumer a failed review is a
	// dropped RUN_COMPLETED event nobody notices.
	RefreshSkew = 60 * time.Second

	// maxTTL caps how long one minted credential is held regardless of what the
	// issuer says. A service credential living longer than an hour is a posture
	// regression, so an over-long TTL is clamped (we simply re-mint hourly)
	// rather than trusted or rejected — clamping keeps a misconfigured
	// PLANE_TOKEN_TTL_* from either widening exposure or causing an outage.
	maxTTL = time.Hour

	// fallbackTTL applies only if a mint response omits expiresInSeconds.
	// auth-core always sets it today (it is a required field of
	// PlaneTokenBundle), so this is purely defensive against a future contract
	// change. It is deliberately short — two refresh margins, leaving exactly
	// one usable margin — so an unknown expiry can never mean a stale token,
	// only a slightly chattier mint cadence.
	fallbackTTL = 2 * RefreshSkew

	// mintTimeout bounds one token exchange. Auth Core is a same-datacentre
	// dependency; a slow mint should fail the RPC, not hang the consumer.
	mintTimeout = 5 * time.Second

	// maxResponseBytes bounds what is read from a mint response. Tokens are a
	// few kilobytes; anything larger is a misrouted response, not a credential.
	maxResponseBytes = 1 << 16
)

// Config describes one audience's minting identity and bounds.
type Config struct {
	// AuthCoreURL is the Auth Core base URL, e.g. http://auth-core:3011.
	AuthCoreURL string
	// ServiceID is this deployment's service-principal id (the `x-service-id`
	// header, and the registry key on the auth-core side).
	ServiceID string
	// Credential is the service-principal secret (the `x-service-api-key`
	// header). Never logged.
	Credential string
	// Audience is the plane-audience slug to mint for, e.g. "session-core".
	Audience string
	// Scopes are the scopes this audience actually needs. One provider per
	// audience carries exactly that audience's scopes — a union across audiences
	// would ask session-core's ceiling to cover inference-core's scopes and vice
	// versa, and would hand each backend more authority than it needs.
	Scopes []string
	// Reason is audited by Auth Core, so it must describe the real purpose
	// rather than be copied from another caller.
	Reason string

	// HTTPClient overrides the default client (tests).
	HTTPClient *http.Client
	// Now overrides the clock (tests).
	Now func() time.Time
}

// Provider mints and caches tokens for exactly one audience.
type Provider struct {
	authCoreURL string
	serviceID   string
	credential  string
	audience    string
	scopes      []string
	reason      string
	http        *http.Client
	now         func() time.Time

	mu    sync.Mutex
	cache map[string]cachedToken
}

type cachedToken struct {
	value string
	// refreshAt is when this entry stops being served — the issued expiry minus
	// the refresh margin, NOT the expiry itself.
	refreshAt time.Time
}

// New validates cfg and builds a Provider.
//
// Every failure names the specific missing piece: this is the error a caller
// turns into the startup WARN that tells an operator which variable to set, so
// "not configured" must never be the whole message.
func New(cfg Config) (*Provider, error) {
	authCoreURL := strings.TrimRight(strings.TrimSpace(cfg.AuthCoreURL), "/")
	if authCoreURL == "" {
		return nil, errors.New("servicetoken: auth-core URL is required")
	}
	if strings.TrimSpace(cfg.ServiceID) == "" {
		return nil, errors.New("servicetoken: service id is required")
	}
	if strings.TrimSpace(cfg.Credential) == "" {
		return nil, errors.New("servicetoken: service principal credential is required")
	}
	if strings.TrimSpace(cfg.Audience) == "" {
		return nil, errors.New("servicetoken: audience is required")
	}
	scopes := normalizedScopes(cfg.Scopes)
	if len(scopes) == 0 {
		return nil, errors.New("servicetoken: at least one scope is required")
	}
	if strings.TrimSpace(cfg.Reason) == "" {
		return nil, errors.New("servicetoken: reason is required (Auth Core audits it)")
	}

	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: mintTimeout}
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}

	return &Provider{
		authCoreURL: authCoreURL,
		serviceID:   strings.TrimSpace(cfg.ServiceID),
		credential:  strings.TrimSpace(cfg.Credential),
		audience:    strings.TrimSpace(cfg.Audience),
		scopes:      scopes,
		reason:      strings.TrimSpace(cfg.Reason),
		http:        httpClient,
		now:         now,
		cache:       map[string]cachedToken{},
	}, nil
}

// Audience reports the plane audience this provider mints for.
func (p *Provider) Audience() string { return p.audience }

// Scopes reports a copy of the scopes this provider requests.
func (p *Provider) Scopes() []string { return append([]string(nil), p.scopes...) }

// Token returns a live token for orgID, minting one when none is cached or the
// cached one is inside the refresh margin of its expiry.
//
// orgID is required and is not defaulted: the mint request carries it, and
// session-core enforces an exact match against it, so a guessed org produces a
// token that authenticates but is refused on every call. An empty org is a
// caller bug worth surfacing.
func (p *Provider) Token(ctx context.Context, orgID string) (string, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", errors.New("servicetoken: org is required to mint a token")
	}
	key := cacheKey(orgID, p.scopes)
	if token, ok := p.cachedToken(key); ok {
		return token, nil
	}
	return p.mint(ctx, key, orgID)
}

// Invalidate drops the cached token for orgID so the next Token call mints a
// fresh one.
//
// This is the 401 backstop, not the primary refresh path. It exists so a
// credential rotated underneath a live process, a re-keyed issuer, or clock
// skew that made a token look live while the backend considered it dead
// self-heals on one retry instead of wedging until restart.
func (p *Provider) Invalidate(orgID string) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return
	}
	key := cacheKey(orgID, p.scopes)
	p.mu.Lock()
	defer p.mu.Unlock()
	delete(p.cache, key)
}

func (p *Provider) cachedToken(key string) (string, bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	entry, ok := p.cache[key]
	if !ok || !p.now().Before(entry.refreshAt) {
		return "", false
	}
	return entry.value, true
}

type mintRequest struct {
	OrgID  string   `json:"orgId"`
	Scopes []string `json:"scopes"`
	Reason string   `json:"reason"`
}

type mintResponse struct {
	Token            string `json:"token"`
	ExpiresAt        string `json:"expiresAt"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
	Issuer           string `json:"issuer"`
	Audience         string `json:"audience"`
}

// mint exchanges the service-principal credential for one audience-bound token.
//
// The cache lock is NOT held across the HTTP call: holding it would serialize
// every tenant behind one network round trip. Two goroutines racing for the same
// key may therefore both mint; both tokens are valid and last-write-wins is
// harmless, which is the right trade against blocking the whole consumer.
func (p *Provider) mint(ctx context.Context, key, orgID string) (string, error) {
	body, err := json.Marshal(mintRequest{
		OrgID:  orgID,
		Scopes: p.scopes,
		Reason: p.reason,
	})
	if err != nil {
		return "", fmt.Errorf("servicetoken: encode %s token request: %w", p.audience, err)
	}

	endpoint := p.authCoreURL + "/api/" + p.audience + "/internal-token"
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("servicetoken: build %s token request: %w", p.audience, err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-service-id", p.serviceID)
	request.Header.Set("x-service-api-key", p.credential)
	// Deliberately no x-zdr header and no zdr body field: auth-core answers 400
	// to either, because the retention posture is the deployment's to choose.

	response, err := p.http.Do(request)
	if err != nil {
		return "", fmt.Errorf("servicetoken: %s token request failed: %w", p.audience, err)
	}
	defer func() { _ = response.Body.Close() }()

	raw, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes))
	if err != nil {
		return "", fmt.Errorf("servicetoken: read %s token response: %w", p.audience, err)
	}
	if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusCreated {
		// A non-2xx response never carries a token, so echoing a bounded slice of
		// it is safe and is usually the difference between diagnosing "principal
		// not authorized" and "unknown plane audience" in one pass.
		return "", fmt.Errorf(
			"servicetoken: auth-core refused a %s token for org %s with scopes [%s]: status %d: %s",
			p.audience, orgID, strings.Join(p.scopes, " "), response.StatusCode, summarize(raw))
	}

	var parsed mintResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return "", fmt.Errorf("servicetoken: decode %s token response: %w", p.audience, err)
	}
	token := strings.TrimSpace(parsed.Token)
	if token == "" {
		return "", fmt.Errorf("servicetoken: auth-core returned no token for audience %s", p.audience)
	}
	// auth-core pins the response audience to the path slug, so a mismatch means
	// the request was routed somewhere unexpected. Presenting such a token would
	// fail the audience check at the backend anyway; fail here, where the reason
	// is legible.
	if got := strings.TrimSpace(parsed.Audience); got != "" && got != p.audience {
		return "", fmt.Errorf(
			"servicetoken: auth-core issued an %q token but %q was requested", got, p.audience)
	}

	ttl := time.Duration(parsed.ExpiresInSeconds) * time.Second
	if ttl <= 0 {
		ttl = fallbackTTL
	}
	if ttl > maxTTL {
		ttl = maxTTL
	}

	p.mu.Lock()
	p.cache[key] = cachedToken{value: token, refreshAt: refreshAt(p.now(), ttl)}
	p.mu.Unlock()

	return token, nil
}

// refreshAt returns the instant a freshly minted token stops being served: the
// issued expiry minus RefreshSkew.
//
// The margin shrinks to half the lifetime for tokens shorter than two margins.
// Without that, a deployment configured near auth-core's 60-second TTL floor
// would compute a refresh instant already in the past and re-mint on literally
// every RPC.
func refreshAt(now time.Time, ttl time.Duration) time.Time {
	margin := RefreshSkew
	if ttl <= 2*RefreshSkew {
		margin = ttl / 2
	}
	return now.Add(ttl - margin)
}

// cacheKey binds an entry to both the tenant and the exact scope set it was
// minted with. NUL separates the parts so no org id or scope can forge a
// collision with a different pair.
func cacheKey(orgID string, scopes []string) string {
	return orgID + "\x00" + strings.Join(scopes, "\x00")
}

// normalizedScopes trims, drops blanks, and de-duplicates while preserving
// order, so the cache key for one logical scope set is stable.
func normalizedScopes(scopes []string) []string {
	out := make([]string, 0, len(scopes))
	seen := make(map[string]struct{}, len(scopes))
	for _, scope := range scopes {
		trimmed := strings.TrimSpace(scope)
		if trimmed == "" {
			continue
		}
		if _, duplicate := seen[trimmed]; duplicate {
			continue
		}
		seen[trimmed] = struct{}{}
		out = append(out, trimmed)
	}
	return out
}

// summarize bounds an error body so a misrouted HTML page cannot flood the log.
func summarize(raw []byte) string {
	const limit = 200
	text := strings.TrimSpace(string(raw))
	if len(text) > limit {
		return text[:limit] + "..."
	}
	return text
}
