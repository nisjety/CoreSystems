package http

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
)

const (
	serviceCredentialAudience = "billing-core"
	serviceCredentialEnv      = "BILLING_CORE_SERVICE_CREDENTIALS"
	gatewayServicePrincipal   = "verevon-gateway"
	serviceDelegationMaxAge   = 30 * time.Second
	serviceDelegationSkew     = 5 * time.Second
	serviceDelegationMaxBody  = 1 << 20
)

var billingServiceScopeAllowlist = map[string]struct{}{
	"billing:account:read:self":           {},
	"billing:account:read:any":            {},
	"billing:account:write:any":           {},
	"billing:usage:write:any":             {},
	"billing:entitlement:read:self":       {},
	"billing:entitlement:read:any":        {},
	"billing:quota:read:self":             {},
	"billing:quota:read:any":              {},
	"billing:invoice:create:any":          {},
	"billing:checkout:create:self":        {},
	"billing:checkout:confirm:self":       {},
	"billing:organization:deactivate:any": {},
}

type serviceCredential struct {
	Principal string   `json:"principal"`
	Audience  string   `json:"audience"`
	Token     string   `json:"token"`
	Scopes    []string `json:"scopes"`
}

func parseServiceCredentials(raw string) ([]serviceCredential, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, fmt.Errorf("%s is required", serviceCredentialEnv)
	}
	if len(raw) > 256<<10 {
		return nil, fmt.Errorf("%s exceeds maximum size", serviceCredentialEnv)
	}
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.DisallowUnknownFields()
	var credentials []serviceCredential
	if err := decoder.Decode(&credentials); err != nil {
		return nil, fmt.Errorf("decode %s: %w", serviceCredentialEnv, err)
	}
	if err := ensureJSONEOF(decoder); err != nil {
		return nil, fmt.Errorf("decode %s: %w", serviceCredentialEnv, err)
	}
	if len(credentials) == 0 || len(credentials) > 128 {
		return nil, fmt.Errorf("%s must contain between 1 and 128 credentials", serviceCredentialEnv)
	}

	principals := make(map[string]struct{}, len(credentials))
	tokens := make(map[[sha256.Size]byte]struct{}, len(credentials))
	validated := make([]serviceCredential, 0, len(credentials))
	for _, credential := range credentials {
		if err := validateServiceCredential(credential, principals, tokens); err != nil {
			return nil, err
		}
		validated = append(validated, serviceCredential{
			Principal: credential.Principal,
			Audience:  credential.Audience,
			Token:     credential.Token,
			Scopes:    append([]string(nil), credential.Scopes...),
		})
	}
	return validated, nil
}

func ensureJSONEOF(decoder *json.Decoder) error {
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		if err == nil {
			return errors.New("multiple JSON values are not allowed")
		}
		return err
	}
	return nil
}

func validateServiceCredential(credential serviceCredential, principals map[string]struct{}, tokens map[[sha256.Size]byte]struct{}) error {
	if credential.Principal != strings.TrimSpace(credential.Principal) || !validPrincipal(credential.Principal) {
		return fmt.Errorf("invalid %s principal", serviceCredentialEnv)
	}
	if credential.Audience != serviceCredentialAudience {
		return fmt.Errorf("principal %s has invalid audience", credential.Principal)
	}
	if credential.Token != strings.TrimSpace(credential.Token) || !secureServiceCredentialToken(credential.Token) {
		return fmt.Errorf("principal %s has invalid token length", credential.Principal)
	}
	if len(credential.Scopes) == 0 || len(credential.Scopes) > 32 {
		return fmt.Errorf("principal %s must have between 1 and 32 scopes", credential.Principal)
	}
	if _, exists := principals[credential.Principal]; exists {
		return fmt.Errorf("duplicate service principal %s", credential.Principal)
	}
	tokenDigest := sha256.Sum256([]byte(credential.Token))
	if _, exists := tokens[tokenDigest]; exists {
		return fmt.Errorf("duplicate service token")
	}

	scopes := make(map[string]struct{}, len(credential.Scopes))
	for _, scope := range credential.Scopes {
		if scope != strings.TrimSpace(scope) {
			return fmt.Errorf("principal %s has malformed scope", credential.Principal)
		}
		if _, allowed := billingServiceScopeAllowlist[scope]; !allowed {
			return fmt.Errorf("principal %s has unknown scope %s", credential.Principal, scope)
		}
		if _, duplicate := scopes[scope]; duplicate {
			return fmt.Errorf("principal %s has duplicate scope %s", credential.Principal, scope)
		}
		if strings.HasSuffix(scope, ":self") && credential.Principal != gatewayServicePrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		if strings.HasSuffix(scope, ":any") && credential.Principal == gatewayServicePrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		scopes[scope] = struct{}{}
	}
	principals[credential.Principal] = struct{}{}
	tokens[tokenDigest] = struct{}{}
	return nil
}

func secureServiceCredentialToken(token string) bool {
	if len(token) < 32 || len(token) > 512 {
		return false
	}
	lower := strings.ToLower(token)
	for _, prefix := range []string{"test", "placeholder", "change-me", "replace-with"} {
		if strings.HasPrefix(lower, prefix) {
			return false
		}
	}
	return true
}

func validPrincipal(value string) bool {
	if len(value) < 1 || len(value) > 128 {
		return false
	}
	for _, character := range value {
		if (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
			(character >= '0' && character <= '9') || strings.ContainsRune("._:-", character) {
			continue
		}
		return false
	}
	return true
}

// ValidateRequiredServiceCredentialRegistry verifies the minimum Verevon v3
// billing policy before the process opens a database connection or listener.
func ValidateRequiredServiceCredentialRegistry(raw string) error {
	credentials, err := parseServiceCredentials(raw)
	if err != nil {
		return err
	}
	required := map[string]bool{
		"billing:account:read:self":     false,
		"billing:entitlement:read:self": false,
		"billing:quota:read:self":       false,
		"billing:checkout:create:self":  false,
		"billing:checkout:confirm:self": false,
	}
	gatewayCount := 0
	authHasDeactivateScope := false
	authCount := 0
	for _, credential := range credentials {
		switch credential.Principal {
		case gatewayServicePrincipal:
			gatewayCount++
			for _, scope := range credential.Scopes {
				if _, exists := required[scope]; exists {
					required[scope] = true
				}
			}
		case "auth-core":
			authCount++
			for _, scope := range credential.Scopes {
				if scope == "billing:organization:deactivate:any" {
					authHasDeactivateScope = true
				}
			}
		}
	}
	if gatewayCount != 1 {
		return fmt.Errorf("%s must contain exactly one %s principal", serviceCredentialEnv, gatewayServicePrincipal)
	}
	for scope, present := range required {
		if !present {
			return fmt.Errorf("%s principal is missing required %s scope", gatewayServicePrincipal, scope)
		}
	}
	if authCount != 1 {
		return fmt.Errorf("%s must contain exactly one auth-core principal", serviceCredentialEnv)
	}
	if !authHasDeactivateScope {
		return fmt.Errorf("auth-core principal is missing required billing:organization:deactivate:any scope")
	}
	return nil
}

func secureServiceValueEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return subtle.ConstantTimeCompare(expectedDigest[:], receivedDigest[:]) == 1
}

func serviceScopesForRequest(request *http.Request) []string {
	segments, valid := cleanPathSegments(request.URL.Path)
	if !valid || len(segments) < 6 || segments[0] != "api" || segments[1] != "v1" ||
		segments[2] != "billing" || segments[3] != "orgs" || strings.TrimSpace(segments[4]) == "" {
		return nil
	}
	rest := segments[5:]
	switch {
	case len(rest) == 1 && rest[0] == "account" && request.Method == http.MethodGet:
		return []string{"billing:account:read:self", "billing:account:read:any"}
	case len(rest) == 1 && rest[0] == "account" && request.Method == http.MethodPut:
		return []string{"billing:account:write:any"}
	case len(rest) == 1 && rest[0] == "usage" && request.Method == http.MethodPost:
		return []string{"billing:usage:write:any"}
	case len(rest) == 2 && rest[0] == "entitlements" && request.Method == http.MethodGet:
		return []string{"billing:entitlement:read:self", "billing:entitlement:read:any"}
	case len(rest) == 2 && rest[0] == "quotas" && request.Method == http.MethodGet:
		return []string{"billing:quota:read:self", "billing:quota:read:any"}
	case len(rest) == 1 && rest[0] == "invoices" && request.Method == http.MethodPost:
		return []string{"billing:invoice:create:any"}
	case len(rest) == 1 && rest[0] == "checkout-session" && request.Method == http.MethodPost:
		return []string{"billing:checkout:create:self"}
	case len(rest) == 2 && rest[0] == "checkout-session" && rest[1] == "confirm" && request.Method == http.MethodPost:
		return []string{"billing:checkout:confirm:self"}
	case len(rest) == 1 && rest[0] == "deactivate" && request.Method == http.MethodPost:
		return []string{"billing:organization:deactivate:any"}
	default:
		return nil
	}
}

func cleanPathSegments(path string) ([]string, bool) {
	if path == "" || path == "/" || strings.Contains(path, "//") || strings.HasSuffix(path, "/") {
		return nil, false
	}
	return strings.Split(strings.TrimPrefix(path, "/"), "/"), true
}

func credentialScopeForRequest(credential serviceCredential, request *http.Request) string {
	for _, required := range serviceScopesForRequest(request) {
		for _, granted := range credential.Scopes {
			if granted == required {
				return required
			}
		}
	}
	return ""
}

type serviceDelegationClaims struct {
	Principal  string
	Audience   string
	Timestamp  string
	Nonce      string
	Method     string
	URI        string
	UserID     string
	OrgID      string
	UserRole   string
	BodySHA256 string
}

func serviceDelegationBodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func serviceDelegationSignature(token string, claims serviceDelegationClaims) string {
	canonical := strings.Join([]string{
		"v3",
		claims.Principal,
		claims.Audience,
		claims.Timestamp,
		claims.Nonce,
		claims.Method,
		claims.URI,
		claims.UserID,
		claims.OrgID,
		claims.UserRole,
		claims.BodySHA256,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(token))
	_, _ = mac.Write([]byte(canonical))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func verifyServiceDelegation(request *http.Request, credential serviceCredential, now time.Time) (serviceDelegationClaims, bool) {
	version, ok := singleTrimmedHeader(request, "X-Delegation-Version")
	if !ok || version != "v3" {
		return serviceDelegationClaims{}, false
	}
	timestampValue, ok := singleTrimmedHeader(request, "X-Delegation-Timestamp")
	if !ok || strings.Contains(timestampValue, ".") {
		return serviceDelegationClaims{}, false
	}
	timestamp, err := time.Parse(time.RFC3339, timestampValue)
	if err != nil {
		return serviceDelegationClaims{}, false
	}
	age := now.UTC().Sub(timestamp.UTC())
	if age > serviceDelegationMaxAge || age < -serviceDelegationSkew {
		return serviceDelegationClaims{}, false
	}
	claims := serviceDelegationClaims{
		Principal: credential.Principal,
		Audience:  credential.Audience,
		Timestamp: timestampValue,
		Method:    request.Method,
		URI:       request.URL.RequestURI(),
	}
	if claims.Nonce, ok = singleTrimmedHeader(request, "X-Delegation-Nonce"); !ok || len(claims.Nonce) < 16 || len(claims.Nonce) > 128 {
		return serviceDelegationClaims{}, false
	}
	if claims.UserID, ok = singleTrimmedHeader(request, "X-User-Id"); !ok || len(claims.UserID) > 128 {
		return serviceDelegationClaims{}, false
	}
	if claims.OrgID, ok = singleTrimmedHeader(request, "X-Org-Id"); !ok || len(claims.OrgID) > 128 || claims.OrgID != billingOrganizationIDFromPath(request.URL.Path) {
		return serviceDelegationClaims{}, false
	}
	if claims.UserRole, ok = singleTrimmedHeader(request, "X-User-Role"); !ok || len(claims.UserRole) > 64 {
		return serviceDelegationClaims{}, false
	}
	body, ok := delegationRequestBody(request)
	if !ok {
		return serviceDelegationClaims{}, false
	}
	claims.BodySHA256 = serviceDelegationBodyDigest(body)
	providedDigest, ok := singleTrimmedHeader(request, "X-Delegation-Body-SHA256")
	if !ok || !secureServiceValueEqual(claims.BodySHA256, providedDigest) {
		return serviceDelegationClaims{}, false
	}
	providedSignature, ok := singleTrimmedHeader(request, "X-Delegation-Signature")
	if !ok {
		return serviceDelegationClaims{}, false
	}
	providedMAC, err := base64.RawURLEncoding.DecodeString(providedSignature)
	if err != nil || len(providedMAC) != sha256.Size {
		return serviceDelegationClaims{}, false
	}
	expectedMAC, err := base64.RawURLEncoding.DecodeString(serviceDelegationSignature(credential.Token, claims))
	if err != nil || !hmac.Equal(expectedMAC, providedMAC) {
		return serviceDelegationClaims{}, false
	}
	return claims, true
}

func singleTrimmedHeader(request *http.Request, name string) (string, bool) {
	values := request.Header.Values(name)
	if len(values) != 1 || values[0] != strings.TrimSpace(values[0]) || values[0] == "" {
		return "", false
	}
	return values[0], true
}

func delegationRequestBody(request *http.Request) ([]byte, bool) {
	if request.Body == nil || request.Body == http.NoBody {
		return nil, true
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, serviceDelegationMaxBody+1))
	if err != nil || len(body) > serviceDelegationMaxBody {
		return nil, false
	}
	request.Body = io.NopCloser(bytes.NewReader(body))
	return body, true
}

func billingOrganizationIDFromPath(path string) string {
	segments, valid := cleanPathSegments(path)
	if !valid || len(segments) < 5 || segments[0] != "api" || segments[1] != "v1" || segments[2] != "billing" || segments[3] != "orgs" {
		return ""
	}
	return segments[4]
}

type delegationNonceCache struct {
	mu       sync.Mutex
	capacity int
	entries  map[string]time.Time
}

func newDelegationNonceCache(capacity int) *delegationNonceCache {
	return &delegationNonceCache{capacity: capacity, entries: make(map[string]time.Time)}
}

func (cache *delegationNonceCache) consume(key string, expiry, now time.Time) bool {
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

func serviceAuthMiddleware(credentials []serviceCredential, now func() time.Time) gin.HandlerFunc {
	nonces := newDelegationNonceCache(100_000)
	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.URL.Path == nexiWebhookPath || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}
		if len(credentials) == 0 {
			c.AbortWithStatusJSON(http.StatusServiceUnavailable, gin.H{"error": "service authentication unavailable"})
			return
		}
		principal, principalOK := singleTrimmedHeader(c.Request, "X-Service-Id")
		token, tokenOK := singleTrimmedHeader(c.Request, "X-Service-Token")
		if !principalOK || !tokenOK {
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "service authentication required"})
			return
		}

		var authenticated *serviceCredential
		for index := range credentials {
			credential := &credentials[index]
			if credential.Principal == principal && secureServiceValueEqual(credential.Token, token) {
				authenticated = credential
				break
			}
		}
		if authenticated == nil {
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "invalid service credential"})
			return
		}
		scope := credentialScopeForRequest(*authenticated, c.Request)
		if scope == "" {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "service scope denied"})
			return
		}
		if strings.HasSuffix(scope, ":self") {
			requestTime := now().UTC()
			claims, valid := verifyServiceDelegation(c.Request, *authenticated, requestTime)
			if !valid || !nonces.consume(authenticated.Principal+":"+claims.Nonce, claimsTimestampExpiry(claims), requestTime) {
				c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified delegation required"})
				return
			}
			pinDelegatedIdentity(c.Request, claims)
			c.Set("delegation_verified", true)
			c.Set("delegation_nonce", claims.Nonce)
		} else {
			clearCallerIdentityHeaders(c.Request)
		}
		c.Set("auth_method", "service_principal")
		c.Set("service_id", authenticated.Principal)
		c.Set("service_scope", scope)
		c.Next()
	}
}

func claimsTimestampExpiry(claims serviceDelegationClaims) time.Time {
	timestamp, err := time.Parse(time.RFC3339, claims.Timestamp)
	if err != nil {
		return time.Time{}
	}
	return timestamp.UTC().Add(serviceDelegationMaxAge)
}

func pinDelegatedIdentity(request *http.Request, claims serviceDelegationClaims) {
	clearCallerIdentityHeaders(request)
	request.Header.Set("X-User-Id", claims.UserID)
	request.Header.Set("X-Org-Id", claims.OrgID)
	request.Header.Set("X-User-Role", claims.UserRole)
}

func clearCallerIdentityHeaders(request *http.Request) {
	request.Header.Del("X-User-Id")
	request.Header.Del("X-Org-Id")
	request.Header.Del("X-User-Role")
	request.Header.Del("X-User-Roles")
}
