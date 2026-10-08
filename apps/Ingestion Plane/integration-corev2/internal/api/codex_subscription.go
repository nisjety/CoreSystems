package api

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"strconv"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
)

// registerCodexSubscriptionRoutes exposes a user-owned device-code connection
// flow and a Model-Plane-only inference route. It deliberately offers no token
// lease: ChatGPT subscription credentials stay inside Integration Core.
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
	const allowanceReadTimeout = 20 * time.Second

	app.Get(base+"/connections/:id/usage", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedCodexSubscriptionConnection(c, cfg, c.Params("id"), true)
		if err != nil {
			return subscriptionError(c, err)
		}
		if status := strings.ToLower(strings.TrimSpace(connection.Status)); status != "active" && status != "connected" {
			return apiError(c, fiber.StatusConflict, "subscription_not_active", "Reconnect the ChatGPT subscription before checking its Codex allowance.")
		}
		// Fiber's UserContext is not necessarily canceled when a client disconnects.
		// Bound credential-lock waits and app-server startup here so a stalled
		// provider process cannot hold this request open past the gateway timeout.
		ctx, cancel := context.WithTimeout(c.UserContext(), allowanceReadTimeout)
		defer cancel()
		report, err := cfg.CodexSubscriptions.ReadRateLimits(ctx, connection.ID)
		if err != nil {
			return subscriptionError(c, err)
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "codex-subscription-usage-read", connection.ID),
			OrganizationID: connection.OrganizationID, UserID: connection.UserID, ConnectionID: connection.ID,
			EventType: "subscription_usage.read", ProviderKey: connection.ProviderKey,
			Metadata: map[string]any{"source": "provider_rate_limits"},
		})
		return success(c, fiber.Map{"connectionId": connection.ID, "scope": "connected_user", "source": "provider_rate_limits", "usage": report})
	})...)

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
			// Assess the connection for ZDR in this same transaction: a subscription is never
			// created without its capability record, and a failure here rolls it back.
			if _, _, err := applyConnectionZDR(c.UserContext(), tx, persist, connection); err != nil {
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
				// Assess again now that the sign-in is complete. Usually this changes nothing; a
				// catalog change between start and connected moves the digest here, atomically
				// with the connection becoming active.
				if _, _, assessErr := applyConnectionZDR(c.UserContext(), tx, persist, connection); assessErr != nil {
					return assessErr
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
		connection, err := validateCodexSubscriptionInference(c, cfg, body)
		if err != nil {
			return subscriptionError(c, err)
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
		response, err := cfg.CodexSubscriptions.Invoke(c.UserContext(), codexSubscriptionInvokeRequest(connection, body))
		if err != nil {
			if errors.Is(err, codexsubscription.ErrReauthenticationRequired) {
				markCodexSubscriptionNeedsRefresh(c, cfg, connection)
			}
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

	app.Post("/internal/model-subscriptions/openai-codex/infer/stream", chainHandlers(rateLimited, modelPlaneAuth, func(c *fiber.Ctx) error {
		var body codexSubscriptionInferBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		connection, err := validateCodexSubscriptionInference(c, cfg, body)
		if err != nil {
			return subscriptionError(c, err)
		}
		if err := withAuditTransaction(c.UserContext(), cfg, func(_ store.AuditTransaction, persist func(store.AuditEvent) error) error {
			return persist(store.AuditEvent{
				ID:             auditMutationID(c.UserContext(), "codex-subscription-infer-stream-requested", connection.ID),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "subscription_inference.requested",
				ProviderKey:    connection.ProviderKey,
				Metadata:       map[string]any{"requestId": strings.TrimSpace(body.RequestID), "model": strings.TrimSpace(body.Model), "stream": true},
			})
		}); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "subscription_audit_unavailable", "The subscription invocation could not be audited safely.")
		}

		request := codexSubscriptionInvokeRequest(connection, body)
		streamContext := c.UserContext()
		// Validation and the concurrency queue run before the 200 and the
		// ready event are committed, so a full queue is a 429 with
		// Retry-After, exactly as on the unary route.
		run, release, err := cfg.CodexSubscriptions.ReserveStream(streamContext, request)
		if err != nil {
			return subscriptionError(c, err)
		}
		c.Set("Content-Type", "text/event-stream")
		c.Set("Cache-Control", "no-cache, no-transform")
		c.Set("Connection", "keep-alive")
		c.Set("X-Accel-Buffering", "no")
		c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
			defer release()
			writeSSE(w, "subscription.ready", codexSubscriptionStreamEvent{Type: "ready", RequestID: request.RequestID, ModelUsed: request.Model})
			if err := w.Flush(); err != nil {
				return
			}

			response, invokeErr := run(func(delta string) error {
				writeSSE(w, "subscription.delta", codexSubscriptionStreamEvent{Type: "delta", Delta: delta})
				return w.Flush()
			})
			release() // the turn is over; the deferred call is then a no-op
			if invokeErr != nil {
				if errors.Is(invokeErr, codexsubscription.ErrReauthenticationRequired) {
					markCodexSubscriptionNeedsRefreshContext(streamContext, cfg, connection)
				}
				// The 200 is already sent, so no Retry-After can follow: the
				// event carries the same content-free fields as the unary
				// error body. No plan type or upstream text is returned.
				writeSSE(w, "subscription.error", subscriptionFailureStreamEvent(codexsubscription.ClassifyFailure(invokeErr, time.Now())))
				_ = w.Flush()
				return
			}

			recordAuditEvent(streamContext, cfg, store.AuditEvent{
				ID:             auditMutationID(streamContext, "codex-subscription-infer-stream-completed", connection.ID),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "subscription_inference.completed",
				ProviderKey:    connection.ProviderKey,
				Metadata:       map[string]any{"requestId": response.RequestID, "model": response.ModelUsed, "stream": true},
			})
			writeSSE(w, "subscription.done", codexSubscriptionStreamEvent{
				Type:      "done",
				RequestID: response.RequestID,
				ModelUsed: response.ModelUsed,
				Usage:     response.Usage,
				PhasesMs:  response.PhasesMs,
			})
			_ = w.Flush()
		})
		return nil
	})...)
}

func validateCodexSubscriptionInference(c *fiber.Ctx, cfg ServerConfig, body codexSubscriptionInferBody) (store.Connection, error) {
	if strings.TrimSpace(body.OrganizationID) == "" || strings.TrimSpace(body.UserID) == "" || strings.TrimSpace(body.ConnectionID) == "" || strings.TrimSpace(body.Model) == "" || len(body.Messages) == 0 {
		return store.Connection{}, auth.NewError(fiber.StatusBadRequest, "invalid_subscription_request", "organizationId, userId, connectionId, model, and messages are required")
	}
	connection, err := cfg.Repo.GetConnection(c.UserContext(), strings.TrimSpace(body.ConnectionID))
	if err != nil {
		return store.Connection{}, err
	}
	subscription := connection.ProviderKey == codexsubscription.ProviderKey && connection.ConnectorType == codexsubscription.ConnectorType && connection.DeletedAt == nil
	if subscription && connection.Status == "needs_refresh" {
		// The sign-in already needs renewing: say so before any turn runs.
		return store.Connection{}, auth.NewError(fiber.StatusConflict, string(codexsubscription.FailureReauthRequired), "Reconnect your ChatGPT subscription in Integrations.")
	}
	if !subscription || connection.Status != "active" {
		return store.Connection{}, auth.NewError(fiber.StatusConflict, "subscription_connection_inactive", "The selected ChatGPT subscription connection is not active")
	}
	if connection.OrganizationID != strings.TrimSpace(body.OrganizationID) || connection.UserID != strings.TrimSpace(body.UserID) {
		return store.Connection{}, auth.NewError(fiber.StatusForbidden, "subscription_connection_scope_mismatch", "The selected subscription connection is not owned by this organization and user")
	}
	if !hasCapability(connection.Capabilities, codexsubscription.Capability) {
		return store.Connection{}, auth.NewError(fiber.StatusForbidden, "subscription_capability_not_granted", "The subscription connection does not permit inference")
	}
	return connection, nil
}

func codexSubscriptionInvokeRequest(connection store.Connection, body codexSubscriptionInferBody) codexsubscription.InvokeRequest {
	return codexsubscription.InvokeRequest{
		ConnectionID:    connection.ID,
		RequestID:       strings.TrimSpace(body.RequestID),
		Model:           strings.TrimSpace(body.Model),
		Messages:        body.Messages,
		MaxTokens:       body.MaxTokens,
		ReasoningEffort: strings.TrimSpace(body.ReasoningEffort),
		ServiceTier:     strings.TrimSpace(body.ServiceTier),
		OutputSchema:    body.OutputSchema,
	}
}

type codexSubscriptionStreamEvent struct {
	Type      string `json:"type"`
	Delta     string `json:"delta,omitempty"`
	RequestID string `json:"requestId,omitempty"`
	ModelUsed string `json:"modelUsed,omitempty"`
	Code      string `json:"code,omitempty"`
	Message   string `json:"message,omitempty"`
	// ResetsAt, RetryAfterMs, WindowMinutes and LimitReachedType ride on the
	// error event only, when known: the same content-free fields as the unary
	// error body.
	ResetsAt         string `json:"resetsAt,omitempty"`
	RetryAfterMs     int64  `json:"retryAfterMs,omitempty"`
	WindowMinutes    int    `json:"windowMinutes,omitempty"`
	LimitReachedType string `json:"limitReachedType,omitempty"`
	// Usage and PhasesMs ride on the done event only, with the same meaning
	// as on the unary InvokeResponse: token counts and content-free broker
	// phase durations of the completed turn.
	Usage    *codexsubscription.TokenUsage `json:"usage,omitempty"`
	PhasesMs map[string]int64              `json:"phasesMs,omitempty"`
}

// markCodexSubscriptionNeedsRefresh reconciles database state with the
// service-owned credential volume. This prevents a connection whose local
// credential disappeared from continuing to look usable in model pickers.
func markCodexSubscriptionNeedsRefresh(c *fiber.Ctx, cfg ServerConfig, connection store.Connection) {
	markCodexSubscriptionNeedsRefreshContext(c.UserContext(), cfg, connection)
}

func markCodexSubscriptionNeedsRefreshContext(ctx context.Context, cfg ServerConfig, connection store.Connection) {
	connection.Status = "needs_refresh"
	connection.LastSyncStatus = "reauthentication_required"
	if err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		updated, err := tx.UpsertConnection(ctx, connection)
		if err != nil {
			return err
		}
		connection = updated
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "codex-subscription-needs-refresh", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "subscription_connection.reauthentication_required",
			ProviderKey:    connection.ProviderKey,
		})
	}); err == nil {
		publishIntegrationEvent(ctx, cfg, "verevon.ingestion.integration.connection_updated", connection, map[string]any{"change": "subscription_reauthentication_required"})
	}
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
	OrganizationID  string                          `json:"organizationId"`
	UserID          string                          `json:"userId"`
	ConnectionID    string                          `json:"connectionId"`
	RequestID       string                          `json:"requestId"`
	Model           string                          `json:"model"`
	Messages        []codexsubscription.ChatMessage `json:"messages"`
	MaxTokens       int                             `json:"maxTokens"`
	ReasoningEffort string                          `json:"reasoningEffort"`
	ServiceTier     string                          `json:"serviceTier"`
	OutputSchema    json.RawMessage                 `json:"outputSchema,omitempty"`
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
	}
	var authErr auth.Error
	if errors.As(err, &authErr) {
		return authAwareError(c, authErr)
	}
	return subscriptionFailureError(c, codexsubscription.ClassifyFailure(err, time.Now()))
}

// subscriptionFailureStatus is the unary HTTP status of each public failure
// code.
func subscriptionFailureStatus(code codexsubscription.FailureCode) int {
	switch code {
	case codexsubscription.FailureUsageLimit, codexsubscription.FailureRateLimited:
		return fiber.StatusTooManyRequests
	case codexsubscription.FailureReauthRequired:
		return fiber.StatusConflict
	case codexsubscription.FailureUpstreamUnavailable:
		return fiber.StatusServiceUnavailable
	case codexsubscription.FailureContextTooLong:
		return fiber.StatusRequestEntityTooLarge
	case codexsubscription.FailureBadRequest:
		return fiber.StatusUnprocessableEntity
	}
	return fiber.StatusBadGateway
}

// subscriptionFailureMessage is the fixed English message of each code. It is
// never derived from upstream text.
//
// A usage limit with neither a reset nor a retry hint may also be a plan
// without Codex (or with its credits spent): the pinned Codex app-server
// reports all of them as usageLimitExceeded, so the message names both. rate_limited covers the broker's own concurrency queue and
// Integration Core's request limiter as well as a short ChatGPT throttle, so
// its message does not blame ChatGPT.
func subscriptionFailureMessage(failure codexsubscription.Failure, streamed bool) string {
	switch failure.Code {
	case codexsubscription.FailureUsageLimit:
		if failure.ResetsAt.IsZero() && failure.RetryAfter <= 0 {
			return "The connected ChatGPT plan has reached its Codex usage limit, or does not include Codex. Check the plan in ChatGPT."
		}
		return "The connected ChatGPT plan has reached its Codex usage limit."
	case codexsubscription.FailureReauthRequired:
		return "Reconnect your ChatGPT subscription in Integrations."
	case codexsubscription.FailureUpstreamUnavailable:
		return "ChatGPT did not answer. Try again shortly."
	case codexsubscription.FailureContextTooLong:
		return "The conversation is too long for the ChatGPT model."
	case codexsubscription.FailureBadRequest:
		return "ChatGPT rejected the request."
	case codexsubscription.FailureRateLimited:
		if streamed {
			return "The ChatGPT subscription is busy. Retry later."
		}
		return "The ChatGPT subscription is busy. Retry after the indicated time."
	}
	if streamed {
		return "The ChatGPT subscription broker could not complete the streamed request."
	}
	return "The ChatGPT subscription broker could not complete the request."
}

// subscriptionFailureDetails are the optional content-free fields of a
// failure, each omitted when unknown: the reset time and window length are
// account metadata, never content, and the plan type is never returned.
func subscriptionFailureDetails(failure codexsubscription.Failure) fiber.Map {
	details := fiber.Map{}
	if resetsAt := failure.FormatResetsAt(); resetsAt != "" {
		details["resetsAt"] = resetsAt
	}
	if ms := failure.RetryAfterMs(); ms > 0 {
		details["retryAfterMs"] = ms
	}
	if failure.WindowMinutes > 0 {
		details["windowMinutes"] = failure.WindowMinutes
	}
	if failure.LimitReachedType != "" {
		details["limitReachedType"] = failure.LimitReachedType
	}
	return details
}

// subscriptionFailureError writes a classified failure. Retry-After is in
// whole seconds: for a usage limit only when its reset (or the upstream's own
// retry hint) is known, for a rate limit always, and for an upstream outage
// only when the upstream sent a hint.
func subscriptionFailureError(c *fiber.Ctx, failure codexsubscription.Failure) error {
	if seconds, ok := failure.RetryAfterSeconds(); ok {
		c.Set(fiber.HeaderRetryAfter, strconv.FormatInt(seconds, 10))
	}
	return apiErrorWithDetails(c, subscriptionFailureStatus(failure.Code), string(failure.Code), subscriptionFailureMessage(failure, false), subscriptionFailureDetails(failure))
}

// subscriptionFailureStreamEvent is the subscription.error event of a
// classified failure after the stream's 200.
func subscriptionFailureStreamEvent(failure codexsubscription.Failure) codexSubscriptionStreamEvent {
	return codexSubscriptionStreamEvent{
		Type:             "error",
		Code:             string(failure.Code),
		Message:          subscriptionFailureMessage(failure, true),
		ResetsAt:         failure.FormatResetsAt(),
		RetryAfterMs:     failure.RetryAfterMs(),
		WindowMinutes:    failure.WindowMinutes,
		LimitReachedType: failure.LimitReachedType,
	}
}
