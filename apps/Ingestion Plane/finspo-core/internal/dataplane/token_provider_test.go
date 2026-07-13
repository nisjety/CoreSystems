package dataplane

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

type fakeOrgTokenProvider struct {
	mu          sync.Mutex
	tokens      []string
	orgs        []string
	invalidated []string
	configured  bool
}

func (p *fakeOrgTokenProvider) Configured() bool { return p != nil && p.configured }

func (p *fakeOrgTokenProvider) Token(_ context.Context, orgID string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.orgs = append(p.orgs, orgID)
	if len(p.tokens) == 0 {
		return "", fmt.Errorf("no token")
	}
	token := p.tokens[0]
	if len(p.tokens) > 1 {
		p.tokens = p.tokens[1:]
	}
	return token, nil
}

func (p *fakeOrgTokenProvider) Invalidate(orgID, token string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.invalidated = append(p.invalidated, orgID+":"+token)
}

func TestAuthCoreTokenProviderMintsPerOrgAndCachesUntilNearExpiry(t *testing.T) {
	now := time.Date(2026, time.July, 12, 12, 0, 0, 0, time.UTC)
	var mu sync.Mutex
	mints := map[string]int{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/data-plane/internal-token" {
			t.Fatalf("path = %q", r.URL.Path)
		}
		if r.Header.Get("X-Service-ID") != "finspo-core" {
			t.Fatalf("X-Service-ID = %q", r.Header.Get("X-Service-ID"))
		}
		if r.Header.Get("X-Service-API-Key") != "finspo-key" {
			t.Fatal("X-Service-API-Key missing")
		}
		var body struct {
			OrgID  string   `json:"orgId"`
			Scopes []string `json:"scopes"`
			Reason string   `json:"reason"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode mint request: %v", err)
		}
		if len(body.Scopes) != 1 || body.Scopes[0] != "documents:write" || body.Reason == "" {
			t.Fatalf("mint body = %#v", body)
		}
		mu.Lock()
		mints[body.OrgID]++
		mintNumber := mints[body.OrgID]
		mu.Unlock()
		writeFinspoTokenBundle(t, w, fmt.Sprintf("token-%s-%d", body.OrgID, mintNumber), now.Add(5*time.Minute), 300)
	}))
	defer server.Close()

	provider := NewAuthCoreTokenProvider(AuthCoreTokenConfig{
		AuthCoreURL: server.URL, ServiceID: "finspo-core", ServiceAPIKey: "finspo-key",
		HTTPClient: server.Client(), Now: func() time.Time { return now },
	})
	first, err := provider.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	second, err := provider.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	other, err := provider.Token(context.Background(), "org-2")
	if err != nil {
		t.Fatal(err)
	}
	if first != second || first == other || mints["org-1"] != 1 || mints["org-2"] != 1 {
		t.Fatalf("tokens=%q/%q/%q mints=%#v", first, second, other, mints)
	}

	now = now.Add(4*time.Minute + 31*time.Second)
	refreshed, err := provider.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	if refreshed == first || mints["org-1"] != 2 {
		t.Fatalf("refreshed=%q mints=%#v", refreshed, mints)
	}
}

func TestAuthCoreTokenProviderSingleflightsConcurrentMintPerOrg(t *testing.T) {
	var mints int
	var mu sync.Mutex
	var startedOnce sync.Once
	started := make(chan struct{})
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		mu.Lock()
		mints++
		mu.Unlock()
		startedOnce.Do(func() { close(started) })
		<-release
		writeFinspoTokenBundle(t, w, "shared-token", time.Now().Add(5*time.Minute), 300)
	}))
	defer server.Close()

	provider := NewAuthCoreTokenProvider(AuthCoreTokenConfig{
		AuthCoreURL: server.URL, ServiceID: "finspo-core", ServiceAPIKey: "finspo-key", HTTPClient: server.Client(),
	})
	const callers = 32
	start := make(chan struct{})
	errs := make(chan error, callers)
	for range callers {
		go func() {
			<-start
			token, err := provider.Token(context.Background(), "org-1")
			if err == nil && token != "shared-token" {
				err = fmt.Errorf("token = %q", token)
			}
			errs <- err
		}()
	}
	close(start)
	<-started
	close(release)
	for range callers {
		if err := <-errs; err != nil {
			t.Fatal(err)
		}
	}
	mu.Lock()
	defer mu.Unlock()
	if mints != 1 {
		t.Fatalf("mints = %d, want one per-key in-flight mint", mints)
	}
}

func TestAuthCoreTokenProviderBoundsCacheAndConditionallyInvalidates(t *testing.T) {
	mints := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		mints++
		writeFinspoTokenBundle(t, w, fmt.Sprintf("token-%d", mints), time.Now().Add(5*time.Minute), 300)
	}))
	defer server.Close()

	provider := NewAuthCoreTokenProvider(AuthCoreTokenConfig{
		AuthCoreURL: server.URL, ServiceID: "finspo-core", ServiceAPIKey: "finspo-key", HTTPClient: server.Client(),
	})
	provider.maxCacheEntries = 2
	oldToken, err := provider.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := provider.Token(context.Background(), "org-2"); err != nil {
		t.Fatal(err)
	}
	provider.Invalidate("org-1", oldToken)
	newToken, err := provider.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	provider.Invalidate("org-1", oldToken)
	if got, err := provider.Token(context.Background(), "org-1"); err != nil || got != newToken {
		t.Fatalf("stale invalidation evicted newer token: token=%q err=%v", got, err)
	}
	if _, err := provider.Token(context.Background(), "org-3"); err != nil {
		t.Fatal(err)
	}
	provider.mu.Lock()
	defer provider.mu.Unlock()
	if len(provider.cache) != 2 {
		t.Fatalf("cache entries = %d, want bounded capacity 2", len(provider.cache))
	}
	if _, ok := provider.cache["org-2"]; ok {
		t.Fatal("least recently used org-2 entry was not evicted")
	}
}

func TestAuthCoreTokenProviderRejectsInvalidBundleWithoutLeakingToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token": "secret-token-must-not-appear", "expiresAt": time.Now().Add(5 * time.Minute).Format(time.RFC3339),
			"expiresInSeconds": 300, "issuer": "issuer", "audience": "wrong-plane",
		})
	}))
	defer server.Close()

	provider := NewAuthCoreTokenProvider(AuthCoreTokenConfig{
		AuthCoreURL: server.URL, ServiceID: "finspo-core", ServiceAPIKey: "finspo-key", HTTPClient: server.Client(),
	})
	_, err := provider.Token(context.Background(), "org-1")
	if err == nil {
		t.Fatal("invalid audience accepted")
	}
	if got := err.Error(); got == "" || contains(got, "secret-token-must-not-appear") {
		t.Fatalf("unsafe error = %q", got)
	}
}

func TestAuthCoreTokenProviderRefusesRedirectWithoutForwardingCredential(t *testing.T) {
	forwardedCredential := ""
	redirectTarget := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		forwardedCredential = r.Header.Get("X-Service-API-Key")
		w.WriteHeader(http.StatusOK)
	}))
	defer redirectTarget.Close()
	issuer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, redirectTarget.URL, http.StatusTemporaryRedirect)
	}))
	defer issuer.Close()

	provider := NewAuthCoreTokenProvider(AuthCoreTokenConfig{
		AuthCoreURL: issuer.URL, ServiceID: "finspo-core",
		ServiceAPIKey: "must-not-cross-redirect", HTTPClient: issuer.Client(),
	})
	if _, err := provider.Token(context.Background(), "org-1"); err == nil {
		t.Fatal("redirected token issuance unexpectedly succeeded")
	}
	if forwardedCredential != "" {
		t.Fatal("durable service credential was forwarded across a redirect")
	}
}

func writeFinspoTokenBundle(t *testing.T, w http.ResponseWriter, token string, expiresAt time.Time, expiresIn int) {
	t.Helper()
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]any{
		"token": token, "expiresAt": expiresAt.Format(time.RFC3339), "expiresInSeconds": expiresIn,
		"issuer": "http://auth-core/api/convex-auth", "audience": "data-plane",
	}); err != nil {
		t.Fatalf("encode token response: %v", err)
	}
}

func contains(value, part string) bool {
	for i := 0; i+len(part) <= len(value); i++ {
		if value[i:i+len(part)] == part {
			return true
		}
	}
	return false
}
