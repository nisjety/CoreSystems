package api

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"strings"
	"sync"
	"time"
)

const (
	auditServiceAudience      = "audit-core"
	delegationMaxAge          = 30 * time.Second
	delegationFutureSkew      = 5 * time.Second
	delegationMaxBody         = 1 << 20
	maxServiceCredentialCount = 100
	delegationNonceMinLength  = 16
	delegationNonceMaxLength  = 128
)

var (
	serviceIdentityPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$`)
	producerIdentityPattern    = regexp.MustCompile(`^[a-z0-9][a-z0-9_-]{0,63}$`)
	delegationTimestampPattern = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:\d{2})$`)
)

type serviceCredential struct {
	Principal string   `json:"principal"`
	Audience  string   `json:"audience"`
	Token     string   `json:"token"`
	Scopes    []string `json:"scopes"`
	Planes    []string `json:"planes,omitempty"`
}

type serviceAuthorization struct {
	Principal string
	Scope     string
	UserID    string
	OrgID     string
	UserRole  string
	planes    map[string]struct{}
}

func (a serviceAuthorization) AllowsPlane(plane string) bool {
	_, ok := a.planes[strings.TrimSpace(plane)]
	return ok
}

type authFailure struct {
	Status  int
	Message string
}

func (e *authFailure) Error() string { return e.Message }

func parseServiceCredentialRegistry(raw string) ([]serviceCredential, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, fmt.Errorf("AUDIT_CORE_SERVICE_CREDENTIALS is required")
	}
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.DisallowUnknownFields()
	var credentials []serviceCredential
	if err := decoder.Decode(&credentials); err != nil {
		return nil, fmt.Errorf("decode AUDIT_CORE_SERVICE_CREDENTIALS: %w", err)
	}
	if err := ensureJSONEOF(decoder); err != nil {
		return nil, err
	}
	if len(credentials) == 0 || len(credentials) > maxServiceCredentialCount {
		return nil, fmt.Errorf("AUDIT_CORE_SERVICE_CREDENTIALS must contain 1..%d entries", maxServiceCredentialCount)
	}

	seenPrincipals := make(map[string]struct{}, len(credentials))
	seenTokens := make(map[[sha256.Size]byte]struct{}, len(credentials))
	validated := make([]serviceCredential, 0, len(credentials))
	for _, candidate := range credentials {
		credential := serviceCredential{
			Principal: strings.TrimSpace(candidate.Principal),
			Audience:  strings.TrimSpace(candidate.Audience),
			Token:     strings.TrimSpace(candidate.Token),
			Scopes:    normalizedUnique(candidate.Scopes),
			Planes:    normalizedUnique(candidate.Planes),
		}
		if !serviceIdentityPattern.MatchString(credential.Principal) || credential.Audience != auditServiceAudience || !secureCredentialValue(credential.Token) {
			return nil, fmt.Errorf("invalid audit-core service credential policy")
		}
		if _, exists := seenPrincipals[credential.Principal]; exists {
			return nil, fmt.Errorf("duplicate audit-core service principal")
		}
		seenPrincipals[credential.Principal] = struct{}{}
		tokenDigest := sha256.Sum256([]byte(credential.Token))
		if _, exists := seenTokens[tokenDigest]; exists {
			return nil, fmt.Errorf("duplicate audit-core service credential")
		}
		seenTokens[tokenDigest] = struct{}{}
		if len(credential.Scopes) == 0 {
			return nil, fmt.Errorf("audit-core service principal has no scopes")
		}
		writer := false
		for _, scope := range credential.Scopes {
			switch scope {
			case "audit:read:self":
			case "audit:write":
				writer = true
			default:
				return nil, fmt.Errorf("audit-core service principal has an unsupported scope")
			}
		}
		if writer && len(credential.Planes) == 0 {
			return nil, fmt.Errorf("audit:write principal must declare allowed planes")
		}
		if writer && !producerIdentityPattern.MatchString(credential.Principal) {
			return nil, fmt.Errorf("audit:write principal must be a producer authority token")
		}
		for _, plane := range credential.Planes {
			if !serviceIdentityPattern.MatchString(plane) {
				return nil, fmt.Errorf("audit-core service principal has an invalid plane")
			}
		}
		validated = append(validated, credential)
	}
	return validated, nil
}

// ValidateRequiredServiceCredentialRegistry verifies the canonical callers
// needed by the Audit HTTP boundary before the process opens a listener.
func ValidateRequiredServiceCredentialRegistry(raw string) error {
	credentials, err := parseServiceCredentialRegistry(raw)
	if err != nil {
		return err
	}

	gatewayReady := false
	ingestionWriterReady := false
	for _, credential := range credentials {
		switch credential.Principal {
		case "verevon-gateway":
			gatewayReady = credentialHasScope(credential, "audit:read:self")
		case "integration-corev2":
			ingestionWriterReady = credentialHasScope(credential, "audit:write") &&
				len(credential.Planes) == 1 && credential.Planes[0] == "ingestion"
		}
	}
	if !gatewayReady {
		return fmt.Errorf("AUDIT_CORE_SERVICE_CREDENTIALS must contain verevon-gateway with audit:read:self")
	}
	if !ingestionWriterReady {
		return fmt.Errorf("AUDIT_CORE_SERVICE_CREDENTIALS must contain integration-corev2 with audit:write pinned to ingestion")
	}
	return nil
}

func ensureJSONEOF(decoder *json.Decoder) error {
	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		if err == nil {
			return fmt.Errorf("AUDIT_CORE_SERVICE_CREDENTIALS contains trailing JSON")
		}
		return fmt.Errorf("decode AUDIT_CORE_SERVICE_CREDENTIALS trailer: %w", err)
	}
	return nil
}

func normalizedUnique(values []string) []string {
	seen := make(map[string]struct{}, len(values))
	result := make([]string, 0, len(values))
	for _, value := range values {
		value = strings.TrimSpace(value)
		if value == "" {
			continue
		}
		if _, exists := seen[value]; exists {
			continue
		}
		seen[value] = struct{}{}
		result = append(result, value)
	}
	return result
}

func secureCredentialValue(value string) bool {
	lower := strings.ToLower(strings.TrimSpace(value))
	return len(value) >= 32 &&
		!strings.HasPrefix(lower, "test") &&
		!strings.HasPrefix(lower, "placeholder") &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
}

func secureEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return hmac.Equal(expectedDigest[:], receivedDigest[:])
}

func requiredAuditScope(request *http.Request) string {
	switch {
	case request.Method == http.MethodGet &&
		(request.URL.Path == "/v1/audit" || request.URL.Path == "/v1/usage" || request.URL.Path == "/v1/usage/summary"):
		return "audit:read:self"
	case request.Method == http.MethodPost &&
		(request.URL.Path == "/v1/audit" || request.URL.Path == "/v1/usage"):
		return "audit:write"
	default:
		return ""
	}
}

func credentialHasScope(credential serviceCredential, required string) bool {
	for _, scope := range credential.Scopes {
		if scope == required {
			return true
		}
	}
	return false
}

func authorizeServiceRequest(request *http.Request, credentials []serviceCredential, nonces *delegationNonceCache, now time.Time) (serviceAuthorization, *authFailure) {
	principal, principalOK := requiredSingleHeader(request, "X-Service-Id")
	token, tokenOK := requiredSingleHeader(request, "X-Service-Token")
	if !principalOK || !tokenOK {
		return serviceAuthorization{}, &authFailure{Status: http.StatusUnauthorized, Message: "unauthorized"}
	}
	requiredScope := requiredAuditScope(request)
	if requiredScope == "" {
		return serviceAuthorization{}, &authFailure{Status: http.StatusForbidden, Message: "service scope denied"}
	}

	var matched *serviceCredential
	for index := range credentials {
		candidate := &credentials[index]
		if candidate.Principal == principal && secureEqual(candidate.Token, token) {
			matched = candidate
			break
		}
	}
	if matched == nil {
		return serviceAuthorization{}, &authFailure{Status: http.StatusUnauthorized, Message: "unauthorized"}
	}
	if !credentialHasScope(*matched, requiredScope) {
		return serviceAuthorization{}, &authFailure{Status: http.StatusForbidden, Message: "service scope denied"}
	}

	authorization := serviceAuthorization{
		Principal: matched.Principal,
		Scope:     requiredScope,
		planes:    make(map[string]struct{}, len(matched.Planes)),
	}
	for _, plane := range matched.Planes {
		authorization.planes[plane] = struct{}{}
	}
	if requiredScope == "audit:write" {
		return authorization, nil
	}

	claims, ok := verifyAuditReadDelegation(request, *matched, nonces, now)
	if !ok {
		return serviceAuthorization{}, &authFailure{Status: http.StatusForbidden, Message: "signed service delegation required"}
	}
	organizationValues := request.URL.Query()["org_id"]
	if len(organizationValues) != 1 || strings.TrimSpace(organizationValues[0]) != claims.OrgID {
		return serviceAuthorization{}, &authFailure{Status: http.StatusForbidden, Message: "delegated organization mismatch"}
	}
	authorization.UserID = claims.UserID
	authorization.OrgID = claims.OrgID
	authorization.UserRole = claims.UserRole
	return authorization, nil
}

type auditReadDelegation struct {
	UserID   string
	OrgID    string
	UserRole string
}

func verifyAuditReadDelegation(request *http.Request, credential serviceCredential, nonces *delegationNonceCache, now time.Time) (auditReadDelegation, bool) {
	version, ok := requiredSingleHeader(request, "X-Delegation-Version")
	if !ok || version != "v3" {
		return auditReadDelegation{}, false
	}
	timestampValue, ok := requiredSingleHeader(request, "X-Delegation-Timestamp")
	if !ok || !delegationTimestampPattern.MatchString(timestampValue) {
		return auditReadDelegation{}, false
	}
	timestamp, err := time.Parse(time.RFC3339, timestampValue)
	if err != nil {
		return auditReadDelegation{}, false
	}
	age := now.UTC().Sub(timestamp.UTC())
	if age > delegationMaxAge || age < -delegationFutureSkew {
		return auditReadDelegation{}, false
	}
	nonce, nonceOK := requiredSingleHeader(request, "X-Delegation-Nonce")
	if !nonceOK || len(nonce) < delegationNonceMinLength || len(nonce) > delegationNonceMaxLength {
		return auditReadDelegation{}, false
	}
	userID, userOK := requiredSingleHeader(request, "X-User-Id")
	orgID, orgOK := requiredSingleHeader(request, "X-Org-Id")
	userRole, roleOK := requiredSingleHeader(request, "X-User-Role")
	if !userOK || !orgOK || !roleOK {
		return auditReadDelegation{}, false
	}

	body, ok := boundedRequestBody(request)
	if !ok {
		return auditReadDelegation{}, false
	}
	bodyDigest := sha256.Sum256(body)
	bodySHA := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	providedBodySHA, bodySHAOK := requiredSingleHeader(request, "X-Delegation-Body-SHA256")
	if !bodySHAOK || !secureEqual(bodySHA, providedBodySHA) {
		return auditReadDelegation{}, false
	}
	canonical := strings.Join([]string{
		"v3",
		credential.Principal,
		credential.Audience,
		timestampValue,
		nonce,
		request.Method,
		request.URL.RequestURI(),
		userID,
		orgID,
		userRole,
		bodySHA,
	}, "\n")
	signatureValue, signatureOK := requiredSingleHeader(request, "X-Delegation-Signature")
	if !signatureOK {
		return auditReadDelegation{}, false
	}
	providedSignature, err := base64.RawURLEncoding.DecodeString(signatureValue)
	if err != nil {
		return auditReadDelegation{}, false
	}
	mac := hmac.New(sha256.New, []byte(credential.Token))
	_, _ = mac.Write([]byte(canonical))
	if !hmac.Equal(mac.Sum(nil), providedSignature) {
		return auditReadDelegation{}, false
	}
	if nonces == nil || !nonces.Consume(credential.Principal+":"+nonce, timestamp.Add(delegationMaxAge), now.UTC()) {
		return auditReadDelegation{}, false
	}
	return auditReadDelegation{UserID: userID, OrgID: orgID, UserRole: userRole}, true
}

func requiredSingleHeader(request *http.Request, name string) (string, bool) {
	values := request.Header.Values(name)
	if len(values) != 1 {
		return "", false
	}
	value := strings.TrimSpace(values[0])
	return value, value != ""
}

func boundedRequestBody(request *http.Request) ([]byte, bool) {
	if request.Body == nil || request.Body == http.NoBody {
		return nil, true
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, delegationMaxBody+1))
	if err != nil || len(body) > delegationMaxBody {
		return nil, false
	}
	request.Body = io.NopCloser(bytes.NewReader(body))
	return body, true
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
	if c == nil || c.capacity < 1 || key == "" || !expiry.After(now) {
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
