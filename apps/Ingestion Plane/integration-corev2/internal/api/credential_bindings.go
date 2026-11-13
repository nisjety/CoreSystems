package api

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Credential bindings (dark, CREDENTIAL_BINDINGS_ENABLED). W4-CC-3, ADR-NEXT-sandbox-credential-injection
// sections 1 and 4.
//
// An organisation admin binds one secret to one host: the header the secret may be placed in, and
// the methods and path prefixes that may be forwarded with it. The secret is accepted once, sealed
// in this service with the credential vault (additional data = the binding id) and never returned,
// logged, audited or put in an error. List returns metadata only; DELETE revokes (and empties the
// stored ciphertext). Nothing here redeems a binding: the seal and redeem routes that consume it
// ship later behind their own switch.
//
// A binding cannot be created for a zero-data-retention organisation: a ZDR organisation stores no
// content, and a standing credential is stored state. Audit rows carry ids only.

// Audit event types written by the credential-binding routes. Ids only, never a host, header or secret.
const (
	auditEventCredentialBindingCreated = "credential_binding.created"
	auditEventCredentialBindingRevoked = "credential_binding.revoked"
)

const (
	credentialBindingsPath    = "/api/v1/credential-bindings"
	credentialBindingsItem    = "/api/v1/credential-bindings/:id"
	credentialBindingIDPrefix = "cb_"
	// credentialBindingKeyVersion names the vault key generation the secret was sealed under: the
	// vault's ciphertext prefix (internal/crypto/vault.go). Key rotation moves it to the next one.
	credentialBindingKeyVersion = "v1"

	credentialBindingMaxSecretBytes   = 4096
	credentialBindingMaxHostLength    = 253
	credentialBindingMaxMethods       = 7
	credentialBindingMaxPathPrefixes  = 32
	credentialBindingMaxPathPrefixLen = 256
)

// CredentialSealer seals a binding's secret and opens it again for a redemption (W4-CC-4).
// *crypto.Vault satisfies it. The additional data is the binding id, so a ciphertext copied onto
// another binding does not open.
type CredentialSealer interface {
	Encrypt(plaintext string, additionalData []byte) (string, error)
	Decrypt(ciphertext string, additionalData []byte) (string, error)
}

// CredentialHostVetter resolves a bound host once and refuses private, loopback, link-local and
// metadata addresses. *egress.Guard satisfies it.
type CredentialHostVetter interface {
	ResolveAndVet(ctx context.Context, host string) ([]netip.Addr, error)
}

// credentialBindingHeaders is the closed list of headers a secret may be injected into.
var credentialBindingHeaders = []string{"Authorization", "X-Api-Key", "Api-Key", "X-Auth-Token"}

// credentialBindingMethods is the closed list of methods a binding may forward.
var credentialBindingMethods = []string{"GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"}

type credentialBindingCreateBody struct {
	Kind                string   `json:"kind"`
	Host                string   `json:"host"`
	HeaderName          string   `json:"header_name"`
	AllowedMethods      []string `json:"allowed_methods"`
	AllowedPathPrefixes []string `json:"allowed_path_prefixes"`
	// Secret is accepted once. It is sealed and then dropped; no route returns it.
	Secret string `json:"secret"`
}

// registerCredentialBindingRoutes mounts the admin routes. Nothing is mounted unless the switch is
// on and a sealer was built, so while the switch is off every path answers 404.
func registerCredentialBindingRoutes(
	app *fiber.App,
	cfg ServerConfig,
	bearerOnly fiber.Handler,
	rateLimited []fiber.Handler,
) {
	if !cfg.Config.CredentialBindingsEnabled || cfg.CredentialSealer == nil {
		return
	}

	app.Post(credentialBindingsPath, chainHandlers(rateLimited, bearerOnly, func(c *fiber.Ctx) error {
		return credentialBindingCreate(c, cfg)
	})...)
	app.Get(credentialBindingsPath, bearerOnly, func(c *fiber.Ctx) error {
		return credentialBindingList(c, cfg)
	})
	app.Delete(credentialBindingsItem, chainHandlers(rateLimited, bearerOnly, func(c *fiber.Ctx) error {
		return credentialBindingRevoke(c, cfg)
	})...)
}

// credentialBindingAdmin admits only a verified user who is an owner or admin of their
// organisation. A service principal and the internal key never reach a binding: a model may
// propose, but only a person grants.
func credentialBindingAdmin(c *fiber.Ctx) (auth.Principal, bool, error) {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || principal.PrincipalType != "user" ||
		strings.TrimSpace(principal.OrganizationID) == "" || strings.TrimSpace(principal.UserID) == "" {
		return auth.Principal{}, false, apiError(c, fiber.StatusUnauthorized, "unauthorized", "A verified user is required.")
	}
	if !zdrHasOrgAdminRole(principal.Role) {
		return auth.Principal{}, false, apiError(c, fiber.StatusForbidden, "org_admin_required", "Only an organisation owner or admin can manage credential bindings.")
	}
	if err := auth.AssertOrgAccess(c, principal.OrganizationID); err != nil {
		return auth.Principal{}, false, authAwareError(c, err)
	}
	return principal, true, nil
}

// credentialBindingCreate validates, vets the host, seals the secret and stores the binding with
// its audit row in one transaction.
func credentialBindingCreate(c *fiber.Ctx, cfg ServerConfig) error {
	principal, ok, denied := credentialBindingAdmin(c)
	if !ok {
		return denied
	}
	if cfg.Repo == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The binding store is unavailable.")
	}
	// ZDR is checked before the body is read, so a refused request never handles the secret. An
	// unreadable posture is a refusal, never a pass.
	if cfg.OrgZDR == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was stored.")
	}
	zdrOn, err := cfg.OrgZDR.EffectiveZDR(c.UserContext(), principal.OrganizationID)
	if err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was stored.")
	}
	if zdrOn {
		return apiError(c, fiber.StatusConflict, "credential_binding_zdr_refused", "A credential binding cannot be created while the organisation uses zero data retention.")
	}

	var body credentialBindingCreateBody
	if err := c.BodyParser(&body); err != nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
	}
	secret := body.Secret
	body.Secret = ""
	kind, ok := normalizeCredentialBindingKind(body.Kind)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_kind", "kind must be one of mcp_bearer, rest_api_key or rest_bearer.")
	}
	host, ok := normalizeCredentialBindingHost(body.Host)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_host", "host must be a lowercase DNS name on port 443, with no IP literal, wildcard, userinfo or path.")
	}
	header, ok := normalizeCredentialBindingHeader(kind, body.HeaderName)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_header_name", "header_name must be Authorization, X-Api-Key, Api-Key or X-Auth-Token, and a bearer kind uses Authorization.")
	}
	methods, ok := normalizeCredentialBindingMethods(body.AllowedMethods)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_allowed_methods", "allowed_methods must list 1 to 7 of GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS.")
	}
	prefixes, ok := normalizeCredentialBindingPathPrefixes(body.AllowedPathPrefixes)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_allowed_path_prefixes", "allowed_path_prefixes must list 1 to 32 absolute paths with no query, fragment, encoding or dot segments.")
	}
	if !credentialBindingSecretValid(secret) {
		return apiError(c, fiber.StatusBadRequest, "invalid_secret", "secret is required, at most 4096 characters, and must be visible ASCII with no spaces.")
	}

	vetter := cfg.CredentialHostVetter
	if vetter == nil {
		vetter = egress.Default()
	}
	if _, err := vetter.ResolveAndVet(c.UserContext(), host); err != nil {
		if errors.Is(err, egress.ErrBlocked) {
			return apiError(c, fiber.StatusBadRequest, "host_not_public", "host must point to a public address.")
		}
		return apiError(c, fiber.StatusBadRequest, "host_unresolvable", "host could not be resolved.")
	}

	bindingID := credentialBindingIDPrefix + uuid.NewString()
	sealed, err := cfg.CredentialSealer.Encrypt(secret, []byte(bindingID))
	secret = ""
	if err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "credential_unavailable", "The credential could not be stored safely.")
	}

	var created store.CredentialBinding
	if err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var saveErr error
		created, saveErr = tx.CreateCredentialBinding(c.UserContext(), store.CredentialBinding{
			ID:                  bindingID,
			OrganizationID:      principal.OrganizationID,
			CreatedByUserID:     principal.UserID,
			Kind:                kind,
			Host:                host,
			HeaderName:          header,
			AllowedMethods:      methods,
			AllowedPathPrefixes: prefixes,
			SecretCiphertext:    []byte(sealed),
			KeyVersion:          credentialBindingKeyVersion,
		})
		if saveErr != nil {
			return saveErr
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "credential-binding-created", created.ID),
			OrganizationID: created.OrganizationID,
			UserID:         principal.UserID,
			EventType:      auditEventCredentialBindingCreated,
			Metadata:       map[string]any{"bindingId": created.ID},
		})
	}); err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "credential_binding_persist_failed", "Could not store the credential binding safely.")
	}
	return c.Status(fiber.StatusCreated).JSON(fiber.Map{"success": true, "data": fiber.Map{"binding": created}})
}

// credentialBindingList returns the organisation's bindings as metadata only.
func credentialBindingList(c *fiber.Ctx, cfg ServerConfig) error {
	principal, ok, denied := credentialBindingAdmin(c)
	if !ok {
		return denied
	}
	if cfg.Repo == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The binding store is unavailable.")
	}
	bindings, err := cfg.Repo.ListCredentialBindings(c.UserContext(), principal.OrganizationID)
	if err != nil {
		return apiError(c, fiber.StatusInternalServerError, "credential_binding_list_failed", "Could not list credential bindings.")
	}
	return success(c, fiber.Map{"bindings": bindings})
}

// credentialBindingRevoke revokes one of the organisation's active bindings. Revoking is allowed
// under ZDR: removing a standing credential is never refused.
func credentialBindingRevoke(c *fiber.Ctx, cfg ServerConfig) error {
	principal, ok, denied := credentialBindingAdmin(c)
	if !ok {
		return denied
	}
	if cfg.Repo == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The binding store is unavailable.")
	}
	id := strings.TrimSpace(c.Params("id"))
	if !strings.HasPrefix(id, credentialBindingIDPrefix) {
		return apiError(c, fiber.StatusNotFound, "credential_binding_not_found", "The credential binding was not found.")
	}
	now := time.Now().UTC()
	var revoked store.CredentialBinding
	err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var revokeErr error
		revoked, revokeErr = tx.RevokeCredentialBinding(c.UserContext(), principal.OrganizationID, id, now)
		if revokeErr != nil {
			return revokeErr
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "credential-binding-revoked", revoked.ID),
			OrganizationID: revoked.OrganizationID,
			UserID:         principal.UserID,
			EventType:      auditEventCredentialBindingRevoked,
			Metadata:       map[string]any{"bindingId": revoked.ID},
			CreatedAt:      now,
		})
	})
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			return apiError(c, fiber.StatusNotFound, "credential_binding_not_found", "The credential binding was not found.")
		}
		return apiError(c, fiber.StatusServiceUnavailable, "credential_binding_persist_failed", "Could not revoke the credential binding safely.")
	}
	return success(c, fiber.Map{"binding": revoked})
}

func normalizeCredentialBindingKind(raw string) (string, bool) {
	switch kind := strings.TrimSpace(raw); kind {
	case store.CredentialBindingKindMCPBearer, store.CredentialBindingKindRESTAPIKey, store.CredentialBindingKindRESTBearer:
		return kind, true
	}
	return "", false
}

// normalizeCredentialBindingHost applies the ADR section 4 grammar: lowercase ASCII DNS name, no
// trailing dot, no IP literal, no wildcard, no userinfo, port 443 only. It returns the bare host
// (an explicit ":443" is accepted and dropped). It does not resolve the host.
func normalizeCredentialBindingHost(raw string) (string, bool) {
	host := strings.ToLower(strings.TrimSpace(raw))
	host = strings.TrimSuffix(host, ":443")
	if host == "" || len(host) > credentialBindingMaxHostLength || strings.HasSuffix(host, ".") {
		return "", false
	}
	if net.ParseIP(host) != nil {
		return "", false
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return "", false
	}
	for _, label := range labels {
		if label == "" || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return "", false
		}
		for index := 0; index < len(label); index++ {
			ch := label[index]
			if !(ch >= 'a' && ch <= 'z') && !(ch >= '0' && ch <= '9') && ch != '-' {
				return "", false
			}
		}
	}
	// A dotted-decimal address has an all-numeric last label; a real name has a letter in it.
	last := labels[len(labels)-1]
	if strings.Trim(last, "0123456789") == "" {
		return "", false
	}
	return host, true
}

// normalizeCredentialBindingHeader returns the canonical closed-list header. A bearer kind sends
// its token in Authorization; an API-key kind may use any header on the list.
func normalizeCredentialBindingHeader(kind, raw string) (string, bool) {
	name := strings.TrimSpace(raw)
	for _, allowed := range credentialBindingHeaders {
		if !strings.EqualFold(name, allowed) {
			continue
		}
		if kind != store.CredentialBindingKindRESTAPIKey && allowed != "Authorization" {
			return "", false
		}
		return allowed, true
	}
	return "", false
}

func normalizeCredentialBindingMethods(raw []string) ([]string, bool) {
	if len(raw) == 0 || len(raw) > credentialBindingMaxMethods {
		return nil, false
	}
	out := make([]string, 0, len(raw))
	seen := map[string]bool{}
	for _, item := range raw {
		method := strings.ToUpper(strings.TrimSpace(item))
		known := false
		for _, allowed := range credentialBindingMethods {
			if method == allowed {
				known = true
				break
			}
		}
		if !known {
			return nil, false
		}
		if !seen[method] {
			seen[method] = true
			out = append(out, method)
		}
	}
	return out, true
}

func normalizeCredentialBindingPathPrefixes(raw []string) ([]string, bool) {
	if len(raw) == 0 || len(raw) > credentialBindingMaxPathPrefixes {
		return nil, false
	}
	out := make([]string, 0, len(raw))
	seen := map[string]bool{}
	for _, item := range raw {
		prefix := strings.TrimSpace(item)
		if !credentialBindingPathPrefixValid(prefix) {
			return nil, false
		}
		if !seen[prefix] {
			seen[prefix] = true
			out = append(out, prefix)
		}
	}
	return out, true
}

// credentialBindingPathPrefixValid accepts an absolute path of visible ASCII with no query,
// fragment, percent-encoding, backslash, empty segment or dot segment: a prefix that is matched
// literally cannot be walked out of with an encoded or relative segment.
func credentialBindingPathPrefixValid(prefix string) bool {
	if len(prefix) == 0 || len(prefix) > credentialBindingMaxPathPrefixLen || prefix[0] != '/' {
		return false
	}
	for index := 0; index < len(prefix); index++ {
		ch := prefix[index]
		if ch < 0x21 || ch > 0x7e || ch == '?' || ch == '#' || ch == '%' || ch == '\\' {
			return false
		}
	}
	if strings.Contains(prefix, "//") {
		return false
	}
	for _, segment := range strings.Split(prefix, "/") {
		if segment == "." || segment == ".." {
			return false
		}
	}
	return true
}

// credentialBindingSecretValid reports whether a secret could be sent as one header value: visible
// ASCII, no whitespace or control characters, bounded.
func credentialBindingSecretValid(secret string) bool {
	if secret == "" || len(secret) > credentialBindingMaxSecretBytes {
		return false
	}
	for index := 0; index < len(secret); index++ {
		if secret[index] < 0x21 || secret[index] > 0x7e {
			return false
		}
	}
	return true
}
