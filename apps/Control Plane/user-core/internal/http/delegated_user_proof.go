package http

import (
	"crypto/rsa"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

type planeUserClaims struct {
	jwt.RegisteredClaims
	UserID        string `json:"user_id"`
	OrgID         string `json:"org_id"`
	PrincipalType string `json:"principal_type"`
}

type verifiedPlaneUser struct {
	UserID string
	OrgID  string
	Expiry time.Time
}

type planeUserVerifier struct {
	key      *rsa.PublicKey
	issuer   string
	audience string
}

func newPlaneUserVerifier(publicPEM []byte, issuer, audience string) (*planeUserVerifier, error) {
	issuer = strings.TrimSpace(issuer)
	audience = strings.TrimSpace(audience)
	if len(publicPEM) == 0 || issuer == "" || audience == "" {
		return nil, errors.New("delegated user proof verification is not configured")
	}
	key, err := jwt.ParseRSAPublicKeyFromPEM(publicPEM)
	if err != nil {
		return nil, fmt.Errorf("parse delegated user proof key: %w", err)
	}
	return &planeUserVerifier{key: key, issuer: issuer, audience: audience}, nil
}

func planeUserVerifierFromEnv() (*planeUserVerifier, error) {
	path := strings.TrimSpace(os.Getenv("AUTH_CORE_JWT_PUBLIC_KEY_FILE"))
	if path == "" {
		return nil, errors.New("AUTH_CORE_JWT_PUBLIC_KEY_FILE is required")
	}
	publicPEM, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read delegated user proof key: %w", err)
	}
	issuer := strings.TrimSpace(os.Getenv("AUTH_CORE_ISSUER"))
	if issuer == "" {
		return nil, errors.New("AUTH_CORE_ISSUER is required")
	}
	audience := strings.TrimSpace(os.Getenv("DATA_PLANE_AUTH_AUDIENCE"))
	if audience == "" {
		audience = "data-plane"
	}
	return newPlaneUserVerifier(publicPEM, issuer, audience)
}

func (v *planeUserVerifier) VerifyAuthorization(authorization string) (verifiedPlaneUser, error) {
	if v == nil {
		return verifiedPlaneUser{}, errors.New("delegated user proof verifier unavailable")
	}
	const prefix = "Bearer "
	if !strings.HasPrefix(authorization, prefix) || strings.ContainsAny(authorization, "\r\n") {
		return verifiedPlaneUser{}, errors.New("valid delegated user bearer required")
	}
	raw := strings.TrimPrefix(authorization, prefix)
	if raw == "" || strings.Contains(raw, " ") {
		return verifiedPlaneUser{}, errors.New("valid delegated user bearer required")
	}
	claims := &planeUserClaims{}
	parsed, err := jwt.ParseWithClaims(raw, claims, func(token *jwt.Token) (any, error) {
		if token.Method != jwt.SigningMethodRS256 {
			return nil, errors.New("delegated user proof requires RS256")
		}
		return v.key, nil
	}, jwt.WithValidMethods([]string{"RS256"}), jwt.WithIssuer(v.issuer), jwt.WithAudience(v.audience), jwt.WithExpirationRequired(), jwt.WithIssuedAt(), jwt.WithLeeway(5*time.Second))
	if err != nil || !parsed.Valid || claims.IssuedAt == nil || claims.NotBefore == nil || claims.ExpiresAt == nil {
		return verifiedPlaneUser{}, errors.New("delegated user proof verification failed")
	}
	if claims.PrincipalType != "user" || strings.TrimSpace(claims.Subject) == "" || claims.UserID != claims.Subject || strings.TrimSpace(claims.OrgID) == "" {
		return verifiedPlaneUser{}, errors.New("delegated user proof claims are invalid")
	}
	return verifiedPlaneUser{UserID: claims.UserID, OrgID: claims.OrgID, Expiry: claims.ExpiresAt.Time}, nil
}

type delegationNonceCache struct {
	mu       sync.Mutex
	capacity int
	entries  map[string]time.Time
}

func newDelegationNonceCache(capacity int) *delegationNonceCache {
	return &delegationNonceCache{capacity: capacity, entries: make(map[string]time.Time)}
}

func (c *delegationNonceCache) Consume(key string, expiry, now time.Time) bool {
	if c == nil || strings.TrimSpace(key) == "" || !expiry.After(now) {
		return false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	for entry, expiresAt := range c.entries {
		if !expiresAt.After(now) {
			delete(c.entries, entry)
		}
	}
	if _, exists := c.entries[key]; exists || len(c.entries) >= c.capacity {
		return false
	}
	c.entries[key] = expiry
	return true
}
