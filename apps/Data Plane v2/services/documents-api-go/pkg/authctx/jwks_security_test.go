package authctx

import (
	"crypto/rand"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/golang-jwt/jwt/v5"
)

type rotatingJWKSServer struct {
	server  *httptest.Server
	mu      sync.RWMutex
	keys    map[string]*rsa.PublicKey
	fetches atomic.Int64
}

func newRotatingJWKSServer(t *testing.T, keys map[string]*rsa.PublicKey) *rotatingJWKSServer {
	t.Helper()
	fixture := &rotatingJWKSServer{keys: keys}
	fixture.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		fixture.fetches.Add(1)
		fixture.mu.RLock()
		defer fixture.mu.RUnlock()

		type jwk struct {
			Kid string `json:"kid"`
			Kty string `json:"kty"`
			N   string `json:"n"`
			E   string `json:"e"`
		}
		doc := struct {
			Keys []jwk `json:"keys"`
		}{Keys: make([]jwk, 0, len(fixture.keys))}
		for kid, key := range fixture.keys {
			doc.Keys = append(doc.Keys, jwk{
				Kid: kid,
				Kty: "RSA",
				N:   base64.RawURLEncoding.EncodeToString(key.N.Bytes()),
				E:   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(key.E)).Bytes()),
			})
		}
		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(doc); err != nil {
			t.Errorf("encode JWKS: %v", err)
		}
	}))
	t.Cleanup(fixture.server.Close)
	return fixture
}

func (s *rotatingJWKSServer) rotate(keys map[string]*rsa.PublicKey) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.keys = keys
}

func generateRSAKey(t *testing.T) *rsa.PrivateKey {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	return key
}

func signTokenWithKid(t *testing.T, key *rsa.PrivateKey, kid string, claims jwt.MapClaims) string {
	t.Helper()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = kid
	signed, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign token with kid: %v", err)
	}
	return signed
}

func newJWKSVerifier(t *testing.T, jwksURL string) *verifier {
	t.Helper()
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", "")
	v, err := newVerifier(Config{
		Audience:       testAudience,
		ExpectedIssuer: testIssuer,
		JWKSURL:        jwksURL,
	})
	if err != nil {
		t.Fatalf("new JWKS verifier: %v", err)
	}
	return v
}

func TestVerifyRefreshesImmediatelyForNewRotatedKid(t *testing.T) {
	oldKey := generateRSAKey(t)
	newKey := generateRSAKey(t)
	jwks := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"old": &oldKey.PublicKey})
	v := newJWKSVerifier(t, jwks.server.URL)

	if _, err := v.Verify(signTokenWithKid(t, oldKey, "old", validClaims())); err != nil {
		t.Fatalf("verify token before rotation: %v", err)
	}
	jwks.rotate(map[string]*rsa.PublicKey{"new": &newKey.PublicKey})

	if _, err := v.Verify(signTokenWithKid(t, newKey, "new", validClaims())); err != nil {
		t.Fatalf("verify token immediately after JWKS rotation: %v", err)
	}
	if got := jwks.fetches.Load(); got != 2 {
		t.Fatalf("JWKS fetches = %d, want one initial fetch and one rotation refresh", got)
	}
}

func TestVerifyCachesSuccessfulJWKSLookup(t *testing.T) {
	key := generateRSAKey(t)
	jwks := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"active": &key.PublicKey})
	v := newJWKSVerifier(t, jwks.server.URL)
	token := signTokenWithKid(t, key, "active", validClaims())

	for i := 0; i < 2; i++ {
		if _, err := v.Verify(token); err != nil {
			t.Fatalf("verify cached token attempt %d: %v", i+1, err)
		}
	}
	if got := jwks.fetches.Load(); got != 1 {
		t.Fatalf("JWKS fetches = %d, want 1 for a cached kid", got)
	}
}

func TestVerifyFailsClosedWhenJWKSUnavailableAndRateLimitsSameKid(t *testing.T) {
	var fetches atomic.Int64
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		fetches.Add(1)
		http.Error(w, "temporarily unavailable", http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)
	v := newJWKSVerifier(t, server.URL)
	key := generateRSAKey(t)
	token := signTokenWithKid(t, key, "unavailable", validClaims())

	for i := 0; i < 2; i++ {
		if _, err := v.Verify(token); err == nil {
			t.Fatalf("attempt %d accepted a token while JWKS was unavailable", i+1)
		}
	}
	if got := fetches.Load(); got != 1 {
		t.Fatalf("JWKS fetches = %d, want one fetch for a repeatedly missing kid", got)
	}
}

func TestVerifyRateLimitsDifferentUnknownKids(t *testing.T) {
	activeKey := generateRSAKey(t)
	jwks := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"active": &activeKey.PublicKey})
	v := newJWKSVerifier(t, jwks.server.URL)
	attackerKey := generateRSAKey(t)

	for _, kid := range []string{"attacker-kid-1", "attacker-kid-2"} {
		if _, err := v.Verify(signTokenWithKid(t, attackerKey, kid, validClaims())); err == nil {
			t.Fatalf("unknown kid %q was accepted", kid)
		}
	}
	if got := jwks.fetches.Load(); got != 1 {
		t.Fatalf("JWKS fetches = %d, want one bounded refresh for different attacker-controlled kids", got)
	}
}

func TestVerifyFailsClosedForMalformedJWKSResponses(t *testing.T) {
	tests := []struct {
		name string
		body string
	}{
		{name: "invalid JSON", body: `{not-json`},
		{name: "no usable keys", body: `{"keys":[{"kid":"target","kty":"EC","n":"unused","e":"unused"}]}`},
		{name: "invalid modulus", body: `{"keys":[{"kid":"target","kty":"RSA","n":"%%%","e":"AQAB"}]}`},
		{name: "invalid exponent", body: `{"keys":[{"kid":"target","kty":"RSA","n":"AQ","e":"%%%"}]}`},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.Header().Set("Content-Type", "application/json")
				_, _ = w.Write([]byte(tc.body))
			}))
			t.Cleanup(server.Close)
			v := newJWKSVerifier(t, server.URL)
			key := generateRSAKey(t)
			if _, err := v.Verify(signTokenWithKid(t, key, "target", validClaims())); err == nil {
				t.Fatal("malformed JWKS response must not authenticate the token")
			}
		})
	}
}

func TestVerifyJWKSOnlyRejectsTokenWithoutKidWithoutFetching(t *testing.T) {
	key := generateRSAKey(t)
	jwks := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"active": &key.PublicKey})
	v := newJWKSVerifier(t, jwks.server.URL)

	if _, err := v.Verify(signToken(t, key, validClaims())); err == nil {
		t.Fatal("JWKS-only verifier accepted a token without a kid")
	}
	if got := jwks.fetches.Load(); got != 0 {
		t.Fatalf("JWKS fetches = %d, want 0 when token has no kid", got)
	}
}

func TestVerifyCanUsePinnedStaticKeyDuringJWKSOutage(t *testing.T) {
	key := writeTestKey(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "temporarily unavailable", http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)
	v, err := newVerifier(Config{
		Audience:       testAudience,
		ExpectedIssuer: testIssuer,
		JWKSURL:        server.URL,
	})
	if err != nil {
		t.Fatalf("new verifier with pinned key: %v", err)
	}

	if _, err := v.Verify(signTokenWithKid(t, key, "rotating", validClaims())); err != nil {
		t.Fatalf("pinned static verification key should remain usable during JWKS outage: %v", err)
	}
}

func TestNewVerifierAcceptsJWKSURLFromSupportedEnvironmentVariables(t *testing.T) {
	key := generateRSAKey(t)
	jwks := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"active": &key.PublicKey})
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", jwks.server.URL)

	v, err := newVerifier(Config{Audience: testAudience, ExpectedIssuer: testIssuer})
	if err != nil {
		t.Fatalf("new verifier from JWT_JWKS_URL: %v", err)
	}
	if _, err := v.Verify(signTokenWithKid(t, key, "active", validClaims())); err != nil {
		t.Fatalf("verify token using JWT_JWKS_URL: %v", err)
	}

	// AUTH_CORE_JWKS_URL is the highest-priority runtime override.
	second := newRotatingJWKSServer(t, map[string]*rsa.PublicKey{"second": &key.PublicKey})
	t.Setenv("AUTH_CORE_JWKS_URL", second.server.URL)
	t.Setenv("JWT_JWKS_URL", "https://unused.invalid/jwks")
	v, err = newVerifier(Config{Audience: testAudience, ExpectedIssuer: testIssuer})
	if err != nil {
		t.Fatalf("new verifier from AUTH_CORE_JWKS_URL: %v", err)
	}
	if _, err := v.Verify(signTokenWithKid(t, key, "second", validClaims())); err != nil {
		t.Fatalf("verify token using AUTH_CORE_JWKS_URL: %v", err)
	}
	if second.fetches.Load() != 1 {
		t.Fatalf("AUTH_CORE_JWKS_URL was not used: fetches=%d", second.fetches.Load())
	}
}
