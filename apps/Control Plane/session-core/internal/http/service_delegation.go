package http

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	sessionDelegationMaxAge         = 30 * time.Second
	sessionDelegationFutureSkew     = 5 * time.Second
	sessionDelegationMaxBody        = 1 << 20
	sessionDelegationNonceMinLength = 16
	sessionDelegationNonceMaxLength = 128
)

type sessionDelegationClaims struct {
	Principal  string
	Audience   string
	Timestamp  string
	Nonce      string
	Method     string
	URI        string
	UserID     string
	OrgID      string
	Email      string
	Name       string
	Avatar     string
	BodySHA256 string
}

func sessionDelegationBodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func sessionDelegationSignature(token string, claims sessionDelegationClaims) string {
	canonical := strings.Join([]string{
		"v2",
		claims.Principal,
		claims.Audience,
		claims.Timestamp,
		claims.Nonce,
		claims.Method,
		claims.URI,
		claims.UserID,
		claims.OrgID,
		claims.Email,
		claims.Name,
		claims.Avatar,
		claims.BodySHA256,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(token))
	_, _ = mac.Write([]byte(canonical))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func verifySessionServiceDelegation(request *http.Request, credential serviceCredential, nonces *sessionDelegationNonceCache, now time.Time) (sessionDelegationClaims, bool) {
	timestampValue := strings.TrimSpace(request.Header.Get("X-Delegation-Timestamp"))
	timestamp, err := time.Parse(time.RFC3339, timestampValue)
	if err != nil {
		return sessionDelegationClaims{}, false
	}
	age := now.UTC().Sub(timestamp.UTC())
	if age > sessionDelegationMaxAge || age < -sessionDelegationFutureSkew {
		return sessionDelegationClaims{}, false
	}

	nonce := strings.TrimSpace(request.Header.Get("X-Delegation-Nonce"))
	if len(nonce) < sessionDelegationNonceMinLength || len(nonce) > sessionDelegationNonceMaxLength {
		return sessionDelegationClaims{}, false
	}

	body, ok := readSessionDelegationBody(request)
	if !ok {
		return sessionDelegationClaims{}, false
	}
	bodyDigest := sessionDelegationBodyDigest(body)
	if !constantTimeEncodedEqual(bodyDigest, request.Header.Get("X-Delegation-Body-SHA256")) {
		return sessionDelegationClaims{}, false
	}

	claims := sessionDelegationClaims{
		Principal:  credential.Principal,
		Audience:   credential.Audience,
		Timestamp:  timestampValue,
		Nonce:      nonce,
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     strings.TrimSpace(request.Header.Get("X-User-Id")),
		OrgID:      strings.TrimSpace(request.Header.Get("X-Org-Id")),
		Email:      strings.ToLower(strings.TrimSpace(request.Header.Get("X-User-Email"))),
		Name:       strings.TrimSpace(request.Header.Get("X-User-Name")),
		Avatar:     strings.TrimSpace(request.Header.Get("X-User-Avatar")),
		BodySHA256: bodyDigest,
	}
	if claims.UserID == "" {
		return sessionDelegationClaims{}, false
	}
	if !constantTimeEncodedEqual(sessionDelegationSignature(credential.Token, claims), request.Header.Get("X-Delegation-Signature")) {
		return sessionDelegationClaims{}, false
	}
	// Consume the nonce only after the signature verifies, so a forged request
	// cannot burn a legitimate caller's nonce. The cache entry outlives the
	// signed timestamp only until that timestamp would fail the freshness
	// check anyway, bounding memory to the replay window.
	if !nonces.consume(credential.Principal+":"+claims.Nonce, timestamp.UTC().Add(sessionDelegationMaxAge), now.UTC()) {
		return sessionDelegationClaims{}, false
	}
	return claims, true
}

func readSessionDelegationBody(request *http.Request) ([]byte, bool) {
	if request.Body == nil || request.Body == http.NoBody {
		return nil, true
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, sessionDelegationMaxBody+1))
	if err != nil || len(body) > sessionDelegationMaxBody {
		return nil, false
	}
	request.Body = io.NopCloser(strings.NewReader(string(body)))
	return body, true
}

func constantTimeEncodedEqual(expected, provided string) bool {
	expectedBytes, expectedErr := base64.RawURLEncoding.DecodeString(strings.TrimSpace(expected))
	providedBytes, providedErr := base64.RawURLEncoding.DecodeString(strings.TrimSpace(provided))
	return expectedErr == nil && providedErr == nil && hmac.Equal(expectedBytes, providedBytes)
}

// sessionDelegationNonceCache tracks consumed delegation nonces so a captured,
// still-fresh signed request cannot be replayed within its validity window.
// Entries self-expire at the signed timestamp's own expiry, since a replay
// after that point already fails the timestamp freshness check.
type sessionDelegationNonceCache struct {
	mu       sync.Mutex
	capacity int
	entries  map[string]time.Time
}

func newSessionDelegationNonceCache(capacity int) *sessionDelegationNonceCache {
	return &sessionDelegationNonceCache{capacity: capacity, entries: make(map[string]time.Time)}
}

func (cache *sessionDelegationNonceCache) consume(key string, expiry, now time.Time) bool {
	if cache == nil || strings.TrimSpace(key) == "" || !expiry.After(now) {
		return false
	}
	cache.mu.Lock()
	defer cache.mu.Unlock()
	for entry, expiresAt := range cache.entries {
		if !expiresAt.After(now) {
			delete(cache.entries, entry)
		}
	}
	if _, exists := cache.entries[key]; exists || len(cache.entries) >= cache.capacity {
		return false
	}
	cache.entries[key] = expiry
	return true
}
