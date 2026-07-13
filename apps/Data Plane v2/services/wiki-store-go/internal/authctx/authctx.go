// Package authctx verifies Control Plane data-plane tokens and pins wiki
// requests to the tenant in cryptographically verified claims.
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
	"os"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const (
	jwksRefreshBackoff = 30 * time.Second
	jwksCacheTTL       = 5 * time.Minute
)

// Claims is the verified identity contract minted by Control Plane auth-core.
type Claims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type,omitempty"`
	Email         string   `json:"email,omitempty"`
	Scopes        []string `json:"scopes,omitempty"`
	Verified      bool     `json:"-"`
	jwt.RegisteredClaims
}

func (c *Claims) PrincipalID() string {
	if c == nil {
		return ""
	}
	if c.ServiceID != "" {
		return c.ServiceID
	}
	return c.UserID
}

func (c *Claims) IsService() bool { return c != nil && c.ServiceID != "" }

func (c *Claims) hasScope(required string) bool {
	for _, scope := range c.Scopes {
		if scope == required {
			return true
		}
	}
	return false
}

type contextKey struct{}

// FromContext returns only claims installed after successful verification.
func FromContext(ctx context.Context) (*Claims, bool) {
	claims, ok := ctx.Value(contextKey{}).(*Claims)
	return claims, ok && claims != nil && claims.Verified
}

// TenantID returns the verified tenant and rejects a conflicting caller value.
// Handlers use the returned claim value rather than the request field.
func TenantID(ctx context.Context, requested string) (string, error) {
	claims, ok := FromContext(ctx)
	if !ok {
		return "", status.Error(codes.Unauthenticated, "authentication required")
	}
	requested = strings.TrimSpace(requested)
	if requested != "" && requested != claims.OrgID {
		return "", status.Error(codes.PermissionDenied, "request tenant does not match verified identity")
	}
	return claims.OrgID, nil
}

// Config defines the mandatory JWT verification boundary.
type Config struct {
	Audience      string
	Issuer        string
	PublicKeyFile string
	PublicKeyPEM  []byte
	JWKSURL       string
	HTTPClient    *http.Client
}

// TokenVerifier lets middleware wiring tests use the same narrow contract.
type TokenVerifier interface {
	Verify(token string) (*Claims, error)
}

// Verifier checks RS256 signature, issuer, audience, time bounds, and an
// unambiguous org/user claim pair.
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

	if url := strings.TrimSpace(cfg.JWKSURL); url != "" {
		client := cfg.HTTPClient
		if client == nil {
			client = &http.Client{Timeout: 5 * time.Second}
		}
		verifier.jwks = newJWKSCache(url, client)
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
	switch {
	case claims.ServiceID != "" || claims.PrincipalType == "service":
		if claims.PrincipalType != "service" || claims.ServiceID == "" || claims.UserID != "" || claims.Subject != claims.ServiceID || len(claims.Scopes) == 0 {
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

// RequireScope authorizes both users and service principals with an explicit
// least-privilege scope. Authentication alone never grants a wiki action.
func RequireScope(required string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			claims, ok := FromContext(r.Context())
			if !ok {
				writeAuthError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
				return
			}
			if !claims.hasScope(required) {
				writeAuthError(w, http.StatusForbidden, "insufficient_scope", "principal lacks required scope")
				return
			}
			next.ServeHTTP(w, r)
		})
	}
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

// Middleware requires a valid bearer and rejects tenant headers conflicting
// with the signed identity. There is no header-only or permissive mode.
func Middleware(verifier TokenVerifier) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			claims, err := verifyBearer(verifier, r.Header.Values("Authorization"))
			if err != nil {
				writeAuthError(w, http.StatusUnauthorized, "unauthorized", "invalid credentials")
				return
			}
			if headerOrg := strings.TrimSpace(r.Header.Get("X-Org-ID")); headerOrg != "" && headerOrg != claims.OrgID {
				writeAuthError(w, http.StatusForbidden, "tenant_mismatch", "request tenant does not match verified identity")
				return
			}
			next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), contextKey{}, claims)))
		})
	}
}

// UnaryServerInterceptor applies the same bearer and tenant policy to gRPC.
func UnaryServerInterceptor(verifier TokenVerifier) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		md, _ := metadata.FromIncomingContext(ctx)
		claims, err := verifyBearer(verifier, md.Get("authorization"))
		if err != nil {
			return nil, status.Error(codes.Unauthenticated, "invalid credentials")
		}
		orgValues := md.Get("x-org-id")
		if len(orgValues) > 1 {
			return nil, status.Error(codes.PermissionDenied, "ambiguous request tenant")
		}
		if len(orgValues) == 1 {
			headerOrg := strings.TrimSpace(orgValues[0])
			if headerOrg != "" && headerOrg != claims.OrgID {
				return nil, status.Error(codes.PermissionDenied, "request tenant does not match verified identity")
			}
		}
		tenantRequest, ok := req.(interface{ GetOrgId() string })
		if !ok {
			return nil, status.Error(codes.PermissionDenied, "request tenant is required")
		}
		if requested := strings.TrimSpace(tenantRequest.GetOrgId()); requested != "" && requested != claims.OrgID {
			return nil, status.Error(codes.PermissionDenied, "request tenant does not match verified identity")
		}
		if !claims.hasScope(grpcRequiredScope(info.FullMethod)) {
			return nil, status.Error(codes.PermissionDenied, "principal lacks required scope")
		}
		return handler(context.WithValue(ctx, contextKey{}, claims), req)
	}
}

func grpcRequiredScope(fullMethod string) string {
	method := fullMethod[strings.LastIndex(fullMethod, "/")+1:]
	if method == "ReviewProposal" {
		return "wiki.approve"
	}
	if strings.HasPrefix(method, "Get") || strings.HasPrefix(method, "List") {
		return "wiki.read"
	}
	return "wiki.write"
}

func verifyBearer(verifier TokenVerifier, values []string) (*Claims, error) {
	if verifier == nil || len(values) != 1 {
		return nil, errors.New("authentication required")
	}
	token := extractBearer(values[0])
	if token == "" {
		return nil, errors.New("authentication required")
	}
	claims, err := verifier.Verify(token)
	if err != nil || claims == nil || !claims.Verified {
		return nil, errors.New("invalid credentials")
	}
	return claims, nil
}

func extractBearer(header string) string {
	parts := strings.Fields(header)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return parts[1]
}

func writeAuthError(w http.ResponseWriter, statusCode int, code, message string) {
	w.Header().Set("Content-Type", "application/json")
	if statusCode == http.StatusUnauthorized {
		w.Header().Set("WWW-Authenticate", "Bearer")
	}
	w.WriteHeader(statusCode)
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
	expiresAt   time.Time
}

func newJWKSCache(url string, client *http.Client) *jwksCache {
	return &jwksCache{url: url, client: client, keys: make(map[string]*rsa.PublicKey)}
}

func (j *jwksCache) keyForKid(kid string) (*rsa.PublicKey, error) {
	j.mu.RLock()
	key := j.keys[kid]
	j.mu.RUnlock()
	if key != nil && time.Now().Before(j.expiresAt) {
		return key, nil
	}

	j.mu.Lock()
	defer j.mu.Unlock()
	if key := j.keys[kid]; key != nil && time.Now().Before(j.expiresAt) {
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
	j.expiresAt = time.Now().Add(jwksCacheTTL)
	return nil
}
