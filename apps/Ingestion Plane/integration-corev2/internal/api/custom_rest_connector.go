package api

import (
	"errors"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Custom REST connection creation (dark, CUSTOM_REST_CONNECTORS_ENABLED). W4-CC-8.
//
// An organisation admin creates a connection of provider custom_rest that calls through exactly one
// of the organisation's credential bindings (W4-CC-3). The connection holds the binding's id in its
// providerContext and nothing else: no secret, no host, no method list. The host, the methods and
// the path prefixes stay the binding's, and the secret is opened for one call at a time by
// custom_rest.call (custom_rest.go).
//
// The route is mounted only while the switch is on, so off it answers 404. It is bearer-only: the
// internal key and a service principal never create a connection, because a connection that can
// call an organisation's own API is chosen by a person. A new connection grants no capability
// unless the request names custom_rest.read or custom_rest.write explicitly; the default grants
// none, and custom_rest.call refuses a connection that lacks the capability its method needs.
//
// Zero data retention: an organisation that uses it cannot create the connection, because the
// connection is standing state that fronts stored credentials. The posture is read from org-core
// here, never taken from the request body, and an unreadable posture is a refusal. The refusal is
// made before the body is read and before anything is written.

// auditEventCustomRESTConnectionCreated is written once per created custom_rest connection.
// Content-free: ids and the granted capability names only, never a host, header, name or secret.
const auditEventCustomRESTConnectionCreated = "connection.custom_rest_created"

const (
	customRESTConnectorPath = "/api/v1/connectors/custom-rest"
	// customRESTConnectorType is the connector type stored on a custom_rest connection.
	customRESTConnectorType = "custom_rest"
	// customRESTAuthMode is the providerContext auth_mode of a custom_rest connection: the credential
	// is a binding, not an OAuth token.
	customRESTAuthMode = "credential_binding"

	customRESTMaxNameChars      = 120
	customRESTMaxBindingIDBytes = 128
	customRESTMaxCapabilities   = 8
)

// customRESTConnectorBody is the request. The browser gateway sends the name and the binding id; a
// caller may also name the capabilities to grant. The organisation always comes from the verified
// caller, never from the body.
type customRESTConnectorBody struct {
	Name                string   `json:"name"`
	CredentialBindingID string   `json:"credential_binding_id"`
	Capabilities        []string `json:"capabilities"`
}

// Refusals raised inside the create transaction and mapped to HTTP answers after it rolls back.
var (
	errCustomRESTConnectorBindingMissing  = errors.New("credential binding not found")
	errCustomRESTConnectorBindingInactive = errors.New("credential binding is not active")
	errCustomRESTConnectorBindingKind     = errors.New("credential binding is not a REST credential")
)

// registerCustomRESTConnectorRoutes mounts the creation route (an org admin's bearer token only).
// Nothing is mounted unless the switch is on and the credential-binding store can seal and open
// secrets, so while the switch is off the path answers 404.
func registerCustomRESTConnectorRoutes(
	app *fiber.App,
	cfg ServerConfig,
	bearerOnly fiber.Handler,
	rateLimited []fiber.Handler,
) {
	if !cfg.Config.CustomRESTConnectorsEnabled || !cfg.Config.CredentialBindingsEnabled || cfg.CredentialSealer == nil {
		return
	}
	app.Post(customRESTConnectorPath, chainHandlers(rateLimited, bearerOnly, func(c *fiber.Ctx) error {
		return customRESTConnectorCreate(c, cfg)
	})...)
}

// customRESTConnectorCreate validates the request, checks the binding, and stores the connection
// with its audit row in one transaction.
func customRESTConnectorCreate(c *fiber.Ctx, cfg ServerConfig) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || principal.PrincipalType != "user" ||
		strings.TrimSpace(principal.OrganizationID) == "" || strings.TrimSpace(principal.UserID) == "" {
		return apiError(c, fiber.StatusUnauthorized, "unauthorized", "A verified user is required.")
	}
	if !zdrHasOrgAdminRole(principal.Role) {
		return apiError(c, fiber.StatusForbidden, "org_admin_required", "Only an organisation owner or admin can create a custom REST connector.")
	}
	if err := auth.AssertOrgAccess(c, principal.OrganizationID); err != nil {
		return authAwareError(c, err)
	}
	if cfg.Repo == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The connection store is unavailable.")
	}
	// ZDR is checked before the body is read and before any write. An unreadable posture is a
	// refusal, never a pass.
	if cfg.OrgZDR == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was stored.")
	}
	zdrOn, err := cfg.OrgZDR.EffectiveZDR(c.UserContext(), principal.OrganizationID)
	if err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was stored.")
	}
	if zdrOn {
		return apiError(c, fiber.StatusConflict, "custom_rest_zdr_refused", "A custom REST connector cannot be created while the organisation uses zero data retention.")
	}

	var body customRESTConnectorBody
	if err := c.BodyParser(&body); err != nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
	}
	name := strings.TrimSpace(body.Name)
	if name == "" || utf8.RuneCountInString(name) > customRESTMaxNameChars {
		return apiError(c, fiber.StatusBadRequest, "invalid_connector_name", "A connector name of 1 to 120 characters is required.")
	}
	bindingID := strings.TrimSpace(body.CredentialBindingID)
	if !strings.HasPrefix(bindingID, credentialBindingIDPrefix) || len(bindingID) > customRESTMaxBindingIDBytes || !validCustomerGatewayConnectionID(bindingID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_binding_id", "A credential binding id is required.")
	}
	capabilities, ok := normalizeCustomRESTCapabilities(body.Capabilities)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_capabilities", "capabilities may list custom_rest.read and custom_rest.write only.")
	}

	connectionID := "conn_" + uuid.NewString()
	var connection store.Connection
	if err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		// The binding is read inside the transaction, scoped to the caller's organisation: an id
		// from another organisation is simply not found. Metadata only, never the ciphertext.
		binding, readErr := tx.GetCredentialBinding(c.UserContext(), principal.OrganizationID, bindingID)
		if readErr != nil {
			if errors.Is(readErr, store.ErrNotFound) {
				return errCustomRESTConnectorBindingMissing
			}
			return readErr
		}
		if binding.OrganizationID != principal.OrganizationID {
			return errCustomRESTConnectorBindingMissing
		}
		if binding.Status != store.CredentialBindingStatusActive {
			return errCustomRESTConnectorBindingInactive
		}
		if binding.Kind != store.CredentialBindingKindRESTAPIKey && binding.Kind != store.CredentialBindingKindRESTBearer {
			return errCustomRESTConnectorBindingKind
		}
		var saveErr error
		connection, saveErr = tx.UpsertConnection(c.UserContext(), store.Connection{
			ID:             connectionID,
			ProviderKey:    actions.CustomRESTProviderKey,
			ConnectorType:  customRESTConnectorType,
			OrganizationID: principal.OrganizationID,
			WorkspaceID:    principal.WorkspaceID,
			UserID:         principal.UserID,
			UserEmail:      principal.Email,
			Status:         "active",
			DisplayName:    name,
			ProviderContext: map[string]string{
				"auth_mode":                         customRESTAuthMode,
				actions.CustomRESTBindingContextKey: binding.ID,
			},
			Capabilities:   capabilities,
			LastSyncStatus: "connected",
		})
		if saveErr != nil {
			return saveErr
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "custom-rest-connection-created", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         principal.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventCustomRESTConnectionCreated,
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"bindingId":    binding.ID,
				"capabilities": capabilities,
			},
		})
	}); err != nil {
		switch {
		case errors.Is(err, errCustomRESTConnectorBindingMissing):
			return apiError(c, fiber.StatusNotFound, "credential_binding_not_found", "The credential binding was not found.")
		case errors.Is(err, errCustomRESTConnectorBindingInactive):
			return apiError(c, fiber.StatusConflict, "credential_binding_inactive", "The credential binding is not active.")
		case errors.Is(err, errCustomRESTConnectorBindingKind):
			return apiError(c, fiber.StatusConflict, "credential_binding_kind_unsupported", "The credential binding must be a rest_api_key or rest_bearer binding.")
		}
		return apiError(c, fiber.StatusServiceUnavailable, "connection_persist_failed", "Could not create the connector safely.")
	}
	publishIntegrationEvent(c.UserContext(), cfg, integrationEventConnUpd, connection, map[string]any{"change": "custom_rest_created"})
	// Only the id, the name and the grants are echoed: no binding id, host, secret or sealed value.
	return c.Status(fiber.StatusCreated).JSON(fiber.Map{"success": true, "data": fiber.Map{
		"connection_id": connection.ID,
		"name":          connection.DisplayName,
		"capabilities":  connection.Capabilities,
	}})
}

// normalizeCustomRESTCapabilities returns the explicitly named grants, de-duplicated and sorted. An
// empty or absent list grants nothing. A name other than custom_rest.read or custom_rest.write is
// refused, so a request cannot grant a capability of another provider.
func normalizeCustomRESTCapabilities(raw []string) ([]string, bool) {
	if len(raw) > customRESTMaxCapabilities {
		return nil, false
	}
	seen := map[string]struct{}{}
	granted := []string{}
	for _, item := range raw {
		capability := strings.TrimSpace(item)
		if capability != actions.CustomRESTCapabilityRead && capability != actions.CustomRESTCapabilityWrite {
			return nil, false
		}
		if _, dup := seen[capability]; dup {
			continue
		}
		seen[capability] = struct{}{}
		granted = append(granted, capability)
	}
	sort.Strings(granted)
	return granted, true
}
