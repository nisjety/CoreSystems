package http

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

type serviceCredential struct {
	Principal string   `json:"principal"`
	Audience  string   `json:"audience"`
	Token     string   `json:"token"`
	Scopes    []string `json:"scopes"`
}

func parseServiceCredentials(raw string) ([]serviceCredential, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, nil
	}
	var credentials []serviceCredential
	if err := json.Unmarshal([]byte(raw), &credentials); err != nil {
		return nil, fmt.Errorf("decode USER_CORE_SERVICE_CREDENTIALS: %w", err)
	}
	for _, credential := range credentials {
		if strings.TrimSpace(credential.Principal) == "" || credential.Audience != "user-core" || len(credential.Token) < 32 || len(credential.Scopes) == 0 {
			return nil, fmt.Errorf("invalid user-core service credential policy")
		}
		for _, scope := range credential.Scopes {
			if strings.TrimSpace(scope) == "" {
				return nil, fmt.Errorf("invalid empty user-core service scope")
			}
		}
	}
	return credentials, nil
}

// ValidateRequiredServiceCredentialRegistry verifies the minimum canonical
// gateway identity that user-core needs to serve delegated self-profile and
// session-context requests. Production startup calls this before opening any
// listener so a malformed or incomplete registry cannot look healthy while
// every gateway membership lookup fails.
func ValidateRequiredServiceCredentialRegistry(raw string) error {
	credentials, err := parseServiceCredentials(raw)
	if err != nil {
		return err
	}
	if len(credentials) == 0 {
		return fmt.Errorf("USER_CORE_SERVICE_CREDENTIALS is required")
	}

	const gatewayPrincipal = "verevon-gateway"
	requiredScopes := map[string]bool{
		"users:read:self":  false,
		"users:write:self": false,
	}
	gatewayEntries := 0
	for _, credential := range credentials {
		if credential.Principal != gatewayPrincipal {
			continue
		}
		gatewayEntries++
		for _, scope := range credential.Scopes {
			if _, required := requiredScopes[scope]; required {
				requiredScopes[scope] = true
			}
		}
	}
	if gatewayEntries != 1 {
		return fmt.Errorf("USER_CORE_SERVICE_CREDENTIALS must contain exactly one %s principal", gatewayPrincipal)
	}
	for scope, present := range requiredScopes {
		if !present {
			return fmt.Errorf("%s principal is missing required %s scope", gatewayPrincipal, scope)
		}
	}
	return nil
}

func secureServiceTokenEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return subtle.ConstantTimeCompare(expectedDigest[:], receivedDigest[:]) == 1
}

func serviceScopeForRequest(request *http.Request) string {
	path := request.URL.Path
	method := request.Method
	switch {
	case path == "/api/v1/internal/spaces/register":
		return "spaces:register"
	case path == "/api/v1/internal/spaces/deletion-authorizations":
		return "spaces:deletion:authorize"
	case path == "/api/v1/internal/spaces/deletion-policy":
		return "spaces:policy:write"
	case strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/legal-hold"):
		return "spaces:policy:write"
	case path == "/api/v1/internal/spaces/recipient-audiences":
		return "spaces:audience:publish"
	case method == http.MethodGet && strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/membership"):
		return "spaces:resolve"
	case path == "/api/v1/internal/spaces/personal-thread-decision" || path == "/api/v1/internal/spaces/thread-decision" || path == "/api/v1/internal/spaces/thread-append-decision" || path == "/api/v1/internal/spaces/personal-retrieval-decision" || path == "/api/v1/internal/spaces/personal-import-decision" || path == "/api/v1/internal/spaces/schedule-create-decision":
		return "spaces:issue"
	case path == "/api/v1/internal/spaces/import-execution-decision":
		return "spaces:import:reauthorize"
	case path == "/api/v1/internal/spaces/schedule-fire-decision":
		return "spaces:schedule:reauthorize"
	case path == "/api/v1/internal/spaces/effect-policy":
		return "spaces:policy:write"
	case strings.HasPrefix(path, "/api/v1/internal/authz/"):
		if method == http.MethodGet || method == http.MethodHead {
			return "authz:read"
		}
		return "authz:write"
	case path == "/api/v1/internal/memberships/ensure":
		return "memberships:write"
	case path == "/api/v1/internal/users/enrich-from-provider":
		return "users:sync"
	case method == http.MethodGet || method == http.MethodHead:
		if strings.TrimSpace(request.Header.Get("X-User-Id")) == "" {
			return "users:read:any"
		}
		return "users:read:self"
	default:
		if strings.TrimSpace(request.Header.Get("X-User-Id")) == "" {
			return "users:write:any"
		}
		return "users:write:self"
	}
}

func credentialHasScope(credential serviceCredential, required string) bool {
	for _, scope := range credential.Scopes {
		if strings.TrimSpace(scope) == required {
			return true
		}
	}
	return false
}

// Static service credentials prove only the calling workload. They do not
// prove an end-user subject, tenant membership, or authority to act as a grant
// actor. Until user-core accepts a cryptographically verified delegation token
// that binds those claims to the service principal, these route families must
// remain unavailable to service credentials.
func serviceScopeRequiresVerifiedDelegation(scope string) bool {
	return strings.HasSuffix(scope, ":self") || strings.HasPrefix(scope, "authz:") || scope == "spaces:resolve" || scope == "spaces:issue"
}

// authenticateServicePrincipal returns (present, authorized). A presented but
// invalid credential must never fall through to a weaker auth mechanism.
func authenticateServicePrincipal(c *gin.Context, credentials []serviceCredential) (bool, bool) {
	token := strings.TrimSpace(c.GetHeader("X-Service-Token"))
	principal := strings.TrimSpace(c.GetHeader("X-Service-Id"))
	if token == "" && principal == "" {
		return false, false
	}
	if token == "" || principal == "" {
		return true, false
	}
	requiredScope := serviceScopeForRequest(c.Request)
	for _, credential := range credentials {
		if credential.Principal != principal || !secureServiceTokenEqual(credential.Token, token) {
			continue
		}
		if !credentialHasScope(credential, requiredScope) {
			return true, false
		}
		if serviceScopeRequiresVerifiedDelegation(requiredScope) {
			claims, valid := verifyServiceDelegation(c.Request, credential, time.Now())
			if !valid {
				return true, false
			}
			c.Set("user_id", claims.UserID)
			c.Set("org_id", claims.OrgID)
			c.Set("user_email", claims.Email)
			c.Set("user_name", claims.Name)
			c.Set("user_avatar", claims.Avatar)
			c.Set("delegation_verified", true)
			c.Set("delegation_version", claims.Version)
			c.Set("delegation_operation", claims.Operation)
			c.Set("delegation_resource_type", claims.ResourceType)
			c.Set("delegation_resource_id", claims.ResourceID)
			c.Set("delegation_reason", claims.Reason)
			c.Set("delegation_zdr", claims.ZDR)
			c.Set("delegation_nonce", claims.Nonce)
		}
		c.Set("auth_method", "service_principal")
		c.Set("service_id", credential.Principal)
		c.Set("service_scopes", append([]string(nil), credential.Scopes...))
		return true, true
	}
	return true, false
}

func hasServiceScope(c *gin.Context, required string) bool {
	value, exists := c.Get("service_scopes")
	if !exists {
		return false
	}
	scopes, ok := value.([]string)
	if !ok {
		return false
	}
	for _, scope := range scopes {
		if scope == required {
			return true
		}
	}
	return false
}
