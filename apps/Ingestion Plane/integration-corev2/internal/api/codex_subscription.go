package api

import (
	"errors"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
)

// registerCodexSubscriptionRoutes exposes a user-owned device-code connection
// flow and a Model-Plane-only inference route. It deliberately offers no token
// lease: ChatGPT subscription credentials stay managed by Codex app-server.
func registerCodexSubscriptionRoutes(
	app *fiber.App,
	cfg ServerConfig,
	modelPlaneAuth fiber.Handler,
	internalOrBearerAuth fiber.Handler,
	rateLimited []fiber.Handler,
) {
	if cfg.CodexSubscriptions == nil {
		return
	}
	const base = "/api/v1/model-subscriptions/openai-codex"

	app.Post(base+"/connect", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body codexConnectBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if err := normalizeCodexConnectIdentity(c, &body); err != nil {
			return authAwareError(c, err)
		}
		connection := store.Connection{
			ID:              "conn_" + uuid.NewString(),
			ProviderKey:     codexsubscription.ProviderKey,
			ConnectorType:   codexsubscription.ConnectorType,
			OrganizationID:  body.OrganizationID,
			WorkspaceID:     body.WorkspaceID,
			UserID:          body.UserID,
			UserEmail:       body.UserEmail,
			Status:          "pending",
			DisplayName:     "ChatGPT subscription (Codex)",
			ProviderContext: map[string]string{"auth_mode": "chatgpt_device_code"},
			Capabilities:    []string{codexsubscription.Capability},
		}
		if err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var err error
			connection, err = tx.UpsertConnection(c.UserContext(), connection)
			if err != nil {
				return err
			}
			return persist(store.AuditEvent{
				ID:             auditMutationID(c.UserContext(), "codex-subscription-connect-start", connection.ID),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "subscription_connection.login_started",
				ProviderKey:    connection.ProviderKey,
				Metadata:       map[string]any{"authMode": "chatgpt_device_code"},
			})
		}); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "connection_persist_failed", "Could not begin the subscription connection safely.")
		}
		login, err := cfg.CodexSubscriptions.StartLogin(c.UserContext(), connection.ID)
		if err != nil {
			_, _ = cfg.Repo.MarkConnectionDeleted(c.UserContext(), connection.ID)
			return subscriptionError(c, err)
		}
		return c.Status(fiber.StatusAccepted).JSON(fiber.Map{"success": true, "data": fiber.Map{"connection": connection, "login": login}})
	})...)

	app.Get(base+"/connect/:connectionId/:loginId", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedCodexSubscriptionConnection(c, cfg, c.Params("connectionId"), true)
		if err != nil {
			return subscriptionError(c, err)
		}
		login, err := cfg.CodexSubscriptions.PollLogin(c.UserContext(), c.Params("loginId"))
		if err != nil {
			if errors.Is(err, codexsubscription.ErrLoginExpired) {
				markCodexSubscriptionLoginTerminal(c, cfg, connection, "subscription_connection.login_expired")
			}
			return subscriptionError(c, err)
		}
		if login.ConnectionID != connection.ID {
			return apiError(c, fiber.StatusForbidden, "login_connection_mismatch", "The sign-in session does not belong to this connection.")
		}
		if login.Status == "connected" {
			connection.Status = "active"
			connection.LastSyncStatus = "connected"
			if persistErr := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
				var saveErr error
				connection, saveErr = tx.UpsertConnection(c.UserContext(), connection)
				if saveErr != nil {
					return saveErr
				}
				return persist(store.AuditEvent{
					ID:             auditMutationID(c.UserContext(), "codex-subscription-connected", connection.ID),
					OrganizationID: connection.OrganizationID,
					UserID:         connection.UserID,
					ConnectionID:   connection.ID,
					EventType:      "subscription_connection.connected",
					ProviderKey:    connection.ProviderKey,
					Metadata:       map[string]any{"authMode": "chatgpt_device_code"},
				})
			}); persistErr != nil {
				return apiError(c, fiber.StatusServiceUnavailable, "connection_persist_failed", "ChatGPT sign-in completed, but the connection could not be recorded safely.")
			}
			publishIntegrationEvent(c.UserContext(), cfg, "verevon.ingestion.integration.connection_updated", connection, map[string]any{"change": "subscription_connected"})
		} else if login.Status == "failed" {
			markCodexSubscriptionLoginTerminal(c, cfg, connection, "subscription_connection.login_failed")
		}
		return success(c, fiber.Map{"connection": connection, "login": login})
	})

	app.Delete(base+"/connections/:id", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedCodexSubscriptionConnection(c, cfg, c.Params("id"), true)
		if err != nil {
			return subscriptionError(c, err)
		}
		if err := cfg.CodexSubscriptions.Logout(c.UserContext(), connection.ID); err != nil {
			return subscriptionError(c, err)
		}
		if err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var deleteErr error
			connection, deleteErr = tx.MarkConnectionDeleted(c.UserContext(), connection.ID)
			if deleteErr != nil {
				return deleteErr
			}
			return persist(store.AuditEvent{
				ID:             auditMutationID(c.UserContext(), "codex-subscription-disconnected", connection.ID),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "subscription_connection.disconnected",
				ProviderKey:    connection.ProviderKey,
			})
		}); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "disconnect_persist_failed", "The subscription was signed out, but disconnect could not be recorded safely.")
		}
		return c.SendStatus(fiber.StatusNoContent)
	})

	app.Post("/internal/model-subscriptions/openai-codex/infer", chainHandlers(rateLimited, modelPlaneAuth, func(c *fiber.Ctx) error {
		var body codexSubscriptionInferBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if strings.TrimSpace(body.OrganizationID) == "" || strings.TrimSpace(body.UserID) == "" || strings.TrimSpace(body.ConnectionID) == "" || strings.TrimSpace(body.Model) == "" || len(body.Messages) == 0 {
			return apiError(c, fiber.StatusBadRequest, "invalid_subscription_request", "organizationId, userId, connectionId, model, and messages are required.")
		}
		connection, err := cfg.Repo.GetConnection(c.UserContext(), strings.TrimSpace(body.ConnectionID))
		if err != nil {
			return subscriptionError(c, err)
		}
		if connection.ProviderKey != codexsubscription.ProviderKey || connection.ConnectorType != codexsubscription.ConnectorType || connection.DeletedAt != nil || connection.Status != "active" {
			return apiError(c, fiber.StatusConflict, "subscription_connection_inactive", "The selected ChatGPT subscription connection is not active.")
		}
		if connection.OrganizationID != strings.TrimSpace(body.OrganizationID) || connection.UserID != strings.TrimSpace(body.UserID) {
			return apiError(c, fiber.StatusForbidden, "subscription_connection_scope_mismatch", "The selected subscription connection is not owned by this organization and user.")
		}
		if !hasCapability(connection.Capabilities, codexsubscription.Capability) {
			return apiError(c, fiber.StatusForbidden, "subscription_capability_not_granted", "The subscription connection does not permit inference.")
		}
		if err := withAuditTransaction(c.UserContext(), cfg, func(_ store.AuditTransaction, persist func(store.AuditEvent) error) error {
			return persist(store.AuditEvent{
				ID:             auditMutationID(c.UserContext(), "codex-subscription-infer-requested", connection.ID),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "subscription_inference.requested",
				ProviderKey:    connection.ProviderKey,
				Metadata:       map[string]any{"requestId": strings.TrimSpace(body.RequestID), "model": strings.TrimSpace(body.Model)},
			})
		}); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "subscription_audit_unavailable", "The subscription invocation could not be audited safely.")
		}
		response, err := cfg.CodexSubscriptions.Invoke(c.UserContext(), codexsubscription.InvokeRequest{
			ConnectionID: connection.ID,
			RequestID:    strings.TrimSpace(body.RequestID),
			Model:        strings.TrimSpace(body.Model),
			Messages:     body.Messages,
			MaxTokens:    body.MaxTokens,
		})
		if err != nil {
			return subscriptionError(c, err)
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "codex-subscription-infer-completed", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "subscription_inference.completed",
			ProviderKey:    connection.ProviderKey,
			Metadata:       map[string]any{"requestId": response.RequestID, "model": response.ModelUsed},
		})
		return success(c, fiber.Map{"response": response, "providerUsed": codexsubscription.ProviderKey})
	})...)
}

// markCodexSubscriptionLoginTerminal prevents abandoned device-code attempts
// from appearing as reusable connections. Provider diagnostics are never saved.
func markCodexSubscriptionLoginTerminal(c *fiber.Ctx, cfg ServerConfig, connection store.Connection, eventType string) {
	_ = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		updated, err := tx.MarkConnectionDeleted(c.UserContext(), connection.ID)
		if err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), eventType, updated.ID),
			OrganizationID: updated.OrganizationID,
			UserID:         updated.UserID,
			ConnectionID:   updated.ID,
			EventType:      eventType,
			ProviderKey:    updated.ProviderKey,
		})
	})
}

type codexConnectBody struct {
	OrganizationID string `json:"organizationId"`
	WorkspaceID    string `json:"workspaceId"`
	UserID         string `json:"userId"`
	UserEmail      string `json:"userEmail"`
}

func normalizeCodexConnectIdentity(c *fiber.Ctx, body *codexConnectBody) error {
	if auth.IsInternalCall(c) {
		if strings.TrimSpace(body.OrganizationID) == "" || strings.TrimSpace(body.UserID) == "" {
			return auth.NewError(fiber.StatusBadRequest, "identity_required", "organizationId and userId are required for an internal connection request")
		}
		return nil
	}
	principal, ok := auth.PrincipalFromContext(c)
	if !ok {
		return auth.NewError(fiber.StatusUnauthorized, "unauthorized", "Authentication required")
	}
	if strings.TrimSpace(body.OrganizationID) == "" {
		body.OrganizationID = principal.OrganizationID
	}
	if err := auth.AssertOrgAccess(c, body.OrganizationID); err != nil {
		return err
	}
	body.UserID = principal.UserID
	if strings.TrimSpace(body.WorkspaceID) == "" {
		body.WorkspaceID = principal.WorkspaceID
	}
	if strings.TrimSpace(principal.Email) != "" {
		body.UserEmail = principal.Email
	}
	return nil
}

func scopedCodexSubscriptionConnection(c *fiber.Ctx, cfg ServerConfig, connectionID string, requireOwner bool) (store.Connection, error) {
	connection, err := cfg.Repo.GetConnection(c.UserContext(), strings.TrimSpace(connectionID))
	if err != nil {
		return store.Connection{}, err
	}
	if connection.ProviderKey != codexsubscription.ProviderKey || connection.ConnectorType != codexsubscription.ConnectorType || connection.DeletedAt != nil {
		return store.Connection{}, store.ErrNotFound
	}
	if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
		return store.Connection{}, err
	}
	if requireOwner && !auth.IsInternalCall(c) {
		principal, ok := auth.PrincipalFromContext(c)
		if !ok || principal.UserID != connection.UserID {
			return store.Connection{}, auth.NewError(fiber.StatusForbidden, "forbidden", "Only the user who connected this subscription can manage it")
		}
	}
	return connection, nil
}

type codexSubscriptionInferBody struct {
	OrganizationID string                          `json:"organizationId"`
	UserID         string                          `json:"userId"`
	ConnectionID   string                          `json:"connectionId"`
	RequestID      string                          `json:"requestId"`
	Model          string                          `json:"model"`
	Messages       []codexsubscription.ChatMessage `json:"messages"`
	MaxTokens      int                             `json:"maxTokens"`
}

func subscriptionError(c *fiber.Ctx, err error) error {
	switch {
	case errors.Is(err, store.ErrNotFound):
		return apiError(c, fiber.StatusNotFound, "subscription_connection_not_found", "The requested ChatGPT subscription connection was not found.")
	case errors.Is(err, codexsubscription.ErrDisabled):
		return apiError(c, fiber.StatusServiceUnavailable, "subscription_connections_disabled", "ChatGPT subscription connections are not enabled.")
	case errors.Is(err, codexsubscription.ErrLoginNotFound):
		return apiError(c, fiber.StatusNotFound, "subscription_login_not_found", "The subscription sign-in session was not found.")
	case errors.Is(err, codexsubscription.ErrLoginExpired):
		return apiError(c, fiber.StatusGone, "subscription_login_expired", "The subscription sign-in session expired. Start a new connection.")
	case errors.Is(err, codexsubscription.ErrInvalidRequest):
		return apiError(c, fiber.StatusBadRequest, "invalid_subscription_request", "The subscription request is invalid.")
	default:
		var authErr auth.Error
		if errors.As(err, &authErr) {
			return authAwareError(c, authErr)
		}
		return apiError(c, fiber.StatusBadGateway, "subscription_broker_failed", "The ChatGPT subscription broker could not complete the request.")
	}
}
