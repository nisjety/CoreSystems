package processwatch

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"
)

// The audience and scope capability-core presents to sandbox-manager.
//
// # Read only, and that is a decision
//
// The watch sweeper never writes to the process registry — it resolves a
// process and reads output, and nothing else. Requesting `sandbox:write` would
// let a bug in a poller move a process's state, so the credential simply cannot.
//
// # This needs a matching registry entry, and it is not in this repo's code
//
// Auth Core issues exactly the scopes requested, narrowed by the per-principal
// allowlist in `apps/Control Plane/config/plane-service-principals.json`. Until
// 2026-09-14 capability-core's entry listed only session-core, inference-core
// and orchestrator-core, so this request would have returned nothing usable and
// every poll would have been refused at sandbox-manager's interceptor.
//
// That is the exact failure S3.3 shipped and S4.2 found a slice later:
// `sandbox:write` requested, `sandbox:read` required, refused at the real
// interceptor while every unit test passed. No Go test can see that file, so
// the grant is part of this change rather than a deployment note.
const (
	sandboxManagerAudience = "sandbox-manager"
	sandboxReadScope       = "sandbox:read"
)

const (
	// maxTokenTTLSeconds mirrors execution-core's own bound: a credential Auth
	// Core says lasts longer than an hour is not one this service will cache.
	maxTokenTTLSeconds = 3600
	// tokenRefreshSkew re-mints slightly early, so a token never expires
	// between the cache check and the call that uses it.
	tokenRefreshSkew = 30 * time.Second
)

type cachedToken struct {
	value     string
	expiresAt time.Time
}

// AuthCoreTokens mints and caches capability-core's `aud=sandbox-manager`
// credential, per organization.
//
// Per-organization because the credential is org-scoped: one cache entry per
// tenant, exactly as execution-core's own provider does it. A single shared
// token would be a token for the wrong tenant on every call but the first.
type AuthCoreTokens struct {
	baseURL    string
	serviceID  string
	credential string
	http       *http.Client
	nowFn      func() time.Time

	mu    sync.Mutex
	cache map[string]cachedToken
}

// NewAuthCoreTokens builds a provider from deployment configuration.
func NewAuthCoreTokens(baseURL, serviceID, credential string, client *http.Client) (*AuthCoreTokens, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	serviceID = strings.TrimSpace(serviceID)
	credential = strings.TrimSpace(credential)
	if baseURL == "" || serviceID == "" || credential == "" {
		return nil, fmt.Errorf("a sandbox-manager token provider requires AUTH_CORE_URL, a service id and a credential")
	}
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return &AuthCoreTokens{
		baseURL:    baseURL,
		serviceID:  serviceID,
		credential: credential,
		http:       client,
		nowFn:      func() time.Time { return time.Now().UTC() },
		cache:      map[string]cachedToken{},
	}, nil
}

type tokenResponse struct {
	Token            string `json:"token"`
	ExpiresInSeconds int64  `json:"expiresInSeconds"`
	Audience         string `json:"audience"`
}

// Token returns a current credential for the organization, minting one if the
// cache has none.
func (t *AuthCoreTokens) Token(ctx context.Context, orgID string) (string, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", fmt.Errorf("a sandbox-manager credential needs a tenant")
	}
	now := t.nowFn()
	t.mu.Lock()
	for key, token := range t.cache {
		if !token.expiresAt.After(now.Add(tokenRefreshSkew)) {
			delete(t.cache, key)
		}
	}
	if token, ok := t.cache[orgID]; ok {
		t.mu.Unlock()
		return token.value, nil
	}
	t.mu.Unlock()

	body, err := json.Marshal(map[string]any{
		"orgId":  orgID,
		"scopes": []string{sandboxReadScope},
		"reason": "read a watched background process's output for a Space watch",
	})
	if err != nil {
		return "", fmt.Errorf("encode a sandbox-manager token request: %w", err)
	}
	url := fmt.Sprintf("%s/api/%s/internal-token", t.baseURL, sandboxManagerAudience)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build a sandbox-manager token request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-service-id", t.serviceID)
	request.Header.Set("x-service-api-key", t.credential)

	response, err := t.http.Do(request)
	if err != nil {
		return "", fmt.Errorf("request a sandbox-manager credential: %w", err)
	}
	defer func() { _ = response.Body.Close() }()
	if response.StatusCode/100 != 2 {
		// Worth naming the likely cause: a 403 here almost always means the
		// registry entry above is missing, not that anything in this process is
		// wrong.
		return "", fmt.Errorf(
			"Auth Core refused a sandbox-manager credential (status %d); check that capability-core's plane-service-principals entry grants the %q audience with %q",
			response.StatusCode, sandboxManagerAudience, sandboxReadScope)
	}
	var bundle tokenResponse
	if err := json.NewDecoder(response.Body).Decode(&bundle); err != nil {
		return "", fmt.Errorf("decode a sandbox-manager credential: %w", err)
	}
	if strings.TrimSpace(bundle.Token) == "" ||
		bundle.Audience != sandboxManagerAudience ||
		bundle.ExpiresInSeconds < 1 || bundle.ExpiresInSeconds > maxTokenTTLSeconds {
		// The audience check is not ceremony: a token minted for a different
		// service would be presented to sandbox-manager and refused there, and
		// the refusal would look like a sandbox-manager problem rather than a
		// token-minting one.
		return "", fmt.Errorf("Auth Core returned an invalid sandbox-manager credential")
	}

	t.mu.Lock()
	t.cache[orgID] = cachedToken{
		value:     bundle.Token,
		expiresAt: t.nowFn().Add(time.Duration(bundle.ExpiresInSeconds) * time.Second),
	}
	t.mu.Unlock()
	return bundle.Token, nil
}
