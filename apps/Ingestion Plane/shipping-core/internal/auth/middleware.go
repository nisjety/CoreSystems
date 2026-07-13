// Package auth verifies Control Plane-issued JWTs and derives immutable
// organization and actor identity for shipping-core requests.
package auth

import (
	"context"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"net/http"
	"net/url"
	"os"
	"slices"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const maxJWKSBytes = 1 << 20

type contextKey struct{}

// Principal is identity proven by an Auth Core signature. Request headers and
// bodies are never used to construct it.
type Principal struct {
	OrganizationID string
	ActorID        string
	PrincipalType  string
	Scopes         []string
}

// Config describes the Auth Core trust boundary. PublicKeyPEM is primarily for
// tests and offline deployments; production normally uses JWKSURL.
type Config struct {
	Audience     string
	Issuer       string
	JWKSURL      string
	PublicKeyPEM []byte
	HTTPClient   *http.Client
}

// ConfigFromEnv returns the production Auth Core contract. The URL defaults
// match the compose service name but can be overridden for other deployments.
func ConfigFromEnv() (Config, error) {
	config := Config{
		Audience: strings.TrimSpace(os.Getenv("INGESTION_AUTH_AUDIENCE")),
		Issuer:   strings.TrimSpace(os.Getenv("PLANE_TOKEN_ISSUER")),
		JWKSURL:  strings.TrimSpace(os.Getenv("AUTH_CORE_JWKS_URL")),
	}
	if config.Audience == "" {
		config.Audience = "ingestion"
	}
	if config.Issuer == "" {
		config.Issuer = "http://auth-service:3011/api/convex-auth"
	}
	if config.JWKSURL == "" {
		config.JWKSURL = "http://auth-service:3011/api/convex-auth/jwks"
	}
	if path := strings.TrimSpace(os.Getenv("JWT_PUBLIC_KEY_FILE")); path != "" {
		publicKey, err := os.ReadFile(path)
		if err != nil {
			return Config{}, fmt.Errorf("read JWT_PUBLIC_KEY_FILE: %w", err)
		}
		config.PublicKeyPEM = publicKey
	}
	return config, nil
}

type verifier struct {
	audience string
	issuer   string
	static   *rsa.PublicKey
	keys     map[string]*rsa.PublicKey
}

type claims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes"`
	jwt.RegisteredClaims
}

// NewMiddleware validates verification material eagerly. Shipping does not
// start with an authentication configuration it cannot actually use.
func NewMiddleware(config Config) (func(http.Handler) http.Handler, error) {
	v, err := newVerifier(config)
	if err != nil {
		return nil, err
	}
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := bearerToken(r.Header.Get("Authorization"))
			if token == "" {
				writeAuthError(w, http.StatusUnauthorized, "authentication required")
				return
			}
			principal, err := v.verify(token)
			if err != nil {
				writeAuthError(w, http.StatusUnauthorized, "invalid authentication token")
				return
			}
			if conflicts(r.Header.Get("X-Org-ID"), principal.OrganizationID) ||
				conflicts(r.Header.Get("X-User-ID"), principal.ActorID) {
				writeAuthError(w, http.StatusForbidden, "identity context does not match verified token")
				return
			}
			if principal.PrincipalType == "service" && !serviceScopeAllows(principal.Scopes, r.Method) {
				writeAuthError(w, http.StatusForbidden, "service principal lacks shipping scope")
				return
			}

			r.Header.Set("X-Org-ID", principal.OrganizationID)
			r.Header.Set("X-User-ID", principal.ActorID)
			ctx := context.WithValue(r.Context(), contextKey{}, principal)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}, nil
}

func newVerifier(config Config) (*verifier, error) {
	audience := strings.TrimSpace(config.Audience)
	issuer := strings.TrimSpace(config.Issuer)
	if audience == "" || issuer == "" {
		return nil, errors.New("shipping auth: audience and issuer are required")
	}
	v := &verifier{audience: audience, issuer: issuer, keys: map[string]*rsa.PublicKey{}}
	if len(config.PublicKeyPEM) > 0 {
		key, err := jwt.ParseRSAPublicKeyFromPEM(config.PublicKeyPEM)
		if err != nil {
			return nil, fmt.Errorf("shipping auth: parse public key: %w", err)
		}
		v.static = key
	}
	if strings.TrimSpace(config.JWKSURL) != "" {
		keys, err := loadJWKS(config)
		if err != nil {
			return nil, err
		}
		v.keys = keys
	}
	if v.static == nil && len(v.keys) == 0 {
		return nil, errors.New("shipping auth: public key or JWKS URL is required")
	}
	return v, nil
}

func (v *verifier) verify(raw string) (Principal, error) {
	parsedClaims := &claims{}
	parsed, err := jwt.ParseWithClaims(raw, parsedClaims, func(token *jwt.Token) (any, error) {
		if token.Method.Alg() != jwt.SigningMethodRS256.Alg() {
			return nil, fmt.Errorf("unexpected signing algorithm %q", token.Method.Alg())
		}
		if keyID, _ := token.Header["kid"].(string); keyID != "" {
			if key := v.keys[keyID]; key != nil {
				return key, nil
			}
		}
		if v.static != nil {
			return v.static, nil
		}
		return nil, errors.New("signing key not found")
	},
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithAudience(v.audience),
		jwt.WithIssuer(v.issuer),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
		jwt.WithLeeway(30*time.Second),
	)
	if err != nil || !parsed.Valid {
		return Principal{}, errors.New("token verification failed")
	}
	if parsedClaims.IssuedAt == nil || parsedClaims.NotBefore == nil || strings.TrimSpace(parsedClaims.OrgID) == "" {
		return Principal{}, errors.New("token missing required identity or time claims")
	}

	principal := Principal{
		OrganizationID: strings.TrimSpace(parsedClaims.OrgID),
		PrincipalType:  strings.TrimSpace(parsedClaims.PrincipalType),
		Scopes:         append([]string(nil), parsedClaims.Scopes...),
	}
	subject := strings.TrimSpace(parsedClaims.Subject)
	switch principal.PrincipalType {
	case "user":
		principal.ActorID = strings.TrimSpace(parsedClaims.UserID)
		if principal.ActorID == "" || parsedClaims.ServiceID != "" || subject != principal.ActorID {
			return Principal{}, errors.New("ambiguous user identity")
		}
	case "service":
		principal.ActorID = strings.TrimSpace(parsedClaims.ServiceID)
		if principal.ActorID == "" || parsedClaims.UserID != "" || subject != principal.ActorID || len(principal.Scopes) == 0 {
			return Principal{}, errors.New("ambiguous service identity")
		}
	default:
		return Principal{}, errors.New("unsupported principal type")
	}
	return principal, nil
}

// PrincipalFromContext returns only cryptographically verified identity.
func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	principal, ok := ctx.Value(contextKey{}).(Principal)
	return principal, ok && principal.OrganizationID != "" && principal.ActorID != ""
}

func bearerToken(header string) string {
	parts := strings.Fields(header)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}

func conflicts(unverified, verified string) bool {
	unverified = strings.TrimSpace(unverified)
	return unverified != "" && unverified != verified
}

func serviceScopeAllows(scopes []string, method string) bool {
	if method == http.MethodGet || method == http.MethodHead || method == http.MethodOptions {
		return slices.Contains(scopes, "shipping:read") || slices.Contains(scopes, "shipping:write")
	}
	return slices.Contains(scopes, "shipping:write")
}

func writeAuthError(w http.ResponseWriter, status int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]any{"code": "AUTHORIZATION_FAILED", "message": message},
	})
}

type jwksDocument struct {
	Keys []struct {
		KeyID string `json:"kid"`
		Type  string `json:"kty"`
		Use   string `json:"use"`
		Alg   string `json:"alg"`
		N     string `json:"n"`
		E     string `json:"e"`
	} `json:"keys"`
}

func loadJWKS(config Config) (map[string]*rsa.PublicKey, error) {
	parsed, err := url.ParseRequestURI(strings.TrimSpace(config.JWKSURL))
	if err != nil || parsed.Host == "" || parsed.User != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") {
		return nil, errors.New("shipping auth: JWKS URL must be an http(s) URL without user info")
	}
	client := config.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	request, err := http.NewRequestWithContext(context.Background(), http.MethodGet, parsed.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("shipping auth: build JWKS request: %w", err)
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, fmt.Errorf("shipping auth: fetch JWKS: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("shipping auth: JWKS returned HTTP %d", response.StatusCode)
	}
	var document jwksDocument
	decoder := json.NewDecoder(io.LimitReader(response.Body, maxJWKSBytes))
	if err := decoder.Decode(&document); err != nil {
		return nil, fmt.Errorf("shipping auth: decode JWKS: %w", err)
	}
	keys := make(map[string]*rsa.PublicKey, len(document.Keys))
	for _, item := range document.Keys {
		if item.KeyID == "" || item.Type != "RSA" || item.Alg != "RS256" || (item.Use != "" && item.Use != "sig") {
			continue
		}
		n, nErr := base64.RawURLEncoding.DecodeString(item.N)
		e, eErr := base64.RawURLEncoding.DecodeString(item.E)
		if nErr != nil || eErr != nil || len(n) == 0 || len(e) == 0 {
			continue
		}
		exponent := int(new(big.Int).SetBytes(e).Int64())
		if exponent < 3 {
			continue
		}
		keys[item.KeyID] = &rsa.PublicKey{N: new(big.Int).SetBytes(n), E: exponent}
	}
	if len(keys) == 0 {
		return nil, errors.New("shipping auth: JWKS contains no usable RS256 keys")
	}
	return keys, nil
}
