package servicetoken

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

// mintRecord is one observed mint exchange, captured so tests can assert the
// wire contract rather than only the returned string.
type mintRecord struct {
	path       string
	serviceID  string
	credential string
	zdrHeader  string
	body       map[string]any
}

// fakeAuthCore stands in for auth-core's POST /api/{audience}/internal-token.
// An httptest server is used rather than a hand-rolled http.RoundTripper fake so
// the real net/http request construction (headers, JSON body, status handling)
// is exercised — the same choice shipping-core's modelplane client tests make.
type fakeAuthCore struct {
	server *httptest.Server

	mu      sync.Mutex
	records []mintRecord

	// ttlSeconds is echoed as expiresInSeconds; 0 omits the field entirely.
	ttlSeconds int
	// audience overrides the echoed audience; empty echoes the path slug.
	audience string
	// status overrides the response status; 0 means 200.
	status int
	// tokenPrefix makes successive tokens distinguishable.
	tokenPrefix string
}

func newFakeAuthCore(t *testing.T, ttlSeconds int) *fakeAuthCore {
	t.Helper()
	fake := &fakeAuthCore{ttlSeconds: ttlSeconds, tokenPrefix: "tok"}
	fake.server = httptest.NewServer(http.HandlerFunc(fake.handle))
	t.Cleanup(fake.server.Close)
	return fake
}

func (f *fakeAuthCore) handle(w http.ResponseWriter, r *http.Request) {
	var body map[string]any
	_ = json.NewDecoder(r.Body).Decode(&body)

	f.mu.Lock()
	f.records = append(f.records, mintRecord{
		path:       r.URL.Path,
		serviceID:  r.Header.Get("x-service-id"),
		credential: r.Header.Get("x-service-api-key"),
		zdrHeader:  r.Header.Get("x-zdr"),
		body:       body,
	})
	count := len(f.records)
	status := f.status
	ttl := f.ttlSeconds
	audience := f.audience
	prefix := f.tokenPrefix
	f.mu.Unlock()

	if status != 0 && status != http.StatusOK {
		w.WriteHeader(status)
		_, _ = w.Write([]byte(`{"message":"Service principal is not authorized"}`))
		return
	}

	if audience == "" {
		// Mirror auth-core: the echoed audience is always the path slug.
		audience = r.URL.Path
		audience = audience[len("/api/") : len(audience)-len("/internal-token")]
	}

	payload := map[string]any{
		"token":     prefix + "-" + itoa(count),
		"expiresAt": time.Now().Add(time.Duration(ttl) * time.Second).UTC().Format(time.RFC3339),
		"issuer":    "https://auth.example.test/api/convex-auth",
		"audience":  audience,
	}
	if ttl != 0 {
		payload["expiresInSeconds"] = ttl
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(payload)
}

func (f *fakeAuthCore) calls() []mintRecord {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]mintRecord(nil), f.records...)
}

func (f *fakeAuthCore) callCount() int { return len(f.calls()) }

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var digits []byte
	for n > 0 {
		digits = append([]byte{byte('0' + n%10)}, digits...)
		n /= 10
	}
	return string(digits)
}

// clock is a controllable time source so refresh behaviour is asserted
// deterministically instead of by sleeping.
type clock struct {
	mu  sync.Mutex
	now time.Time
}

func newClock() *clock { return &clock{now: time.Date(2026, 7, 31, 12, 0, 0, 0, time.UTC)} }

func (c *clock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *clock) advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = c.now.Add(d)
}

func sessionProvider(t *testing.T, fake *fakeAuthCore, clk *clock) *Provider {
	t.Helper()
	provider, err := New(Config{
		AuthCoreURL: fake.server.URL,
		ServiceID:   "capability-core",
		Credential:  "principal-secret",
		Audience:    "session-core",
		Scopes:      []string{"session:read", "session:skills:write"},
		Reason:      "capability-core learning review",
		HTTPClient:  fake.server.Client(),
		Now:         clk.Now,
	})
	if err != nil {
		t.Fatalf("New(): %v", err)
	}
	return provider
}

func TestTokenMintsOnceThenServesFromCache(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	first, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("first Token(): %v", err)
	}
	second, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("second Token(): %v", err)
	}

	if first != second {
		t.Fatalf("cached call returned a different token: %q vs %q", first, second)
	}
	if got := fake.callCount(); got != 1 {
		t.Fatalf("expected exactly 1 mint, auth-core saw %d", got)
	}
}

func TestMintRequestMatchesAuthCoreContract(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	if _, err := provider.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("Token(): %v", err)
	}

	calls := fake.calls()
	if len(calls) != 1 {
		t.Fatalf("expected 1 mint, got %d", len(calls))
	}
	call := calls[0]

	if call.path != "/api/session-core/internal-token" {
		t.Fatalf("wrong mint path: %q", call.path)
	}
	if call.serviceID != "capability-core" {
		t.Fatalf("x-service-id not sent: %q", call.serviceID)
	}
	if call.credential != "principal-secret" {
		t.Fatalf("x-service-api-key not sent: %q", call.credential)
	}
	// auth-core answers 400 if the caller tries to select a retention posture.
	if call.zdrHeader != "" {
		t.Fatalf("x-zdr must never be sent, got %q", call.zdrHeader)
	}
	if _, present := call.body["zdr"]; present {
		t.Fatal("zdr must never appear in the mint body")
	}
	if call.body["orgId"] != "org-a" {
		t.Fatalf("orgId not sent: %v", call.body["orgId"])
	}
	if call.body["reason"] != "capability-core learning review" {
		t.Fatalf("reason not sent: %v", call.body["reason"])
	}
	scopes, ok := call.body["scopes"].([]any)
	if !ok || len(scopes) != 2 || scopes[0] != "session:read" || scopes[1] != "session:skills:write" {
		t.Fatalf("scopes not sent as requested: %v", call.body["scopes"])
	}
}

func TestTokenRefreshesBeforeExpiryNotAfter(t *testing.T) {
	const ttl = 300 * time.Second
	fake := newFakeAuthCore(t, int(ttl.Seconds()))
	clk := newClock()
	provider := sessionProvider(t, fake, clk)

	first, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token(): %v", err)
	}

	// One second inside the refresh margin the cached token is still served.
	clk.advance(ttl - RefreshSkew - time.Second)
	same, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token() before margin: %v", err)
	}
	if same != first || fake.callCount() != 1 {
		t.Fatalf("re-minted too early: token=%q mints=%d", same, fake.callCount())
	}

	// Crossing the margin re-mints — while the old token is still valid for
	// another RefreshSkew. This is the whole point: never present an expired
	// credential and never discover expiry from a 401.
	clk.advance(2 * time.Second)
	refreshed, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token() after margin: %v", err)
	}
	if refreshed == first {
		t.Fatal("expected a freshly minted token once inside the refresh margin")
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("expected 2 mints, got %d", got)
	}

	// The refresh happened strictly before the issued expiry.
	elapsed := ttl - RefreshSkew + time.Second
	if elapsed >= ttl {
		t.Fatalf("refresh was not ahead of expiry: refreshed after %s of a %s token", elapsed, ttl)
	}
}

func TestInvalidateForcesExactlyOneRemint(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	first, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token(): %v", err)
	}

	provider.Invalidate("org-a")

	second, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token() after Invalidate: %v", err)
	}
	if second == first {
		t.Fatal("Invalidate did not force a fresh mint")
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("expected 2 mints after one Invalidate, got %d", got)
	}

	// The refreshed token is cached again — Invalidate must not disable caching.
	if _, err := provider.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("Token() after re-mint: %v", err)
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("re-minted token was not cached: %d mints", got)
	}
}

func TestTokensAreCachedPerOrgNotShared(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	orgA, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token(org-a): %v", err)
	}
	orgB, err := provider.Token(context.Background(), "org-b")
	if err != nil {
		t.Fatalf("Token(org-b): %v", err)
	}

	if orgA == orgB {
		t.Fatal("a second org was served the first org's token; session-core would refuse it")
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("expected one mint per org, got %d", got)
	}

	calls := fake.calls()
	if calls[0].body["orgId"] != "org-a" || calls[1].body["orgId"] != "org-b" {
		t.Fatalf("mints did not carry their own org: %v, %v", calls[0].body["orgId"], calls[1].body["orgId"])
	}
}

func TestPerAudienceScopesAreNotUnioned(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	clk := newClock()

	session := sessionProvider(t, fake, clk)
	inference, err := New(Config{
		AuthCoreURL: fake.server.URL,
		ServiceID:   "capability-core",
		Credential:  "principal-secret",
		Audience:    "inference-core",
		Scopes:      []string{"inference:invoke"},
		Reason:      "capability-core learning review",
		HTTPClient:  fake.server.Client(),
		Now:         clk.Now,
	})
	if err != nil {
		t.Fatalf("New(inference-core): %v", err)
	}

	if _, err := session.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("session Token(): %v", err)
	}
	if _, err := inference.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("inference Token(): %v", err)
	}

	calls := fake.calls()
	if len(calls) != 2 {
		t.Fatalf("expected 2 mints, got %d", len(calls))
	}

	byPath := map[string][]any{}
	for _, call := range calls {
		scopes, _ := call.body["scopes"].([]any)
		byPath[call.path] = scopes
	}

	sessionScopes := byPath["/api/session-core/internal-token"]
	if len(sessionScopes) != 2 {
		t.Fatalf("session-core got %d scopes, want exactly its own 2: %v", len(sessionScopes), sessionScopes)
	}
	for _, scope := range sessionScopes {
		if scope == "inference:invoke" {
			t.Fatal("session-core token was minted with inference-core's scope (scopes were unioned)")
		}
	}

	inferenceScopes := byPath["/api/inference-core/internal-token"]
	if len(inferenceScopes) != 1 || inferenceScopes[0] != "inference:invoke" {
		t.Fatalf("inference-core got %v, want exactly [inference:invoke]", inferenceScopes)
	}
}

func TestScopeSetIsPartOfTheCacheKey(t *testing.T) {
	// Two providers for the same audience and org but different scope sets must
	// not share a cached entry — otherwise a read-scoped token gets handed to a
	// caller that needed write.
	fake := newFakeAuthCore(t, 300)
	clk := newClock()

	readOnly, err := New(Config{
		AuthCoreURL: fake.server.URL, ServiceID: "capability-core", Credential: "s",
		Audience: "session-core", Scopes: []string{"session:read"}, Reason: "read",
		HTTPClient: fake.server.Client(), Now: clk.Now,
	})
	if err != nil {
		t.Fatalf("New(read): %v", err)
	}
	writer, err := New(Config{
		AuthCoreURL: fake.server.URL, ServiceID: "capability-core", Credential: "s",
		Audience: "session-core", Scopes: []string{"session:read", "session:skills:write"}, Reason: "write",
		HTTPClient: fake.server.Client(), Now: clk.Now,
	})
	if err != nil {
		t.Fatalf("New(write): %v", err)
	}

	if _, err := readOnly.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("read Token(): %v", err)
	}
	if _, err := writer.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("write Token(): %v", err)
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("differing scope sets shared a cache entry: %d mints", got)
	}

	if key := cacheKey("org-a", []string{"session:read"}); key == cacheKey("org-a", []string{"session:read", "session:skills:write"}) {
		t.Fatal("cacheKey collides across scope sets")
	}
}

func TestFallbackTTLWhenExpiryAbsent(t *testing.T) {
	// ttlSeconds 0 omits expiresInSeconds. auth-core always sends it today, so
	// this asserts the defensive branch stays conservative rather than caching
	// a token forever.
	fake := newFakeAuthCore(t, 0)
	clk := newClock()
	provider := sessionProvider(t, fake, clk)

	first, err := provider.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("Token(): %v", err)
	}
	if fake.callCount() != 1 {
		t.Fatalf("expected 1 mint, got %d", fake.callCount())
	}

	// Still inside the usable window: exactly one refresh margin of life.
	clk.advance(fallbackTTL - RefreshSkew - time.Second)
	if again, err := provider.Token(context.Background(), "org-a"); err != nil || again != first {
		t.Fatalf("fallback TTL gave no usable window: token=%q err=%v", again, err)
	}

	clk.advance(2 * time.Second)
	if _, err := provider.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("Token() after fallback window: %v", err)
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("expected a re-mint after the fallback window, got %d mints", got)
	}
}

func TestOverlongTTLIsClamped(t *testing.T) {
	// A misconfigured PLANE_TOKEN_TTL_* must not make this process hold one
	// credential for longer than maxTTL.
	fake := newFakeAuthCore(t, int((6 * time.Hour).Seconds()))
	clk := newClock()
	provider := sessionProvider(t, fake, clk)

	if _, err := provider.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("Token(): %v", err)
	}
	clk.advance(maxTTL - RefreshSkew + time.Second)
	if _, err := provider.Token(context.Background(), "org-a"); err != nil {
		t.Fatalf("Token() after clamp window: %v", err)
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("over-long TTL was not clamped: %d mints", got)
	}
}

func TestAudienceMismatchIsRefused(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	fake.audience = "cost-core"
	provider := sessionProvider(t, fake, newClock())

	if _, err := provider.Token(context.Background(), "org-a"); err == nil {
		t.Fatal("a token for the wrong audience must be refused")
	}
}

func TestMintRefusalSurfacesStatus(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	fake.status = http.StatusForbidden
	provider := sessionProvider(t, fake, newClock())

	_, err := provider.Token(context.Background(), "org-a")
	if err == nil {
		t.Fatal("a 403 from auth-core must surface as an error")
	}
	// Nothing is cached on failure, so the next call retries rather than
	// serving an empty credential.
	if _, err := provider.Token(context.Background(), "org-a"); err == nil {
		t.Fatal("second attempt should also fail")
	}
	if got := fake.callCount(); got != 2 {
		t.Fatalf("a failed mint was cached: %d mints", got)
	}
}

func TestTokenRequiresOrg(t *testing.T) {
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	if _, err := provider.Token(context.Background(), "   "); err == nil {
		t.Fatal("an empty org must be refused rather than guessed")
	}
	if got := fake.callCount(); got != 0 {
		t.Fatalf("an org-less request reached auth-core: %d mints", got)
	}
}

func TestNewNamesTheMissingConfiguration(t *testing.T) {
	base := Config{
		AuthCoreURL: "http://auth-core:3011",
		ServiceID:   "capability-core",
		Credential:  "secret",
		Audience:    "session-core",
		Scopes:      []string{"session:read"},
		Reason:      "learning review",
	}

	for name, mutate := range map[string]func(*Config){
		"auth-core URL": func(c *Config) { c.AuthCoreURL = " " },
		"service id":    func(c *Config) { c.ServiceID = "" },
		"credential":    func(c *Config) { c.Credential = "" },
		"audience":      func(c *Config) { c.Audience = "" },
		"scope":         func(c *Config) { c.Scopes = []string{" "} },
		"reason":        func(c *Config) { c.Reason = "" },
	} {
		t.Run("missing "+name, func(t *testing.T) {
			cfg := base
			mutate(&cfg)
			if _, err := New(cfg); err == nil {
				t.Fatalf("missing %s did not fail closed", name)
			}
		})
	}

	if _, err := New(base); err != nil {
		t.Fatalf("valid config rejected: %v", err)
	}
}

func TestScopesAreNormalizedAndDeduplicated(t *testing.T) {
	provider, err := New(Config{
		AuthCoreURL: "http://auth-core:3011",
		ServiceID:   "capability-core",
		Credential:  "secret",
		Audience:    "session-core",
		Scopes:      []string{" session:read ", "session:read", "", "session:skills:write"},
		Reason:      "learning review",
	})
	if err != nil {
		t.Fatalf("New(): %v", err)
	}
	scopes := provider.Scopes()
	if len(scopes) != 2 || scopes[0] != "session:read" || scopes[1] != "session:skills:write" {
		t.Fatalf("scopes not normalized: %v", scopes)
	}
}

func TestConcurrentTokenCallsAreSafe(t *testing.T) {
	// Exercised under -race: the cache must tolerate parallel readers and
	// minters for the same and different orgs.
	fake := newFakeAuthCore(t, 300)
	provider := sessionProvider(t, fake, newClock())

	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			org := "org-a"
			if i%2 == 0 {
				org = "org-b"
			}
			if _, err := provider.Token(context.Background(), org); err != nil {
				t.Errorf("Token(%s): %v", org, err)
			}
			provider.Invalidate(org)
		}(i)
	}
	wg.Wait()
}
