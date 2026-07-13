package authctx

import (
	"context"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

// verifier holds the material needed to check RS256 auth-core tokens. It
// mirrors the retrieval-engine (Rust) verifier that already runs in strict
// mode: a static PUBLIC key file is the primary path, with an optional JWKS
// endpoint for `kid` rotation layered on top.
type verifier struct {
	audience string
	issuer   string

	// staticKey is the RS256 public key loaded from JWT_PUBLIC_KEY_FILE.
	// nil when the file is unset/unreadable (JWKS then becomes mandatory).
	staticKey *rsa.PublicKey

	jwks *jwksCache
}

// newVerifier builds the verifier from the middleware Config + environment.
// Returns an error only when NEITHER a static key file NOR a JWKS URL is
// usable — enforce mode cannot verify anything in that state, so the caller
// keeps failing closed.
func newVerifier(cfg Config) (*verifier, error) {
	v := &verifier{
		audience: strings.TrimSpace(cfg.Audience),
		issuer:   strings.TrimSpace(cfg.ExpectedIssuer),
	}
	if v.audience == "" {
		return nil, errors.New("authctx: required audience is empty")
	}
	if v.issuer == "" {
		return nil, errors.New("authctx: expected issuer is empty")
	}

	if path := strings.TrimSpace(os.Getenv("JWT_PUBLIC_KEY_FILE")); path != "" {
		pem, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("authctx: read JWT_PUBLIC_KEY_FILE %q: %w", path, err)
		}
		key, err := jwt.ParseRSAPublicKeyFromPEM(pem)
		if err != nil {
			return nil, fmt.Errorf("authctx: parse RSA public key: %w", err)
		}
		v.staticKey = key
	}

	jwksURL := strings.TrimSpace(os.Getenv("AUTH_CORE_JWKS_URL"))
	if jwksURL == "" {
		jwksURL = strings.TrimSpace(os.Getenv("JWT_JWKS_URL"))
	}
	if jwksURL == "" {
		jwksURL = strings.TrimSpace(cfg.JWKSURL)
	}
	if jwksURL != "" {
		parsedURL, err := url.ParseRequestURI(jwksURL)
		if err != nil || (parsedURL.Scheme != "http" && parsedURL.Scheme != "https") || parsedURL.Host == "" || parsedURL.User != nil {
			return nil, fmt.Errorf("authctx: invalid JWKS URL %q", jwksURL)
		}
		v.jwks = newJWKSCache(jwksURL)
	}

	if v.staticKey == nil && v.jwks == nil {
		return nil, errors.New("authctx: enforce mode needs JWT_PUBLIC_KEY_FILE or a JWKS URL, both unset")
	}
	return v, nil
}

// Verify checks the token's RS256 signature, audience, issuer, and expiry
// (with the same 30s skew the mint side uses), and returns verified Claims.
// On any failure it returns an error and the caller responds 401.
func (v *verifier) Verify(token string) (*Claims, error) {
	keyfunc := func(t *jwt.Token) (interface{}, error) {
		if _, ok := t.Method.(*jwt.SigningMethodRSA); !ok {
			return nil, fmt.Errorf("unexpected signing method %v", t.Header["alg"])
		}
		// Prefer a JWKS key matched by `kid` (rotation-safe); fall back to
		// the static file key.
		if v.jwks != nil {
			if kid, _ := t.Header["kid"].(string); kid != "" {
				if key, err := v.jwks.keyForKid(kid); err == nil && key != nil {
					return key, nil
				}
			}
		}
		if v.staticKey != nil {
			return v.staticKey, nil
		}
		return nil, errors.New("no verification key available for token")
	}

	opts := []jwt.ParserOption{
		jwt.WithValidMethods([]string{"RS256"}),
		jwt.WithLeeway(30 * time.Second),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
	}
	if v.audience != "" {
		opts = append(opts, jwt.WithAudience(v.audience))
	}
	if v.issuer != "" {
		opts = append(opts, jwt.WithIssuer(v.issuer))
	}

	parsed, err := jwt.Parse(token, keyfunc, opts...)
	if err != nil {
		return nil, err
	}
	if !parsed.Valid {
		return nil, errors.New("token is not valid")
	}

	mc, ok := parsed.Claims.(jwt.MapClaims)
	if !ok {
		return nil, errors.New("unexpected claims type")
	}
	claims := claimsFromMap(mc)
	if claims.IssuedAt <= 0 || claims.NotBefore <= 0 {
		return nil, errors.New("token missing required iat or nbf claim")
	}
	if claims.OrgID == "" {
		return nil, errors.New("token missing org_id claim")
	}
	identityValid := false
	switch claims.PrincipalType {
	case "user":
		identityValid = claims.UserID != "" && claims.ServiceID == "" && claims.UserID == claims.Subject
	case "service":
		identityValid = claims.ServiceID != "" && claims.UserID == "" && claims.ServiceID == claims.Subject && len(claims.Scopes) > 0
	}
	if !identityValid || claims.Subject == "" {
		return nil, errors.New("token has missing or ambiguous principal identity")
	}
	claims.Verified = true
	return claims, nil
}

func claimsFromMap(mc jwt.MapClaims) *Claims {
	getStr := func(key string) string {
		if s, ok := mc[key].(string); ok {
			return s
		}
		return ""
	}
	getInt := func(key string) int64 {
		switch n := mc[key].(type) {
		case float64:
			return int64(n)
		case int64:
			return n
		}
		return 0
	}
	c := &Claims{
		UserID:        getStr("user_id"),
		ServiceID:     getStr("service_id"),
		PrincipalType: getStr("principal_type"),
		OrgID:         getStr("org_id"),
		Email:         getStr("email"),
		Issuer:        getStr("iss"),
		Subject:       getStr("sub"),
		IssuedAt:      getInt("iat"),
		NotBefore:     getInt("nbf"),
		ExpiresAt:     getInt("exp"),
	}
	// aud may be a string or an array; take the first string form.
	switch aud := mc["aud"].(type) {
	case string:
		c.Audience = aud
	case []interface{}:
		if len(aud) > 0 {
			if s, ok := aud[0].(string); ok {
				c.Audience = s
			}
		}
	}
	if scopes, ok := mc["scopes"].([]interface{}); ok {
		for _, s := range scopes {
			if str, ok := s.(string); ok && str != "" && str == strings.TrimSpace(str) {
				c.Scopes = append(c.Scopes, str)
			}
		}
	}
	return c
}

// jwksCache fetches auth-core's JWKS document and resolves RSA keys by `kid`,
// refreshing on a miss (cheap: auth-core rotates rarely). Stdlib-only so we
// don't pull an extra dependency the offline build can't resolve.
type jwksCache struct {
	url string

	mu   sync.RWMutex
	keys map[string]*rsa.PublicKey
	// retryAfter bounds outbound refreshes for attacker-controlled unknown
	// kids. It is cleared when a refresh actually finds the requested kid,
	// allowing a legitimate subsequent rotation to be picked up immediately.
	retryAfter time.Time
}

func newJWKSCache(url string) *jwksCache {
	return &jwksCache{
		url:  url,
		keys: map[string]*rsa.PublicKey{},
	}
}

func (j *jwksCache) keyForKid(kid string) (*rsa.PublicKey, error) {
	j.mu.RLock()
	key := j.keys[kid]
	j.mu.RUnlock()
	if key != nil {
		return key, nil
	}
	// Miss: refresh at most every 30s to avoid hammering auth-core when a
	// bogus kid is presented repeatedly.
	j.mu.Lock()
	defer j.mu.Unlock()
	if key := j.keys[kid]; key != nil {
		return key, nil
	}
	now := time.Now()
	if now.Before(j.retryAfter) {
		return nil, fmt.Errorf("kid %q not in cached JWKS", kid)
	}
	j.retryAfter = now.Add(30 * time.Second)
	if err := j.refreshLocked(); err != nil {
		return nil, err
	}
	if key := j.keys[kid]; key != nil {
		j.retryAfter = time.Time{}
		return key, nil
	}
	return nil, fmt.Errorf("kid %q not found in JWKS", kid)
}

type jwksDoc struct {
	Keys []struct {
		Kid string `json:"kid"`
		Kty string `json:"kty"`
		N   string `json:"n"`
		E   string `json:"e"`
	} `json:"keys"`
}

func (j *jwksCache) refreshLocked() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, j.url, nil)
	if err != nil {
		return err
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("JWKS fetch %s: status %d", j.url, resp.StatusCode)
	}
	var doc jwksDoc
	if err := json.NewDecoder(resp.Body).Decode(&doc); err != nil {
		return err
	}
	next := make(map[string]*rsa.PublicKey, len(doc.Keys))
	for _, k := range doc.Keys {
		if k.Kty != "RSA" || k.N == "" || k.E == "" {
			continue
		}
		nBytes, err := base64.RawURLEncoding.DecodeString(k.N)
		if err != nil {
			continue
		}
		eBytes, err := base64.RawURLEncoding.DecodeString(k.E)
		if err != nil {
			continue
		}
		next[k.Kid] = &rsa.PublicKey{
			N: new(big.Int).SetBytes(nBytes),
			E: int(new(big.Int).SetBytes(eBytes).Int64()),
		}
	}
	j.keys = next
	return nil
}
