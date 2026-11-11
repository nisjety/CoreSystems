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
	serviceCredentialAudience = "org-core"
	serviceCredentialEnv      = "ORG_CORE_SERVICE_CREDENTIALS"
	gatewayServicePrincipal   = "verevon-gateway"
	siemExporterPrincipal     = "audit-core"
	browserBrokerPrincipal    = "browser-broker"
	authCorePrincipal         = "auth-core"
	domainReaderPrincipal     = "auth-core"
	scimEdgePrincipal         = "verevon-scim-edge"
	quarryEdgePrincipal       = "quarry-edge"
	sessionCorePrincipal      = "session-core"
	serviceDelegationMaxAge   = 30 * time.Second
	serviceDelegationSkew     = 5 * time.Second
	serviceDelegationMaxBody  = 1 << 20
)

var orgServiceScopeAllowlist = map[string]struct{}{
	"org:auth:proxy":                {},
	"org:read:self":                 {},
	"org:read:any":                  {},
	"org:provision:self":            {},
	"org:settings:write:self":       {},
	"org:brreg:read":                {},
	"org:website:read":              {},
	"org:erase:self":                {},
	"org:deletion:self":             {},
	"org:tenant:read:any":           {},
	"org:tenant:write:any":          {},
	"org:onboarding:write:self":     {},
	"org:onboarding:write:any":      {},
	"org:projection:write:any":      {},
	"org:projection:delete:any":     {},
	"org:membership:succession:any": {},
	// Read-only access to one org's interactive-retention posture (the stored
	// ZDR intent and the plan entitlement), through
	// GET /internal/orgs/:orgId/retention-posture and nothing else. Deliberately
	// not org:read:any, which returns the whole organization record.
	"org:retention:read:any": {},
	// Read-only access to one org's memory and learning-review policy, through
	// GET /internal/orgs/:orgId/agent-policy and nothing else (WS07-S4). Held by
	// session-core, which mirrors it; deliberately not org:read:any.
	"org:agent-policy:read:any": {},
	// Read-only access to the resolved chat model policy of one org and user,
	// through GET /internal/orgs/:orgId/model-entitlement and nothing else. Held
	// by inference-core (the enforcement point, ADR-0005) and by model-gateway
	// for explain; deliberately not org:read:any.
	"org:model-entitlement:read:any": {},
	// SIEM destination administration (WS06-S7): owner/admin CRUD through the
	// gateway on behalf of a signed user, and the exporter's read of every
	// enabled destination with its unsealed secret. The read is held by audit-core
	// alone (validateServiceCredential); neither is org:read:any.
	"org:siem:manage:self": {},
	"org:siem:read:any":    {},
	// Identity administration (WS05-S6): owner/admin domain verification through the
	// gateway on behalf of a signed user, and the by-domain read of which organisation
	// holds a verified domain, held by auth-core alone (validateServiceCredential).
	"org:identity:admin:self": {},
	// The SCIM surface (WS05-S8), held by the gateway's stateless SCIM edge alone
	// (validateServiceCredential); the IdP's own bearer token still names the org.
	"org:scim:any":        {},
	"org:domain:read:any": {},
	// Read-only access to one org's agent-browser site decision, through
	// GET /internal/orgs/:orgId/browser-sites/resolve and nothing else (WS21-S3).
	// Held by the browser broker alone (validateServiceCredential); the decision
	// is the broker's input at grant mint, never a member's list.
	"org:browser-site:read:any": {},
	// Quarry's per-org browser-profile key custody (WS17.E2): generate a data key
	// under the org's KEK (wrap) and unwrap/rewrap it, through
	// /internal/orgs/:orgId/keys/... and nothing else. Held by quarry-edge alone in
	// the compose registry. The routes exist only when ORG_CORE_KEYS_ENABLED=1, so
	// until then these scopes reach nothing; they are allowlisted now so the
	// static credential (R-13 PR 2) and the org-core image can ship together.
	"org:keys:wrap:any":   {},
	"org:keys:unwrap:any": {},
	// Crypto-shred of one org's keys, through DELETE /internal/orgs/:orgId/keys and
	// nothing else (WS17.E2, Q2-WS17.E2a). Irreversible, so it is held by
	// quarry-edge alone, for the org erasure saga's shred hop
	// (QUARRY_EDGE__ERASE_SHRED_KEYS, default off). The admin scope
	// (org:keys:admin:any: rotate, metadata) stays out of this list.
	"org:keys:destroy:any": {},
	// Read-only access to one org's web policy, through
	// GET /internal/orgs/:orgId/web-policy and nothing else (WS7.H). Held by
	// auth-core alone, which folds it into the web_policy token claim (WS7.I,
	// WEB_POLICY_CLAIM_ENABLED). Unread until WS7.H registers the route.
	"org:web-policy:read:any": {},
	// Read-only access to one org's retention window and active legal holds, through
	// POST /internal/orgs/:orgId/retention/check and nothing else (EP-9 C1). Held by
	// session-core alone, whose sweeper drops every held thread.
	"org:retention-policy:read:any": {},
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
		if _, allowed := orgServiceScopeAllowlist[scope]; !allowed {
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
		// The exporter read returns plaintext destination secrets, so unlike the
		// other :any scopes it is pinned to one principal.
		if scope == "org:siem:read:any" && credential.Principal != siemExporterPrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		if scope == "org:scim:any" && credential.Principal != scimEdgePrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		if scope == "org:domain:read:any" && credential.Principal != domainReaderPrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		if scope == "org:browser-site:read:any" && credential.Principal != browserBrokerPrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		// The web policy read feeds a token claim that bounds what agents may do, so
		// it is pinned to the one principal that mints it (WS7.H, WS7.I).
		if scope == "org:web-policy:read:any" && credential.Principal != authCorePrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		// The check decides which threads the retention sweeper may delete, so it is
		// pinned to the sweeper's principal (EP-9 C1).
		if scope == "org:retention-policy:read:any" && credential.Principal != sessionCorePrincipal {
			return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal)
		}
		// The key scopes hand out plaintext data keys of ANY org, so they are
		// pinned to the one principal that seals Quarry profiles (WS17.E2). The
		// destroy scope crypto-shreds an org irreversibly, so it is pinned to the
		// same principal, whose org erasure saga is its only caller.
		if (scope == "org:keys:wrap:any" || scope == "org:keys:unwrap:any" || scope == "org:keys:destroy:any") && credential.Principal != quarryEdgePrincipal {
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

// ValidateRequiredServiceCredentialRegistry is called before any listener or
// database connection is opened. It also verifies the minimum gateway policy
// needed by the current Verevon v3 organization flows.
func ValidateRequiredServiceCredentialRegistry(raw string) error {
	credentials, err := parseServiceCredentials(raw)
	if err != nil {
		return err
	}
	required := map[string]bool{
		"org:read:self":             false,
		"org:provision:self":        false,
		"org:settings:write:self":   false,
		"org:onboarding:write:self": false,
	}
	gatewayCount := 0
	authRequired := map[string]bool{
		"org:projection:write:any":  false,
		"org:projection:delete:any": false,
		// Auth Core resolves each org's ZDR posture at token mint through this
		// scope. Without it every org silently resolves to non-ZDR (or, now,
		// every mint fails with 503), so a registry that lacks it must not boot.
		"org:retention:read:any": false,
	}
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
		case authCorePrincipal:
			authCount++
			for _, scope := range credential.Scopes {
				if _, exists := authRequired[scope]; exists {
					authRequired[scope] = true
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
	for scope, present := range authRequired {
		if !present {
			return fmt.Errorf("auth-core principal is missing required %s scope", scope)
		}
	}
	return nil
}

func secureServiceValueEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return subtle.ConstantTimeCompare(expectedDigest[:], receivedDigest[:]) == 1
}

func serviceScopesForRequest(request *http.Request) []string {
	path := request.URL.Path
	method := request.Method
	segments, valid := cleanPathSegments(path)
	if !valid {
		return nil
	}

	switch {
	case method == http.MethodPost && path == "/api/v1/auth/login":
		return []string{"org:auth:proxy"}
	case method == http.MethodGet && path == "/api/v1/users/me":
		return []string{"org:read:self"}
	case len(segments) >= 3 && segments[0] == "api" && segments[1] == "v1" && segments[2] == "organizations":
		return organizationAPIScopes(method, segments[3:])
	case len(segments) >= 2 && segments[0] == "api" && segments[1] == "v1" && len(segments) >= 3 && segments[2] == "brreg":
		if method == http.MethodGet && (len(segments) == 4) {
			return []string{"org:brreg:read"}
		}
	case len(segments) >= 1 && segments[0] == "orgs":
		return organizationProxyScopes(method, segments[1:])
	case len(segments) >= 3 && segments[0] == "internal" && segments[1] == "scim" && segments[2] == "v2":
		return []string{"org:scim:any"}
	case len(segments) >= 2 && segments[0] == "internal" && segments[1] == "orgs":
		return internalOrganizationScopes(method, segments[2:])
	}
	return nil
}

func cleanPathSegments(path string) ([]string, bool) {
	if path == "" || path == "/" || strings.Contains(path, "//") || strings.HasSuffix(path, "/") {
		return nil, false
	}
	return strings.Split(strings.TrimPrefix(path, "/"), "/"), true
}

func organizationAPIScopes(method string, rest []string) []string {
	switch {
	case len(rest) == 0 && method == http.MethodGet:
		return []string{"org:read:self"}
	case len(rest) == 0 && method == http.MethodPost:
		return []string{"org:provision:self"}
	case len(rest) == 1 && method == http.MethodGet:
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 2 && method == http.MethodGet && (rest[1] == "entitlements" || rest[1] == "quotas"):
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 3 && method == http.MethodPut && rest[1] == "quotas":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "members" && rest[2] == "search":
		return []string{"org:read:self", "org:read:any"}
	// The model policy is read and written only on behalf of a signed user
	// (:self); org:read:any holders never see it.
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "model-entitlement":
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "model-entitlement":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "model-entitlement" && rest[2] == "explain":
		return []string{"org:read:self"}
	// The agent-browser site policy (WS21-S3): an admin reads and writes it, and a
	// member reads and writes only their own choices, all on behalf of a signed user.
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "browser-sites":
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "browser-sites":
		return []string{"org:settings:write:self"}
	// The org web policy (WS7.H): read and written only on behalf of a signed user
	// (:self); org:read:any holders never see it. The history is an admin read.
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "web-policy":
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "web-policy":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "web-policy" && rest[2] == "history":
		return []string{"org:read:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "browser-sites" && (rest[2] == "explain" || rest[2] == "me"):
		return []string{"org:read:self"}
	case len(rest) == 4 && rest[1] == "browser-sites" && rest[2] == "me" && (method == http.MethodPut || method == http.MethodDelete):
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPost && rest[1] == "plan":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPatch && rest[1] == "settings":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPatch && rest[1] == "brreg":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPatch && rest[1] == "website":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "website":
		return []string{"org:website:read"}
	// Retention window and legal holds (EP-9 C1), on behalf of a signed user only.
	case len(rest) == 2 && method == http.MethodGet && (rest[1] == "retention" || rest[1] == "legal-holds"):
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "retention":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPost && rest[1] == "legal-holds":
		return []string{"org:settings:write:self"}
	case len(rest) == 4 && method == http.MethodPost && rest[1] == "legal-holds" && rest[3] == "release":
		return []string{"org:settings:write:self"}
	default:
		return nil
	}
}

func organizationProxyScopes(method string, rest []string) []string {
	switch {
	case len(rest) == 0 && method == http.MethodGet:
		return []string{"org:read:self"}
	case len(rest) == 0 && method == http.MethodPost:
		return []string{"org:provision:self"}
	case len(rest) == 1 && rest[0] == "me" && method == http.MethodGet:
		return []string{"org:read:self"}
	case len(rest) == 1 && method == http.MethodGet:
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 2 && method == http.MethodGet && (rest[1] == "entitlements" || rest[1] == "members" || rest[1] == "roles" || rest[1] == "quotas"):
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 3 && method == http.MethodPut && rest[1] == "quotas":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "members" && rest[2] == "search":
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "roles" && rest[2] == "catalog":
		return []string{"org:read:self", "org:read:any"}
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "model-entitlement":
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "model-entitlement":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "model-entitlement" && rest[2] == "explain":
		return []string{"org:read:self"}
	// The org web policy (WS7.H), on behalf of a signed user only (:self).
	case len(rest) == 2 && method == http.MethodGet && rest[1] == "web-policy":
		return []string{"org:read:self"}
	case len(rest) == 2 && method == http.MethodPut && rest[1] == "web-policy":
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodGet && rest[1] == "web-policy" && rest[2] == "history":
		return []string{"org:read:self"}
	// SIEM destinations are administered on behalf of a signed owner or admin
	// only (:self); no :any holder reaches them through this path.
	case len(rest) == 2 && rest[1] == "siem-destinations" && (method == http.MethodGet || method == http.MethodPost):
		return []string{"org:siem:manage:self"}
	case len(rest) == 3 && rest[1] == "siem-destinations" && (method == http.MethodPatch || method == http.MethodDelete):
		return []string{"org:siem:manage:self"}
	// Domain verification (WS05-S6) is administered on behalf of a signed owner or
	// admin only (:self); the handlers re-check the role from the membership table.
	case len(rest) == 2 && rest[1] == "domains" && (method == http.MethodGet || method == http.MethodPost):
		return []string{"org:identity:admin:self"}
	case len(rest) == 3 && rest[1] == "domains" && method == http.MethodDelete:
		return []string{"org:identity:admin:self"}
	case len(rest) == 4 && rest[1] == "domains" && rest[3] == "verify" && method == http.MethodPost:
		return []string{"org:identity:admin:self"}
	// SSO policy and providers (WS05-S7): the same signed owner/admin authority.
	case len(rest) == 3 && rest[1] == "scim" && rest[2] == "tokens" && (method == http.MethodGet || method == http.MethodPost):
		return []string{"org:identity:admin:self"}
	case len(rest) == 4 && rest[1] == "scim" && rest[2] == "tokens" && method == http.MethodDelete:
		return []string{"org:identity:admin:self"}
	case len(rest) == 3 && rest[1] == "scim" && rest[2] == "status" && method == http.MethodGet:
		return []string{"org:identity:admin:self"}
	case len(rest) == 3 && rest[1] == "sso" && rest[2] == "policy" && (method == http.MethodGet || method == http.MethodPut):
		return []string{"org:identity:admin:self"}
	case len(rest) == 3 && rest[1] == "sso" && rest[2] == "providers" && method == http.MethodPost:
		return []string{"org:identity:admin:self"}
	case len(rest) == 4 && rest[1] == "sso" && rest[2] == "providers" && method == http.MethodDelete:
		return []string{"org:identity:admin:self"}
	case len(rest) == 2 && method == http.MethodPost && rest[1] == "plan":
		return []string{"org:settings:write:self"}
	case len(rest) == 2 && method == http.MethodPatch && (rest[1] == "capabilities" || rest[1] == "settings"):
		return []string{"org:settings:write:self"}
	case len(rest) == 3 && method == http.MethodDelete && rest[1] == "gdpr" && (rest[2] == "erase" || rest[2] == "soft-delete"):
		return []string{"org:erase:self"}
	// restore reverses a pending soft-delete — same owner-gated erasure
	// lifecycle trust tier as erase/soft-delete above, so it reuses their
	// scope rather than minting a near-duplicate one.
	case len(rest) == 3 && method == http.MethodPost && rest[1] == "gdpr" && rest[2] == "restore":
		return []string{"org:erase:self"}
	// The Flow C self-service surface (mark-exported / acknowledge / status)
	// is a distinct, lower-stakes trust tier: any active member may act on
	// their own ledger row (not owner-only), so it gets its own scope.
	case len(rest) == 4 && rest[1] == "gdpr" && rest[2] == "deletion" &&
		((method == http.MethodPost && (rest[3] == "mark-exported" || rest[3] == "acknowledge")) ||
			(method == http.MethodGet && rest[3] == "status")):
		return []string{"org:deletion:self"}
	default:
		return nil
	}
}

func internalOrganizationScopes(method string, rest []string) []string {
	switch {
	case len(rest) == 0 && method == http.MethodGet:
		return []string{"org:read:any"}
	case len(rest) == 1 && rest[0] == "by-tenant" && method == http.MethodGet:
		return []string{"org:tenant:read:any"}
	// Before the generic one-segment read below: org:read:any must not open the
	// route that returns destination secrets.
	case len(rest) == 1 && rest[0] == "siem-destinations" && method == http.MethodGet:
		return []string{"org:siem:read:any"}
	// Which organisation holds a verified domain (WS05-S6); before the generic reads.
	case len(rest) == 2 && rest[0] == "by-domain" && method == http.MethodGet:
		return []string{"org:domain:read:any"}
	// The SSO policy covering a domain (WS05-S7), read by the same principal.
	case len(rest) == 3 && rest[0] == "by-domain" && rest[2] == "sso-policy" && method == http.MethodGet:
		return []string{"org:domain:read:any"}
	case len(rest) == 1 && method == http.MethodGet:
		return []string{"org:read:any"}
	case len(rest) == 1 && rest[0] == "ensure-from-tenant" && method == http.MethodPost:
		return []string{"org:tenant:write:any"}
	case len(rest) == 3 && rest[1] == "onboarding" && rest[2] == "state" && method == http.MethodPost:
		return []string{"org:onboarding:write:self", "org:onboarding:write:any"}
	case len(rest) == 2 && rest[1] == "retention-posture" && method == http.MethodGet:
		return []string{"org:retention:read:any"}
	case len(rest) == 2 && rest[1] == "agent-policy" && method == http.MethodGet:
		return []string{"org:agent-policy:read:any"}
	case len(rest) == 2 && rest[1] == "model-entitlement" && method == http.MethodGet:
		return []string{"org:model-entitlement:read:any"}
	case len(rest) == 3 && rest[1] == "browser-sites" && rest[2] == "resolve" && method == http.MethodGet:
		return []string{"org:browser-site:read:any"}
	case len(rest) == 2 && rest[1] == "web-policy" && method == http.MethodGet:
		return []string{"org:web-policy:read:any"}
	case len(rest) == 3 && rest[1] == "retention" && rest[2] == "check" && method == http.MethodPost:
		return []string{"org:retention-policy:read:any"}
	// Key custody (WS17.E2). The admin scope is deliberately not in
	// orgServiceScopeAllowlist yet, so no credential holds it and rotate and
	// metadata answer 403 until a reviewed registry change. The destroy scope is
	// allowlisted and pinned to quarry-edge (org erasure saga, Q2-WS17.E2a).
	case len(rest) == 3 && rest[1] == "keys" && rest[2] == "data-keys" && method == http.MethodPost:
		return []string{"org:keys:wrap:any"}
	case len(rest) == 4 && rest[1] == "keys" && rest[2] == "data-keys" && rest[3] == "unwrap" && method == http.MethodPost:
		return []string{"org:keys:unwrap:any"}
	case len(rest) == 4 && rest[1] == "keys" && rest[2] == "data-keys" && rest[3] == "rewrap" && method == http.MethodPost:
		return []string{"org:keys:wrap:any"}
	case len(rest) == 3 && rest[1] == "keys" && rest[2] == "rotate" && method == http.MethodPost:
		return []string{"org:keys:admin:any"}
	case len(rest) == 2 && rest[1] == "keys" && method == http.MethodGet:
		return []string{"org:keys:admin:any"}
	case len(rest) == 2 && rest[1] == "keys" && method == http.MethodDelete:
		return []string{"org:keys:destroy:any"}
	case len(rest) == 2 && rest[1] == "reconcile" && method == http.MethodPost:
		return []string{"org:projection:write:any"}
	case len(rest) == 3 && rest[1] == "members" && rest[2] == "reconcile" && method == http.MethodPost:
		return []string{"org:projection:write:any"}
	case len(rest) == 2 && rest[1] == "reconcile-delete" && method == http.MethodPost:
		return []string{"org:projection:delete:any"}
	case len(rest) == 4 && rest[1] == "members" && rest[3] == "succession" && method == http.MethodPost:
		return []string{"org:membership:succession:any"}
	case len(rest) == 4 && rest[1] == "roles" && rest[3] == "capabilities" && method == http.MethodGet:
		return []string{"org:read:any"}
	default:
		return nil
	}
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
	if claims.OrgID, ok = singleTrimmedHeader(request, "X-Org-Id"); !ok || len(claims.OrgID) > 128 {
		return serviceDelegationClaims{}, false
	}
	if claims.UserRole, ok = singleTrimmedHeader(request, "X-User-Role"); !ok || len(claims.UserRole) > 64 {
		return serviceDelegationClaims{}, false
	}
	if routeOrgID := organizationIDFromPath(request.URL.Path); routeOrgID != "" && routeOrgID != claims.OrgID {
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

func organizationIDFromPath(path string) string {
	segments, valid := cleanPathSegments(path)
	if !valid {
		return ""
	}
	if len(segments) >= 2 && segments[0] == "orgs" && segments[1] != "me" {
		return segments[1]
	}
	if len(segments) >= 4 && segments[0] == "api" && segments[1] == "v1" && segments[2] == "organizations" {
		return segments[3]
	}
	if len(segments) == 5 && segments[0] == "internal" && segments[1] == "orgs" &&
		segments[3] == "onboarding" && segments[4] == "state" {
		return segments[2]
	}
	return ""
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
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
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
