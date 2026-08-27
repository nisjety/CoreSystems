// Package authctx verifies Control Plane data-plane tokens and pins request
// identity to their signed claims. It deliberately has no observe, permissive,
// header-only, or shared-key mode.
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

const jwksRefreshBackoff = 30 * time.Second

// Claims is the verified identity contract minted by Control Plane auth-core.
type Claims struct {
	OrgID string `json:"org_id"`
	// UserID is set for human principals only. Service-principal tokens carry
	// ServiceID/PrincipalType instead and MUST leave this empty — see Verify.
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type,omitempty"`
	Email         string   `json:"email,omitempty"`
	Scopes        []string `json:"scopes,omitempty"`
	Verified      bool     `json:"-"`
	jwt.RegisteredClaims
}

// PrincipalID is the acting identity, whichever kind it is. Mirrors
// wiki-store-go and documents-api-go so audit rows across the plane name the
// principal the same way.
func (c *Claims) PrincipalID() string {
	if c == nil {
		return ""
	}
	if c.ServiceID != "" {
		return c.ServiceID
	}
	return c.UserID
}

// IsService reports whether the verified principal is a service, not a person.
func (c *Claims) IsService() bool { return c != nil && c.ServiceID != "" }

func (c *Claims) HasScope(scope string) bool {
	if c == nil || !c.Verified {
		return false
	}
	for _, candidate := range c.Scopes {
		if candidate == scope {
			return true
		}
	}
	return false
}

type contextKey struct{}

type requestIdentity struct {
	claims        *Claims
	authorization string
}

// FromContext returns only cryptographically verified claims installed by
// Middleware. Callers must never fall back to tenant headers.
func FromContext(ctx context.Context) (*Claims, bool) {
	identity, ok := ctx.Value(contextKey{}).(*requestIdentity)
	if !ok || identity == nil || identity.claims == nil || !identity.claims.Verified {
		return nil, false
	}
	return identity.claims, true
}

// AuthorizationHeader returns the already-verified bearer for an immediate
// same-audience internal callback. Callers must keep it in memory only.
func AuthorizationHeader(ctx context.Context) (string, bool) {
	identity, ok := ctx.Value(contextKey{}).(*requestIdentity)
	if !ok || identity == nil || identity.authorization == "" {
		return "", false
	}
	return identity.authorization, true
}

// Config defines the verification boundary. Audience and Issuer are required,
// as is either a static RSA public key or a JWKS endpoint.
type Config struct {
	Audience      string
	Issuer        string
	PublicKeyFile string
	PublicKeyPEM  []byte
	JWKSURL       string
	HTTPClient    *http.Client
}

// TokenVerifier enables route tests to exercise middleware wiring separately
// from the cryptographic verifier tests.
type TokenVerifier interface {
	Verify(token string) (*Claims, error)
}

// Verifier checks RS256 signature, issuer, audience, time bounds, and the
// unambiguous org/user claim pair emitted by Control Plane.
type Verifier struct {
	audience string
	issuer   string
	static   *rsa.PublicKey
	jwks     *jwksCache
}

func NewVerifier(cfg Config) (*Verifier, error) {
	audience := strings.TrimSpace(cfg.Audience)
	issuer := strings.TrimSpace(cfg.Issuer)
	if audience == "" || issuer == "" {
		return nil, errors.New("authctx: audience and issuer are required")
	}

	verifier := &Verifier{audience: audience, issuer: issuer}
	publicPEM := cfg.PublicKeyPEM
	if path := strings.TrimSpace(cfg.PublicKeyFile); path != "" {
		loaded, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("authctx: read public key: %w", err)
		}
		publicPEM = loaded
	}
	if len(publicPEM) > 0 {
		key, err := jwt.ParseRSAPublicKeyFromPEM(publicPEM)
		if err != nil {
			return nil, fmt.Errorf("authctx: parse RSA public key: %w", err)
		}
		verifier.static = key
	}

	if rawURL := strings.TrimSpace(cfg.JWKSURL); rawURL != "" {
		parsedURL, err := url.ParseRequestURI(rawURL)
		if err != nil || parsedURL.Host == "" || (parsedURL.Scheme != "http" && parsedURL.Scheme != "https") {
			return nil, errors.New("authctx: JWKS URL must be an absolute HTTP(S) URL")
		}
		client := cfg.HTTPClient
		if client == nil {
			client = &http.Client{Timeout: 5 * time.Second}
		}
		verifier.jwks = newJWKSCache(rawURL, client)
	}

	if verifier.static == nil && verifier.jwks == nil {
		return nil, errors.New("authctx: JWT public key or JWKS URL is required")
	}
	return verifier, nil
}

func (v *Verifier) Verify(token string) (*Claims, error) {
	claims := &Claims{}
	parsed, err := jwt.ParseWithClaims(
		token,
		claims,
		v.keyForToken,
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithAudience(v.audience),
		jwt.WithIssuer(v.issuer),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
		jwt.WithLeeway(30*time.Second),
	)
	if err != nil {
		return nil, fmt.Errorf("authctx: verify token: %w", err)
	}
	if !parsed.Valid {
		return nil, errors.New("authctx: invalid token")
	}
	if claims.IssuedAt == nil || claims.NotBefore == nil || claims.ExpiresAt == nil {
		return nil, errors.New("authctx: token requires iat, nbf, and exp")
	}
	claims.OrgID = strings.TrimSpace(claims.OrgID)
	claims.UserID = strings.TrimSpace(claims.UserID)
	claims.ServiceID = strings.TrimSpace(claims.ServiceID)
	claims.PrincipalType = strings.ToLower(strings.TrimSpace(claims.PrincipalType))
	claims.Subject = strings.TrimSpace(claims.Subject)
	if claims.OrgID == "" || claims.Subject == "" {
		return nil, errors.New("authctx: token requires org_id and sub")
	}
	// Accept EITHER a user or a service principal, and validate the full shape
	// of whichever one is claimed. Ported verbatim from wiki-store-go so the
	// plane has one identity contract rather than four.
	//
	// This previously required a `user_id` equal to `sub`, unconditionally.
	// auth-core's `issuePlaneToken` emits `sub` + `service_id` +
	// `principal_type: service` for service principals and NO `user_id`, so
	// every service token was rejected 401 "invalid credentials" — silently,
	// with nothing logged. For an ORCHESTRATOR that is the wrong way round: its
	// whole purpose is automated, unattended work (stale-embedding sweeps,
	// reindex jobs), and those callers are exactly the ones that hold a service
	// credential rather than a human session.
	//
	// Note this is STRICTER than the old check for service tokens, not looser:
	// a service identity must be internally consistent (principal_type set,
	// sub == service_id, no user_id, non-empty scopes) and authorization is
	// still entirely scope-gated afterwards.
	switch {
	case claims.ServiceID != "" || claims.PrincipalType == "service":
		if claims.PrincipalType != "service" || claims.ServiceID == "" ||
			claims.UserID != "" || claims.Subject != claims.ServiceID ||
			len(claims.Scopes) == 0 {
			return nil, errors.New("authctx: ambiguous service identity")
		}
	case claims.UserID != "":
		if claims.PrincipalType != "" && claims.PrincipalType != "user" {
			return nil, errors.New("authctx: ambiguous user identity")
		}
		if claims.Subject != claims.UserID {
			return nil, errors.New("authctx: ambiguous user identity")
		}
	default:
		return nil, errors.New("authctx: token requires one user or service identity")
	}
	claims.Verified = true
	return claims, nil
}

func (v *Verifier) keyForToken(token *jwt.Token) (any, error) {
	if token.Method.Alg() != jwt.SigningMethodRS256.Alg() {
		return nil, fmt.Errorf("authctx: signing method %q is not allowed", token.Method.Alg())
	}
	if v.jwks != nil {
		if kid, _ := token.Header["kid"].(string); strings.TrimSpace(kid) != "" {
			if key, err := v.jwks.keyForKid(kid); err == nil {
				return key, nil
			} else if v.static == nil {
				return nil, err
			}
		}
	}
	if v.static != nil {
		return v.static, nil
	}
	return nil, errors.New("authctx: token kid is required for JWKS verification")
}

// Middleware requires one valid bearer token and rejects any tenant header
// that conflicts with signed claims. Downstream code receives only claims.
func Middleware(verifier TokenVerifier) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := extractBearer(r.Header.Get("Authorization"))
			if token == "" || verifier == nil {
				writeAuthError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
				return
			}
			claims, err := verifier.Verify(token)
			if err != nil || claims == nil || !claims.Verified {
				writeAuthError(w, http.StatusUnauthorized, "unauthorized", "invalid credentials")
				return
			}
			if headerOrg := strings.TrimSpace(r.Header.Get("X-Org-ID")); headerOrg != "" && headerOrg != claims.OrgID {
				writeAuthError(w, http.StatusForbidden, "tenant_mismatch", "request tenant does not match verified identity")
				return
			}
			identity := &requestIdentity{claims: claims, authorization: "Bearer " + token}
			ctx := context.WithValue(r.Context(), contextKey{}, identity)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

// RequireAnyScope authorizes a mutation only when the verified principal has
// one of the explicitly accepted capabilities.
func RequireAnyScope(scopes ...string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			claims, ok := FromContext(r.Context())
			if !ok {
				writeAuthError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
				return
			}
			for _, scope := range scopes {
				if claims.HasScope(scope) {
					next.ServeHTTP(w, r)
					return
				}
			}
			writeAuthError(w, http.StatusForbidden, "insufficient_scope", "dedicated operation scope required")
		})
	}
}

func extractBearer(header string) string {
	parts := strings.Fields(header)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return parts[1]
}

func writeAuthError(w http.ResponseWriter, status int, code, message string) {
	w.Header().Set("Content-Type", "application/json")
	if status == http.StatusUnauthorized {
		w.Header().Set("WWW-Authenticate", "Bearer")
	}
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]string{"code": code, "message": message},
	})
}

type jwksCache struct {
	url    string
	client *http.Client

	mu          sync.RWMutex
	keys        map[string]*rsa.PublicKey
	lastAttempt time.Time
}

func newJWKSCache(url string, client *http.Client) *jwksCache {
	return &jwksCache{url: url, client: client, keys: make(map[string]*rsa.PublicKey)}
}

func (j *jwksCache) keyForKid(kid string) (*rsa.PublicKey, error) {
	j.mu.RLock()
	key := j.keys[kid]
	j.mu.RUnlock()
	if key != nil {
		return key, nil
	}

	j.mu.Lock()
	defer j.mu.Unlock()
	if key := j.keys[kid]; key != nil {
		return key, nil
	}
	if !j.lastAttempt.IsZero() && time.Since(j.lastAttempt) < jwksRefreshBackoff {
		return nil, fmt.Errorf("authctx: kid %q is not in cached JWKS", kid)
	}
	j.lastAttempt = time.Now()
	if err := j.refreshLocked(); err != nil {
		return nil, err
	}
	if key := j.keys[kid]; key != nil {
		return key, nil
	}
	return nil, fmt.Errorf("authctx: kid %q not found in JWKS", kid)
}

type jwksDocument struct {
	Keys []struct {
		Kid string `json:"kid"`
		Kty string `json:"kty"`
		Use string `json:"use"`
		Alg string `json:"alg"`
		N   string `json:"n"`
		E   string `json:"e"`
	} `json:"keys"`
}

func (j *jwksCache) refreshLocked() error {
	req, err := http.NewRequest(http.MethodGet, j.url, nil)
	if err != nil {
		return fmt.Errorf("authctx: create JWKS request: %w", err)
	}
	resp, err := j.client.Do(req)
	if err != nil {
		return fmt.Errorf("authctx: fetch JWKS: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("authctx: fetch JWKS: unexpected status %d", resp.StatusCode)
	}

	var document jwksDocument
	if err := json.NewDecoder(resp.Body).Decode(&document); err != nil {
		return fmt.Errorf("authctx: decode JWKS: %w", err)
	}
	keys := make(map[string]*rsa.PublicKey, len(document.Keys))
	for _, raw := range document.Keys {
		if raw.Kid == "" || raw.Kty != "RSA" || raw.N == "" || raw.E == "" {
			continue
		}
		if raw.Use != "" && raw.Use != "sig" {
			continue
		}
		if raw.Alg != "" && raw.Alg != jwt.SigningMethodRS256.Alg() {
			continue
		}
		n, err := base64.RawURLEncoding.DecodeString(raw.N)
		if err != nil {
			continue
		}
		e, err := base64.RawURLEncoding.DecodeString(raw.E)
		if err != nil {
			continue
		}
		exponent := new(big.Int).SetBytes(e).Int64()
		if exponent <= 0 {
			continue
		}
		keys[raw.Kid] = &rsa.PublicKey{N: new(big.Int).SetBytes(n), E: int(exponent)}
	}
	if len(keys) == 0 {
		return errors.New("authctx: JWKS contains no usable RS256 keys")
	}
	j.keys = keys
	return nil
}
