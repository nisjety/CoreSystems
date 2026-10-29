package delegation

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"net/http"
	"regexp"
	"strings"
	"sync"
	"time"
)

const (
	HeaderServiceID      = "x-service-id"
	HeaderUserID         = "x-user-id"
	HeaderOrganizationID = "x-org-id"
	HeaderRole           = "x-user-role"
	HeaderTimestamp      = "x-delegation-timestamp"
	HeaderNonce          = "x-delegation-nonce"
	HeaderBodySHA256     = "x-delegation-body-sha256"
	HeaderSignature      = "x-delegation-signature"
)

const defaultMaxSkew = 2 * time.Minute

var noncePattern = regexp.MustCompile(`^[A-Za-z0-9_-]{16,128}$`)

var errReplay = errors.New("delegation replay detected")

type Principal struct {
	ServiceID      string
	UserID         string
	OrganizationID string
	Role           string
}

type principalContextKey struct{}

func WithPrincipal(ctx context.Context, principal Principal) context.Context {
	return context.WithValue(ctx, principalContextKey{}, principal)
}

func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	if ctx == nil {
		return Principal{}, false
	}
	principal, ok := ctx.Value(principalContextKey{}).(Principal)
	return principal, ok
}

type CanonicalFields struct {
	ServiceID      string
	Audience       string
	Timestamp      string
	Nonce          string
	Method         string
	URI            string
	UserID         string
	OrganizationID string
	Role           string
	BodySHA256     string
}

func Canonical(fields CanonicalFields) string {
	return strings.Join([]string{
		"v2",
		fields.ServiceID,
		fields.Audience,
		fields.Timestamp,
		fields.Nonce,
		fields.Method,
		fields.URI,
		fields.UserID,
		fields.OrganizationID,
		fields.Role,
		fields.BodySHA256,
	}, "\n")
}

type Config struct {
	Audience string
	Keys     map[string]string
	MaxSkew  time.Duration
	Now      func() time.Time
}

type Verifier struct {
	audience string
	keys     map[string][]byte
	maxSkew  time.Duration
	now      func() time.Time
	nonces   *nonceCache
}

func NewVerifier(config Config) (*Verifier, error) {
	audience := strings.TrimSpace(config.Audience)
	if audience == "" {
		return nil, errors.New("delegation audience is required")
	}
	keys := make(map[string][]byte, len(config.Keys))
	for serviceID, secret := range config.Keys {
		serviceID = strings.TrimSpace(serviceID)
		secret = strings.TrimSpace(secret)
		if serviceID == "" || len(secret) < 32 {
			return nil, errors.New("delegation service id and a secret of at least 32 bytes are required")
		}
		keys[serviceID] = []byte(secret)
	}
	if len(keys) == 0 {
		return nil, errors.New("at least one delegation principal is required")
	}
	maxSkew := config.MaxSkew
	if maxSkew <= 0 {
		maxSkew = defaultMaxSkew
	}
	now := config.Now
	if now == nil {
		now = time.Now
	}
	return &Verifier{
		audience: audience,
		keys:     keys,
		maxSkew:  maxSkew,
		now:      now,
		nonces:   newNonceCache(),
	}, nil
}

func (v *Verifier) Verify(request *http.Request, body []byte) (Principal, error) {
	if v == nil || request == nil {
		return Principal{}, errors.New("delegation verifier is not configured")
	}
	serviceID := strings.TrimSpace(request.Header.Get(HeaderServiceID))
	secret, ok := v.keys[serviceID]
	if !ok {
		return Principal{}, errors.New("unknown delegation principal")
	}
	timestampText := strings.TrimSpace(request.Header.Get(HeaderTimestamp))
	timestamp, err := time.Parse(time.RFC3339, timestampText)
	if err != nil {
		return Principal{}, errors.New("invalid delegation timestamp")
	}
	now := v.now().UTC()
	if timestamp.Before(now.Add(-v.maxSkew)) || timestamp.After(now.Add(v.maxSkew)) {
		return Principal{}, errors.New("delegation timestamp is outside allowed skew")
	}
	nonce := strings.TrimSpace(request.Header.Get(HeaderNonce))
	if !noncePattern.MatchString(nonce) {
		return Principal{}, errors.New("invalid delegation nonce")
	}

	digestBytes := sha256.Sum256(body)
	wantDigest := base64.RawURLEncoding.EncodeToString(digestBytes[:])
	providedDigest := strings.TrimSpace(request.Header.Get(HeaderBodySHA256))
	if subtle.ConstantTimeCompare([]byte(providedDigest), []byte(wantDigest)) != 1 {
		return Principal{}, errors.New("delegation body digest mismatch")
	}

	principal := Principal{
		ServiceID:      serviceID,
		UserID:         strings.TrimSpace(request.Header.Get(HeaderUserID)),
		OrganizationID: strings.TrimSpace(request.Header.Get(HeaderOrganizationID)),
		Role:           strings.TrimSpace(request.Header.Get(HeaderRole)),
	}
	canonical := Canonical(CanonicalFields{
		ServiceID:      principal.ServiceID,
		Audience:       v.audience,
		Timestamp:      timestampText,
		Nonce:          nonce,
		Method:         request.Method,
		URI:            request.URL.RequestURI(),
		UserID:         principal.UserID,
		OrganizationID: principal.OrganizationID,
		Role:           principal.Role,
		BodySHA256:     wantDigest,
	})
	mac := hmac.New(sha256.New, secret)
	_, _ = mac.Write([]byte(canonical))
	wantSignature := mac.Sum(nil)
	providedSignature, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(request.Header.Get(HeaderSignature)))
	if err != nil || !hmac.Equal(providedSignature, wantSignature) {
		return Principal{}, errors.New("invalid delegation signature")
	}

	expiresAt := timestamp.Add(v.maxSkew)
	if !v.nonces.use(serviceID, nonce, now, expiresAt) {
		return Principal{}, errReplay
	}
	return principal, nil
}

func IsReplay(err error) bool {
	return errors.Is(err, errReplay)
}

type nonceCache struct {
	mu      sync.Mutex
	expires map[string]time.Time
}

func newNonceCache() *nonceCache {
	return &nonceCache{expires: make(map[string]time.Time)}
}

func (c *nonceCache) use(serviceID, nonce string, now, expiresAt time.Time) bool {
	key := fmt.Sprintf("%s\x00%s", serviceID, nonce)
	c.mu.Lock()
	defer c.mu.Unlock()
	for existingKey, expiry := range c.expires {
		if !expiry.After(now) {
			delete(c.expires, existingKey)
		}
	}
	if _, exists := c.expires[key]; exists {
		return false
	}
	c.expires[key] = expiresAt
	return true
}
