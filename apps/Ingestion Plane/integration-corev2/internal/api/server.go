package api

import (
	"bufio"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"html"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/limiter"
	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/controlplane"
	"github.com/triodelab/integration-corev2/internal/discovery"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/hotpath"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/webhookorg"
)

type ServerConfig struct {
	Config            config.Config
	Repo              store.Repository
	OAuth             *oauth.Service
	Auth              auth.TokenVerifier
	Org               auth.OrgPlanClient
	Billing           *controlplane.BillingClient
	Audit             *controlplane.AuditClient
	AuditOutbox       AuditDispatcher
	Events            events.Publisher
	Discovery         *discovery.Service
	Actions           *actions.Service
	WriteAttestations *attestation.Verifier
	HotPath           hotpath.WebhookNormalizer
	// WebhookOrg resolves the owning tenant for account-wide provider
	// webhooks (Meta/Slack callbacks carry no Velion org id). Nil-safe.
	WebhookOrg *webhookorg.Resolver
	Logger     *zerolog.Logger
}

const requestIDHeader = "X-Request-ID"

type requestIDContextKey struct{}

func NewServer(cfg ServerConfig) *fiber.App {
	if cfg.WriteAttestations == nil && strings.TrimSpace(cfg.Config.ProviderWriteAttestationKeysJSON) != "" {
		if keys, err := attestation.ParseTrustedKeysJSON(cfg.Config.ProviderWriteAttestationKeysJSON); err == nil {
			cfg.WriteAttestations = attestation.NewVerifier(keys, nil)
		}
	}
	app := fiber.New(fiber.Config{
		DisableStartupMessage: true,
		// OAuth callbacks arrive from a browser whose `localhost` cookie jar is
		// shared across every dev port (SPA, gateway, auth) and easily exceeds
		// fasthttp's 4 KiB default ReadBufferSize — which made Fiber reject the
		// provider redirect with 431 "Request Header Fields Too Large" and broke
		// every connect flow. 64 KiB matches common proxy header limits.
		ReadBufferSize: 64 * 1024,
	})
	app.Use(requestIDMiddleware)
	metrics := newRequestMetrics()
	auditMonitor, _ := cfg.AuditOutbox.(AuditOutboxMonitor)
	app.Use(metrics.middleware)
	if cfg.Logger != nil {
		app.Use(requestLogger(*cfg.Logger))
	}

	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":  "ok",
			"service": cfg.Config.ServiceName,
		})
	})
	app.Get("/ready", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":  "ok",
			"service": cfg.Config.ServiceName,
			"storage": "ready",
		})
	})
	app.Get("/health/detailed", func(c *fiber.Ctx) error {
		statusCode := fiber.StatusOK
		response := fiber.Map{
			"status":      "ok",
			"service":     cfg.Config.ServiceName,
			"environment": cfg.Config.Environment,
			"storage":     "ready",
			"natsEnabled": cfg.Config.NATSEnabled,
			"providers":   len(providers.Catalog()),
			"capabilities": fiber.Map{
				"oauth":          true,
				"tokenBroker":    true,
				"discovery":      cfg.Discovery != nil,
				"actions":        cfg.Actions != nil,
				"webhookHotPath": cfg.HotPath != nil,
			},
		}
		if auditMonitor != nil {
			auditStatus, err := auditMonitor.Status(c.UserContext())
			if err != nil {
				statusCode = fiber.StatusServiceUnavailable
				response["status"] = "degraded"
				response["auditOutbox"] = fiber.Map{"status": "unavailable"}
			} else {
				response["auditOutbox"] = fiber.Map{
					"status":                map[bool]string{true: "degraded", false: "ok"}[auditStatus.Degraded],
					"pending":               auditStatus.Pending,
					"terminal":              auditStatus.Terminal,
					"oldestPendingSeconds":  auditStatus.OldestPendingAge.Seconds(),
					"oldestTerminalSeconds": auditStatus.OldestTerminalAge.Seconds(),
				}
				if auditStatus.Degraded {
					statusCode = fiber.StatusServiceUnavailable
					response["status"] = "degraded"
				}
			}
		}
		return c.Status(statusCode).JSON(response)
	})

	internalAuth := auth.InternalOnly(auth.Config{
		APIKey:       cfg.Config.InternalAPIKey,
		APIKeyHeader: cfg.Config.InternalAPIKeyHeader,
	})
	app.Get("/metrics", internalAuth, func(c *fiber.Ctx) error {
		c.Set("Content-Type", "text/plain; version=0.0.4")
		body := metrics.render()
		if auditMonitor != nil {
			body += renderAuditOutboxMetrics(c.UserContext(), auditMonitor)
		}
		return c.SendString(body)
	})
	app.Post("/internal/audit-outbox/requeue", internalAuth, func(c *fiber.Ctx) error {
		if auditMonitor == nil {
			return apiError(c, fiber.StatusServiceUnavailable, "audit_outbox_unavailable", "Audit outbox recovery is not configured.")
		}
		var body struct {
			EventIDs []string `json:"eventIds"`
		}
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_request", "Request body is invalid.")
		}
		if _, err := validateTerminalAuditEventIDs(body.EventIDs); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_event_ids", err.Error())
		}
		requeued, err := auditMonitor.RequeueTerminal(c.UserContext(), body.EventIDs)
		if err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "audit_outbox_requeue_failed", "Terminal audit events could not be requeued.")
		}
		return success(c, fiber.Map{"requeued": requeued})
	})
	internalOrBearerAuth := auth.InternalOrBearer(auth.Config{
		APIKey:               cfg.Config.InternalAPIKey,
		APIKeyHeader:         cfg.Config.InternalAPIKeyHeader,
		TokenVerifier:        cfg.Auth,
		AllowLegacyTenantKey: cfg.Config.AllowLegacyTenantKey,
	})
	proPlanAuth := []fiber.Handler{internalOrBearerAuth, auth.RequirePlan(cfg.Org, "pro")}
	rateLimited := rateLimitHandlers(cfg.Config)

	app.Get("/api/v1/providers", func(c *fiber.Ctx) error {
		catalog := providers.WithReadiness(providers.Catalog(), cfg.Config.ProviderReadiness())
		return success(c, fiber.Map{"providers": attachMetaSDKConfig(catalog, cfg.Config)})
	})

	connectSessionHandler := func(c *fiber.Ctx) error {
		var body createConnectSessionBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if err := normalizeConnectSessionIdentity(c, &body); err != nil {
			return authAwareError(c, err)
		}
		result, err := cfg.OAuth.CreateSession(c.UserContext(), oauth.CreateSessionInput{
			ProviderKey:     c.Params("provider"),
			OrganizationID:  body.OrganizationID,
			WorkspaceID:     body.WorkspaceID,
			UserID:          body.UserID,
			UserEmail:       body.UserEmail,
			SelectedSources: body.SelectedSources,
			Capabilities:    body.Capabilities,
			Bundles:         body.Bundles,
			ReturnURL:       body.ReturnURL,
			ProviderContext: body.providerContext(),
		})
		if err != nil {
			return apiError(c, fiber.StatusBadRequest, "connect_session_failed", err.Error())
		}
		recordUsage(cfg, body.OrganizationID, "connect_session_created", 1, map[string]any{
			"providerKey": c.Params("provider"),
		})
		return success(c, result)
	}
	app.Post("/api/v1/providers/:provider/connect-session", chainHandlers(rateLimited, append(proPlanAuth, connectSessionHandler)...)...)
	app.Post("/api/v1/providers/:provider/connect", chainHandlers(rateLimited, append(proPlanAuth, connectSessionHandler)...)...)
	app.Post("/api/v1/providers/:provider/reconnect-session", chainHandlers(rateLimited, append(proPlanAuth, connectSessionHandler)...)...)

	app.Get("/oauth/callback/:provider", func(c *fiber.Ctx) error {
		result, err := cfg.OAuth.CompleteCallback(
			c.UserContext(),
			c.Params("provider"),
			c.Query("state"),
			c.Query("code"),
			c.Query("error"),
			c.Query("error_description"),
		)
		if err != nil && cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("provider", c.Params("provider")).Msg("oauth callback failed")
		}
		if err == nil && result.Success && result.ConnectionID != "" && isMetaWebhookProvider(result.ProviderKey) {
			connection, connectionErr := cfg.Repo.GetConnection(c.UserContext(), result.ConnectionID)
			if connectionErr != nil {
				result.Success = false
				result.ErrorCode = "meta_connection_unavailable"
				result.Message = "Authorization succeeded, but the connection could not be loaded for inbox setup. Retry reconnect."
			} else if isMetaWebhookProvider(connection.ProviderKey) && hasMetaInboxCapability(connection.Capabilities) {
				if _, syncErr := createSyncJob(c.UserContext(), cfg, connection, syncJobBody{
					Reason: "oauth_connected", Mode: "incremental",
				}); syncErr != nil {
					result.Success = false
					result.ErrorCode = "meta_inbox_provisioning_failed"
					result.Message = "Meta authorized successfully, but inbox setup did not finish. Retry reconnect or Sync."
					if cfg.Logger != nil {
						cfg.Logger.Warn().Err(syncErr).Str("connection_id", connection.ID).Msg("post-oauth Meta inbox provisioning failed")
					}
				}
			}
			if result.Success {
				if finalizeErr := cfg.OAuth.FinalizeConnectedCallback(c.UserContext(), result, connection); finalizeErr != nil {
					result.Success = false
					result.ErrorCode = "meta_callback_finalize_failed"
					result.Message = "Meta inbox setup succeeded, but completion could not be recorded. Retry reconnect."
				}
			}
			if !result.Success {
				_ = cfg.OAuth.FailCallback(c.UserContext(), result.SessionID, result.ErrorCode, result.Message)
			}
		}
		c.Set("Content-Type", "text/html; charset=utf-8")
		return c.SendString(callbackHTML(result))
	})

	app.Get("/api/v1/connect-sessions/:id/status", internalOrBearerAuth, func(c *fiber.Ctx) error {
		session, err := cfg.Repo.GetConnectSessionByID(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connect_session_not_found")
		}
		if err := auth.AssertOrgAccess(c, session.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		return success(c, fiber.Map{
			"id":          session.ID,
			"providerKey": session.ProviderKey,
			"consumedAt":  session.ConsumedAt,
			"expiresAt":   session.ExpiresAt,
			"errorCode":   session.ErrorCode,
			"status":      sessionStatus(session),
		})
	})

	app.Get("/api/v1/connections", internalOrBearerAuth, func(c *fiber.Ctx) error {
		organizationID := strings.TrimSpace(c.Query("organizationId"))
		if !auth.IsInternalCall(c) {
			if principal, ok := auth.PrincipalFromContext(c); ok {
				organizationID = principal.OrganizationID
			}
		}
		connections, err := cfg.Repo.ListConnections(c.UserContext(), store.ConnectionFilter{
			OrganizationID: organizationID,
			ProviderKey:    providers.NormalizeKey(c.Query("providerKey")),
			ConnectorType:  strings.TrimSpace(c.Query("connectorType")),
			UserID:         strings.TrimSpace(c.Query("userId")),
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "connections_list_failed", err.Error())
		}
		connections = filterConnectionsByCategory(connections, firstNonEmpty(c.Query("category"), c.Query("providerCategory")))
		return success(c, fiber.Map{"connections": connections})
	})

	app.Get("/api/v1/connections/:id", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		return success(c, fiber.Map{"connection": connection})
	})

	app.Get("/api/v1/connections/:id/status", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		return success(c, fiber.Map{
			"connectionId":   connection.ID,
			"status":         connection.Status,
			"lastSyncStatus": connection.LastSyncStatus,
			"deletedAt":      connection.DeletedAt,
		})
	})

	app.Get("/api/v1/connections/:id/capabilities", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedConnection(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		provider, _ := providers.Find(connection.ProviderKey)
		return success(c, fiber.Map{
			"connectionId":          connection.ID,
			"providerKey":           connection.ProviderKey,
			"capabilities":          connection.Capabilities,
			"availableCapabilities": provider.Capabilities,
			"bundles":               provider.Bundles,
		})
	})

	app.Patch("/api/v1/connections/:id/capabilities", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedConnection(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		var body capabilitiesBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		provider, ok := providers.Find(connection.ProviderKey)
		if !ok {
			return apiError(c, fiber.StatusBadRequest, "provider_not_supported", "Provider is not supported.")
		}
		capabilities, err := normalizeCapabilityUpdate(provider, body.Capabilities)
		if err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_capabilities", err.Error())
		}
		var updated store.Connection
		err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var updateErr error
			updated, updateErr = tx.UpdateConnectionCapabilities(c.UserContext(), connection.ID, capabilities)
			if updateErr != nil {
				return updateErr
			}
			return persist(store.AuditEvent{
				ID:             auditMutationID(c.UserContext(), "connection-capabilities", updated.ID),
				OrganizationID: updated.OrganizationID,
				UserID:         updated.UserID,
				ConnectionID:   updated.ID,
				EventType:      "connection.capabilities.updated",
				ProviderKey:    updated.ProviderKey,
				Metadata:       map[string]any{"capabilities": capabilities},
			})
		})
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		publishIntegrationEvent(c.UserContext(), cfg, "velion.ingestion.integration.connection_updated", updated, map[string]any{
			"capabilities": capabilities,
			"change":       "capabilities",
		})
		return success(c, fiber.Map{"connection": updated})
	})

	app.Get("/api/v1/connections/:id/consents", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedConnection(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		consents, err := cfg.Repo.ListConnectionConsents(c.UserContext(), connection.ID)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "consents_list_failed", err.Error())
		}
		return success(c, fiber.Map{"connectionId": connection.ID, "consents": consents})
	})

	app.Post("/api/v1/connections/:id/consents", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := scopedConnection(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		var body consentBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		source := strings.TrimSpace(body.Source)
		purpose := strings.TrimSpace(body.Purpose)
		if source == "" || purpose == "" {
			return apiError(c, fiber.StatusBadRequest, "invalid_consent", "source and purpose are required.")
		}
		var revokedAt *time.Time
		if !body.Granted {
			now := time.Now().UTC()
			revokedAt = &now
		}
		candidate := store.ConnectionConsent{
			ID:             "consent_" + uuid.NewString(),
			OrganizationID: connection.OrganizationID,
			ConnectionID:   connection.ID,
			UserID:         connection.UserID,
			ProviderKey:    connection.ProviderKey,
			Source:         source,
			Purpose:        purpose,
			Granted:        body.Granted,
			Metadata:       body.Metadata,
			ExpiresAt:      body.ExpiresAt,
			RevokedAt:      revokedAt,
		}
		var consent store.ConnectionConsent
		err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var consentErr error
			consent, consentErr = tx.UpsertConnectionConsent(c.UserContext(), candidate)
			if consentErr != nil {
				return consentErr
			}
			return persist(store.AuditEvent{
				ID:             "audit:integration:connection-consent:" + consent.ID,
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "connection.consent_changed",
				ProviderKey:    connection.ProviderKey,
				Metadata:       map[string]any{"source": source, "purpose": purpose, "granted": body.Granted},
			})
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "consent_update_failed", err.Error())
		}
		publishIntegrationEvent(c.UserContext(), cfg, "velion.ingestion.integration.consent_changed", connection, map[string]any{
			"source":  source,
			"purpose": purpose,
			"granted": body.Granted,
		})
		return success(c, fiber.Map{"consent": consent})
	})

	app.Get("/api/v1/connections/:id/discovery", internalOrBearerAuth, func(c *fiber.Ctx) error {
		if cfg.Discovery == nil {
			return apiError(c, fiber.StatusServiceUnavailable, "discovery_unavailable", "Provider discovery is not configured.")
		}
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		token, err := cfg.OAuth.AccessTokenForConnection(c.UserContext(), connection.ID)
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this connection.")
			}
			return apiError(c, fiber.StatusBadGateway, "token_broker_failed", "Could not resolve a provider access token.")
		}
		snapshot, err := cfg.Discovery.Discover(c.UserContext(), connection, token.AccessToken)
		if err != nil {
			return apiError(c, fiber.StatusBadGateway, "discovery_failed", err.Error())
		}
		return success(c, fiber.Map{"discovery": snapshot})
	})

	app.Post("/api/v1/connections/:id/actions", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body actionBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		result, err := executeConnectionAction(c, cfg, c.Params("id"), body)
		if err != nil {
			return actionError(c, err)
		}
		return success(c, fiber.Map{"action": result})
	})...)

	app.Post("/api/v1/actions/execute", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body executeActionBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if strings.TrimSpace(body.ConnectionID) == "" {
			return apiError(c, fiber.StatusBadRequest, "connection_required", "connectionId is required.")
		}
		result, err := executeConnectionAction(c, cfg, body.ConnectionID, actionBody{
			Operation:        body.Operation,
			Params:           body.Params,
			Body:             body.Body,
			WriteAttestation: body.WriteAttestation,
			IdempotencyKey:   body.IdempotencyKey,
		})
		if err != nil {
			return actionError(c, err)
		}
		return success(c, fiber.Map{"action": result})
	})...)

	registerLegacyIntegrationRoutes(app, internalAuth, cfg)

	app.Delete("/api/v1/connections/:id", internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		_, err = cfg.OAuth.DisconnectConnection(c.UserContext(), c.Params("id"), "user_requested")
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		return c.SendStatus(fiber.StatusNoContent)
	})

	app.Post("/api/v1/connections/:id/sync", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if connection.DeletedAt != nil {
			return apiError(c, fiber.StatusNotFound, "connection_not_found", "Connection was not found.")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		job, err := createSyncJob(c.UserContext(), cfg, connection, syncJobBody{Reason: "manual", Mode: "incremental"})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_queue_failed", err.Error())
		}
		return c.Status(fiber.StatusAccepted).JSON(fiber.Map{"success": true, "data": fiber.Map{"syncJob": job}})
	})...)

	app.Post("/api/v1/connections/:id/inbox-history", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		if connection.DeletedAt != nil {
			return storeError(c, store.ErrNotFound, "connection_not_found")
		}
		if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
			return authAwareError(c, err)
		}
		if !auth.IsInternalCall(c) {
			principal, ok := auth.PrincipalFromContext(c)
			role := strings.ToLower(strings.TrimSpace(principal.Role))
			privileged := role == "owner" || role == "admin"
			if !ok || (principal.UserID != connection.UserID && !privileged) {
				return apiError(c, fiber.StatusForbidden, "teams_history_forbidden", "Only the connection owner or a workspace administrator can load this Teams history.")
			}
		}
		teamsReadable := slices.Contains(connection.Capabilities, "teams.messages.read") ||
			slices.ContainsFunc(connection.Scopes, func(scope string) bool {
				return strings.EqualFold(scope, "ChannelMessage.Read.All")
			})
		if providers.NormalizeKey(connection.ProviderKey) != "microsoft" || !teamsReadable {
			return apiError(c, fiber.StatusUnprocessableEntity, "teams_history_unavailable", "This connection cannot read Microsoft Teams history.")
		}

		const (
			teamsDefaultHistoryDays       = 30
			teamsHistoryStepDays          = 30
			teamsMaxAdditionalHistoryDays = 3650
		)
		state, err := cfg.Repo.ExtendEmailSyncHistory(
			c.UserContext(), connection.ID+":teams", "teams",
			teamsHistoryStepDays, teamsMaxAdditionalHistoryDays,
		)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "teams_history_queue_failed", "The next Teams history window could not be queued.")
		}
		return c.Status(fiber.StatusAccepted).JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{"history": fiber.Map{
				"channel":     "teams",
				"historyDays": teamsDefaultHistoryDays + state.HistoryBackfillDays,
				"queued":      true,
			}},
		})
	})...)

	app.Get("/api/v1/sync-jobs", internalOrBearerAuth, func(c *fiber.Ctx) error {
		organizationID := strings.TrimSpace(c.Query("organizationId"))
		if !auth.IsInternalCall(c) {
			if principal, ok := auth.PrincipalFromContext(c); ok {
				organizationID = principal.OrganizationID
			}
		}
		jobs, err := cfg.Repo.ListSyncJobs(c.UserContext(), store.SyncJobFilter{
			OrganizationID: organizationID,
			ConnectionID:   strings.TrimSpace(c.Query("connectionId")),
			ProviderKey:    providers.NormalizeKey(c.Query("providerKey")),
			Status:         strings.TrimSpace(c.Query("status")),
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_jobs_list_failed", err.Error())
		}
		return success(c, fiber.Map{"syncJobs": jobs})
	})

	app.Post("/api/v1/sync-jobs", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body syncJobBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if strings.TrimSpace(body.ConnectionID) == "" {
			return apiError(c, fiber.StatusBadRequest, "connection_required", "connectionId is required.")
		}
		connection, err := scopedConnection(c, cfg, body.ConnectionID)
		if err != nil {
			return actionError(c, err)
		}
		job, err := createSyncJob(c.UserContext(), cfg, connection, body)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_queue_failed", err.Error())
		}
		return c.Status(fiber.StatusAccepted).JSON(fiber.Map{"success": true, "data": fiber.Map{"syncJob": job}})
	})...)

	app.Get("/api/v1/sync-jobs/:id", internalOrBearerAuth, func(c *fiber.Ctx) error {
		job, err := scopedSyncJob(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		events, err := cfg.Repo.ListSyncEvents(c.UserContext(), job.ID)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_events_list_failed", err.Error())
		}
		return success(c, fiber.Map{"syncJob": job, "events": events})
	})

	app.Post("/api/v1/sync-jobs/:id/cancel", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body syncCancelBody
		if len(c.Body()) > 0 {
			if err := c.BodyParser(&body); err != nil {
				return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
			}
		}
		job, err := scopedSyncJob(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		cancelled, err := cancelSyncJob(c.UserContext(), cfg, job, body.Reason)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_cancel_failed", err.Error())
		}
		return success(c, fiber.Map{"syncJob": cancelled})
	})...)

	app.Post("/api/v1/sync-jobs/:id/retry", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		job, err := scopedSyncJob(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		retry, err := retrySyncJob(c.UserContext(), c, cfg, job)
		if err != nil {
			if errors.Is(err, errSyncJobNotRetryable) {
				return apiError(c, fiber.StatusConflict, "sync_not_retryable", "Only failed or cancelled sync jobs can be retried.")
			}
			return apiError(c, fiber.StatusInternalServerError, "sync_retry_failed", err.Error())
		}
		return c.Status(fiber.StatusAccepted).JSON(fiber.Map{"success": true, "data": fiber.Map{"syncJob": retry}})
	})...)

	app.Get("/api/v1/sync-jobs/:id/events", internalOrBearerAuth, func(c *fiber.Ctx) error {
		job, err := scopedSyncJob(c, cfg, c.Params("id"))
		if err != nil {
			return actionError(c, err)
		}
		events, err := cfg.Repo.ListSyncEvents(c.UserContext(), job.ID)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "sync_events_list_failed", err.Error())
		}
		c.Set("Content-Type", "text/event-stream")
		c.Set("Cache-Control", "no-cache")
		c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
			for _, event := range events {
				writeSSE(w, event.Type, event)
			}
			writeSSE(w, "sync.snapshot", fiber.Map{"syncJob": job})
			_ = w.Flush()
		})
		return nil
	})

	app.Post("/internal/sync-jobs/claim", chainHandlers(rateLimited, internalAuth, func(c *fiber.Ctx) error {
		var body syncClaimBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		consumer := strings.TrimSpace(body.Consumer)
		if consumer == "" {
			return apiError(c, fiber.StatusBadRequest, "consumer_required", "consumer is required for internal sync claims.")
		}
		if !tokenLeaseConsumerAllowed(cfg.Config, consumer) || !syncClaimTargetAllowed(firstNonEmpty(body.Target, consumer)) {
			return apiError(c, fiber.StatusForbidden, "consumer_not_allowed", "consumer is not allowed to claim sync jobs.")
		}
		claimed, err := claimSyncJob(c.UserContext(), cfg, syncClaimBody{
			Consumer:       consumer,
			Target:         firstNonEmpty(body.Target, consumer),
			OrganizationID: body.OrganizationID,
			ProviderKey:    body.ProviderKey,
			Checkpoint:     body.Checkpoint,
			Metadata:       body.Metadata,
		})
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				return apiError(c, fiber.StatusNotFound, "sync_job_not_available", "No sync job is available for this worker.")
			}
			return apiError(c, fiber.StatusInternalServerError, "sync_claim_failed", err.Error())
		}
		return success(c, fiber.Map{"syncJob": claimed})
	})...)

	app.Patch("/internal/sync-jobs/:id/progress", chainHandlers(rateLimited, internalAuth, func(c *fiber.Ctx) error {
		var body syncProgressBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		consumer := strings.TrimSpace(body.Consumer)
		if consumer == "" {
			return apiError(c, fiber.StatusBadRequest, "consumer_required", "consumer is required for internal sync progress.")
		}
		if !tokenLeaseConsumerAllowed(cfg.Config, consumer) {
			return apiError(c, fiber.StatusForbidden, "consumer_not_allowed", "consumer is not allowed to update sync jobs.")
		}
		updated, err := advanceWorkerSyncJob(c.UserContext(), cfg, c.Params("id"), body)
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				return apiError(c, fiber.StatusNotFound, "sync_job_not_found", "Sync job was not found.")
			}
			if errors.Is(err, errSyncClaimMismatch) {
				return apiError(c, fiber.StatusConflict, "sync_claim_mismatch", "Sync job is not claimed by this consumer.")
			}
			if errors.Is(err, errInvalidSyncStatus) {
				return apiError(c, fiber.StatusBadRequest, "invalid_sync_status", "Sync status is not valid for worker progress.")
			}
			if errors.Is(err, errSyncJobTerminal) {
				return apiError(c, fiber.StatusConflict, "sync_job_terminal", "Sync job is already terminal.")
			}
			return apiError(c, fiber.StatusInternalServerError, "sync_progress_failed", err.Error())
		}
		return success(c, fiber.Map{"syncJob": updated})
	})...)

	app.Post("/api/v1/webhooks/:provider", chainHandlers(rateLimited, func(c *fiber.Ctx) error {
		providerKey := providers.NormalizeKey(c.Params("provider"))
		if _, ok := providers.Find(providerKey); !ok {
			return apiError(c, fiber.StatusNotFound, "provider_not_supported", "Provider is not supported.")
		}
		if err := verifyProviderWebhook(c, cfg.Config, providerKey); err != nil {
			return apiError(c, fiber.StatusUnauthorized, "invalid_webhook_signature", err.Error())
		}
		// Slack's Events API subscription handshake POSTs a signed
		// url_verification envelope and expects the challenge echoed back at
		// the TOP level of the response (the standard success envelope would
		// fail Slack's check).
		if providerKey == "slack" {
			if challenge := slackURLVerificationChallenge(c.Body()); challenge != "" {
				return c.JSON(fiber.Map{"challenge": challenge})
			}
		}
		normalized, err := normalizeWebhookEvent(c, cfg, providerKey)
		if err != nil {
			if errors.Is(err, hotpath.ErrInvalidJSON) {
				return apiError(c, fiber.StatusBadRequest, "invalid_json", "Webhook payload must be valid JSON.")
			}
			return apiError(c, fiber.StatusBadGateway, "webhook_normalize_failed", err.Error())
		}
		// Real provider callbacks carry no Velion org id; without this
		// resolution the event is stored org-less and every downstream
		// consumer silently drops it (2026-07-07 verification finding).
		type delivery struct {
			organizationID string
			connectionID   string
			payload        map[string]any
			eventID        string
		}
		deliveries := []delivery{{
			organizationID: normalized.OrganizationID,
			payload:        normalized.Payload,
			eventID:        normalized.EventID,
		}}
		// Meta and Slack signatures authenticate the payload, not caller-supplied
		// Velion tenant headers. Always discard any normalized org claim and bind
		// these account-wide callbacks through provider asset ownership.
		if webhookRequiresResolvedOrganization(providerKey) {
			deliveries = nil
			if cfg.WebhookOrg != nil {
				if isMetaWebhookProvider(providerKey) {
					if resolved, ok := cfg.WebhookOrg.ResolvePayloads(c.UserContext(), providerKey, normalized.Payload); ok {
						for _, item := range resolved {
							deliveries = append(deliveries, delivery{
								organizationID: item.Resolution.OrganizationID,
								connectionID:   item.Resolution.ConnectionID,
								payload:        item.Payload,
							})
						}
					}
				} else if resolution, ok := cfg.WebhookOrg.Resolve(c.UserContext(), providerKey, normalized.Payload); ok {
					deliveries = []delivery{{
						organizationID: resolution.OrganizationID,
						connectionID:   resolution.ConnectionID,
						payload:        normalized.Payload,
					}}
				}
			}
		}
		if (len(deliveries) == 0 || deliveries[0].organizationID == "") && webhookRequiresResolvedOrganization(providerKey) {
			// Account-wide callbacks do not carry a Velion org. A resolution
			// miss is often transient (Graph/NATS/Postgres outage) and must be
			// retried by the provider; accepting it would publish an org-less
			// event that Conversation Core terminally acknowledges and drops.
			return apiError(c, fiber.StatusServiceUnavailable, "webhook_tenant_unresolved", "Webhook tenant could not be resolved; retry delivery.")
		}
		if len(deliveries) == 1 {
			deliveries[0].eventID = normalized.EventID
		}
		duplicate := true
		webhookEventIDs := make([]string, 0, len(deliveries))
		for index := range deliveries {
			item := &deliveries[index]
			if item.eventID == "" {
				item.eventID = tenantScopedWebhookEventID(normalized.EventID, item.connectionID)
			}
			event := store.WebhookEvent{
				ID:             item.eventID,
				OrganizationID: item.organizationID,
				ProviderKey:    normalized.ProviderKey,
				EventType:      normalized.EventType,
				SignatureHash:  normalized.SignatureHash,
				Payload:        item.payload,
				ReceivedAt:     time.Now().UTC(),
			}
			if err := cfg.Repo.InsertWebhookEvent(c.UserContext(), event); err != nil {
				if !errors.Is(err, store.ErrConflict) {
					return apiError(c, fiber.StatusInternalServerError, "webhook_store_failed", err.Error())
				}
			} else {
				duplicate = false
			}
			webhookEventIDs = append(webhookEventIDs, event.ID)
			if cfg.Events != nil {
				if err := cfg.Events.Publish(c.UserContext(), events.Event{
					Type:           "velion.ingestion.integration.webhook_received",
					OrganizationID: event.OrganizationID,
					ConnectionID:   item.connectionID,
					ProviderKey:    event.ProviderKey,
					Data:           map[string]any{"eventType": event.EventType, "webhookEventId": event.ID, "normalizedBy": normalized.NormalizedBy},
				}); err != nil {
					// The payload is durably stored, so the provider's retry becomes a
					// duplicate insert. We intentionally republish duplicates above;
					// returning 503 here therefore gives at-least-once delivery across
					// a transient event-bus failure without duplicating conversations
					// (Conversation Core deduplicates provider message ids).
					return apiError(c, fiber.StatusServiceUnavailable, "webhook_delivery_unavailable", "Webhook was stored but downstream delivery is temporarily unavailable; retry delivery.")
				}
			}
		}
		return success(c, fiber.Map{
			"accepted": true, "duplicate": duplicate, "webhookEventId": webhookEventIDs[0],
			"webhookEventIds": webhookEventIDs, "normalizedBy": normalized.NormalizedBy,
		})
	})...)

	app.Get("/api/v1/webhooks/:provider", chainHandlers(rateLimited, func(c *fiber.Ctx) error {
		providerKey := providers.NormalizeKey(c.Params("provider"))
		if _, ok := providers.Find(providerKey); !ok {
			return apiError(c, fiber.StatusNotFound, "provider_not_supported", "Provider is not supported.")
		}
		challenge, err := verifyProviderWebhookChallenge(c, cfg.Config, providerKey)
		if err != nil {
			return apiError(c, fiber.StatusUnauthorized, "invalid_webhook_challenge", err.Error())
		}
		c.Set("Content-Type", "text/plain; charset=utf-8")
		return c.SendString(challenge)
	})...)

	// GET the full stored payload for a webhook_received event by id.
	// velion.ingestion.integration.webhook_received (published above, and at
	// the SCIM/legacy insert sites) carries only metadata (eventType,
	// webhookEventId) so NATS messages stay small — downstream cores
	// (conversation-core, leads-core, …) that need the actual content fetch
	// it here. Internal-only: this is provider-origin data, not
	// end-user-facing, and org-scoped via organizationId.
	app.Get("/internal/webhooks/events/:id", internalAuth, func(c *fiber.Ctx) error {
		organizationID := strings.TrimSpace(c.Query("organizationId"))
		event, err := cfg.Repo.GetWebhookEvent(c.UserContext(), organizationID, c.Params("id"))
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				return apiError(c, fiber.StatusNotFound, "webhook_event_not_found", "No webhook event exists for this id and organization.")
			}
			return apiError(c, fiber.StatusInternalServerError, "webhook_event_lookup_failed", err.Error())
		}
		return success(c, fiber.Map{"webhookEvent": event})
	})

	app.Get("/api/v1/scim/tokens", internalOrBearerAuth, func(c *fiber.Ctx) error {
		organizationID, err := organizationIDForSCIMTokenRequest(c, c.Query("organizationId"))
		if err != nil {
			return authAwareError(c, err)
		}
		tokens, err := cfg.Repo.ListSCIMTokens(c.UserContext(), organizationID)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "scim_tokens_list_failed", err.Error())
		}
		return success(c, fiber.Map{"tokens": tokens})
	})

	app.Post("/api/v1/scim/tokens", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		var body scimTokenBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		organizationID, err := organizationIDForSCIMTokenRequest(c, body.OrganizationID)
		if err != nil {
			return authAwareError(c, err)
		}
		raw, err := oauth.RandomURLToken(32)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "scim_token_generate_failed", err.Error())
		}
		bearer := "scim_" + raw
		now := time.Now().UTC()
		createdBy := userIDForRequest(c)
		if createdBy == "" {
			createdBy = "internal-service"
		}
		name := strings.TrimSpace(body.Name)
		if name == "" {
			name = "SCIM token"
		}
		candidate := store.SCIMToken{
			ID:             "scimtok_" + uuid.NewString(),
			OrganizationID: organizationID,
			Name:           name,
			TokenPrefix:    scimTokenPrefix(bearer),
			CreatedBy:      createdBy,
			ExpiresAt:      body.ExpiresAt,
			CreatedAt:      now,
			UpdatedAt:      now,
		}
		var token store.SCIMToken
		err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var createErr error
			token, createErr = tx.CreateSCIMToken(c.UserContext(), candidate, scimTokenHash(bearer))
			if createErr != nil {
				return createErr
			}
			return persist(store.AuditEvent{
				ID:             "audit:integration:scim-token-created:" + token.ID,
				OrganizationID: organizationID,
				UserID:         createdBy,
				EventType:      "scim.token.created",
				ProviderKey:    "scim",
				Metadata:       map[string]any{"tokenId": token.ID, "tokenPrefix": token.TokenPrefix},
				CreatedAt:      now,
			})
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "scim_token_create_failed", err.Error())
		}
		return c.Status(fiber.StatusCreated).JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"token":       token,
				"bearerToken": bearer,
			},
			"meta": responseMeta(c),
		})
	})...)

	app.Delete("/api/v1/scim/tokens/:id", chainHandlers(rateLimited, internalOrBearerAuth, func(c *fiber.Ctx) error {
		organizationID, err := organizationIDForSCIMTokenRequest(c, c.Query("organizationId"))
		if err != nil {
			return authAwareError(c, err)
		}
		now := time.Now().UTC()
		var revoked store.SCIMToken
		err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			var revokeErr error
			revoked, revokeErr = tx.RevokeSCIMToken(c.UserContext(), organizationID, c.Params("id"), now)
			if revokeErr != nil {
				return revokeErr
			}
			return persist(store.AuditEvent{
				ID:             "audit:integration:scim-token-revoked:" + revoked.ID,
				OrganizationID: organizationID,
				UserID:         userIDForRequest(c),
				EventType:      "scim.token.revoked",
				ProviderKey:    "scim",
				Metadata:       map[string]any{"tokenId": revoked.ID, "tokenPrefix": revoked.TokenPrefix},
				CreatedAt:      now,
			})
		})
		if err != nil {
			return storeError(c, err, "scim_token_not_found")
		}
		return success(c, fiber.Map{"token": revoked})
	})...)

	app.All("/api/v1/scim/v2/*", chainHandlers(rateLimited, func(c *fiber.Ctx) error {
		headerOrganizationID := firstNonEmpty(c.Get("X-Org-ID"), c.Query("organizationId"))
		if len(cfg.Config.SCIMBearerTokens) > 0 && headerOrganizationID == "" {
			return apiError(c, fiber.StatusBadRequest, "organization_required", "X-Org-ID or organizationId is required for SCIM.")
		}
		if !scimBearerAuthorized(c, cfg, headerOrganizationID) {
			return apiError(c, fiber.StatusUnauthorized, "unauthorized", "Invalid SCIM bearer token.")
		}
		payload := map[string]any{}
		if len(c.Body()) > 0 {
			if err := json.Unmarshal(c.Body(), &payload); err != nil {
				return scimError(c, fiber.StatusBadRequest, "invalidValue", "SCIM payload must be valid JSON.")
			}
		}
		resourcePath := strings.Trim(c.Params("*"), "/")
		organizationID := firstNonEmpty(headerOrganizationID, stringFromAny(payload["organizationId"]))
		eventType := scimEventType(c.Method(), resourcePath)
		event := store.WebhookEvent{
			ID:             "scim_" + uuid.NewString(),
			OrganizationID: organizationID,
			ProviderKey:    "scim",
			EventType:      eventType,
			Payload: map[string]any{
				"path":    resourcePath,
				"method":  c.Method(),
				"payload": payload,
			},
			ReceivedAt: time.Now().UTC(),
		}
		if err := cfg.Repo.InsertWebhookEvent(c.UserContext(), event); err != nil {
			return apiError(c, fiber.StatusInternalServerError, "scim_store_failed", err.Error())
		}
		if cfg.Events != nil {
			_ = cfg.Events.Publish(c.UserContext(), events.Event{
				Type:           "velion.ingestion.integration.scim_event_received",
				OrganizationID: organizationID,
				ProviderKey:    "scim",
				Data:           map[string]any{"eventType": eventType, "webhookEventId": event.ID},
			})
		}
		return scimResponse(c, event, resourcePath, payload)
	})...)

	app.Get("/api/v1/projections/integration-profile", internalOrBearerAuth, func(c *fiber.Ctx) error {
		organizationID := strings.TrimSpace(c.Query("organizationId"))
		userID := strings.TrimSpace(c.Query("userId"))
		if !auth.IsInternalCall(c) {
			if principal, ok := auth.PrincipalFromContext(c); ok {
				organizationID = principal.OrganizationID
				userID = principal.UserID
			}
		}
		connections, err := cfg.Repo.ListConnections(c.UserContext(), store.ConnectionFilter{
			OrganizationID: organizationID,
			UserID:         userID,
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "profile_projection_failed", err.Error())
		}
		connections = filterConnectionsByCategory(connections, firstNonEmpty(c.Query("category"), c.Query("providerCategory")))
		items := make([]fiber.Map, 0, len(connections))
		for _, connection := range connections {
			consents, _ := cfg.Repo.ListConnectionConsents(c.UserContext(), connection.ID)
			jobs, _ := cfg.Repo.ListSyncJobs(c.UserContext(), store.SyncJobFilter{ConnectionID: connection.ID})
			items = append(items, fiber.Map{
				"connection": connection,
				"consents":   consents,
				"syncJobs":   jobs,
			})
		}
		return success(c, fiber.Map{
			"organizationId": organizationID,
			"userId":         userID,
			"integrations":   items,
			"generatedAt":    time.Now().UTC(),
		})
	})

	app.Post("/internal/connectors/token", chainHandlers(rateLimited, internalAuth, func(c *fiber.Ctx) error {
		var body tokenBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if strings.TrimSpace(body.Consumer) == "" {
			return apiError(c, fiber.StatusBadRequest, "consumer_required", "consumer is required for internal token leases.")
		}
		if !tokenLeaseConsumerAllowed(cfg.Config, body.Consumer) {
			return apiError(c, fiber.StatusForbidden, "consumer_not_allowed", "consumer is not allowed to lease provider tokens.")
		}
		var token oauth.AccessTokenResult
		var err error
		if strings.TrimSpace(body.ConnectionID) != "" {
			if strings.TrimSpace(body.OrganizationID) == "" {
				return apiError(c, fiber.StatusBadRequest, "organization_required", "organizationId is required.")
			}
			connection, lookupErr := cfg.Repo.GetConnection(c.UserContext(), strings.TrimSpace(body.ConnectionID))
			if lookupErr != nil {
				if errors.Is(lookupErr, store.ErrNotFound) {
					return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this organization and connector.")
				}
				return apiError(c, fiber.StatusBadGateway, "token_broker_failed", "Could not resolve a provider access token.")
			}
			if connection.DeletedAt != nil || connection.Status == "deleted" {
				return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this organization and connector.")
			}
			if strings.TrimSpace(connection.OrganizationID) != strings.TrimSpace(body.OrganizationID) {
				return apiError(c, fiber.StatusForbidden, "connection_org_mismatch", "Connection does not belong to the requested organization.")
			}
			token, err = cfg.OAuth.AccessTokenForConnection(c.UserContext(), body.ConnectionID)
		} else if strings.TrimSpace(body.OrganizationID) == "" {
			return apiError(c, fiber.StatusBadRequest, "organization_required", "organizationId is required.")
		} else {
			connectorType := strings.TrimSpace(body.ConnectorType)
			if connectorType == "" {
				connectorType = "microsoft-graph"
			}
			token, err = cfg.OAuth.AccessToken(c.UserContext(), body.OrganizationID, connectorType)
		}
		if err != nil {
			if errors.Is(err, store.ErrNotFound) {
				return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this organization and connector.")
			}
			return apiError(c, fiber.StatusBadGateway, "token_broker_failed", "Could not resolve a provider access token.")
		}
		if err := recordTokenLease(c.UserContext(), cfg, token, body.Consumer); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "token_lease_audit_failed", "Token lease could not be persisted safely.")
		}
		return success(c, token)
	})...)

	app.Get("/internal/gdpr/export", internalAuth, func(c *fiber.Ctx) error {
		organizationID := strings.TrimSpace(c.Query("organizationId"))
		userID := strings.TrimSpace(c.Query("userId"))
		if organizationID == "" {
			return apiError(c, fiber.StatusBadRequest, "organization_required", "organizationId is required.")
		}
		export, err := buildGDPRExport(c.UserContext(), cfg, organizationID, userID)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "gdpr_export_failed", err.Error())
		}
		return success(c, export)
	})

	app.Post("/internal/gdpr/delete", internalAuth, func(c *fiber.Ctx) error {
		var body gdprRequestBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		if strings.TrimSpace(body.OrganizationID) == "" {
			return apiError(c, fiber.StatusBadRequest, "organization_required", "organizationId is required.")
		}
		result, err := runGDPRDelete(c.UserContext(), cfg, body)
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "gdpr_delete_failed", err.Error())
		}
		return success(c, result)
	})

	return app
}

type createConnectSessionBody struct {
	OrganizationID  string            `json:"organizationId"`
	WorkspaceID     string            `json:"workspaceId"`
	UserID          string            `json:"userId"`
	UserEmail       string            `json:"userEmail"`
	SelectedSources []string          `json:"selectedSources"`
	Capabilities    []string          `json:"capabilities"`
	Bundles         []string          `json:"bundles"`
	ReturnURL       string            `json:"returnUrl"`
	Shop            string            `json:"shop"`
	ProviderContext map[string]string `json:"providerContext"`
}

func (b createConnectSessionBody) providerContext() map[string]string {
	out := map[string]string{}
	for key, value := range b.ProviderContext {
		if strings.TrimSpace(key) != "" && strings.TrimSpace(value) != "" {
			out[key] = value
		}
	}
	if strings.TrimSpace(b.Shop) != "" {
		out["shop"] = b.Shop
	}
	return out
}

func normalizeConnectSessionIdentity(c *fiber.Ctx, body *createConnectSessionBody) error {
	if auth.IsInternalCall(c) {
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
	if strings.TrimSpace(body.WorkspaceID) == "" {
		body.WorkspaceID = principal.WorkspaceID
	}
	if strings.TrimSpace(body.UserID) == "" || body.UserID != principal.UserID {
		body.UserID = principal.UserID
	}
	if strings.TrimSpace(principal.Email) != "" {
		body.UserEmail = principal.Email
	}
	return nil
}

type tokenBody struct {
	OrganizationID string `json:"organizationId"`
	ConnectorType  string `json:"connectorType"`
	ConnectionID   string `json:"connectionId"`
	Consumer       string `json:"consumer"`
}

type gdprRequestBody struct {
	OrganizationID string `json:"organizationId"`
	UserID         string `json:"userId"`
	Reason         string `json:"reason"`
}

type capabilitiesBody struct {
	Capabilities []string `json:"capabilities"`
}

type consentBody struct {
	Source    string         `json:"source"`
	Purpose   string         `json:"purpose"`
	Granted   bool           `json:"granted"`
	Metadata  map[string]any `json:"metadata"`
	ExpiresAt *time.Time     `json:"expiresAt"`
}

type syncJobBody struct {
	ConnectionID string         `json:"connectionId"`
	Reason       string         `json:"reason"`
	Mode         string         `json:"mode"`
	Checkpoint   map[string]any `json:"checkpoint"`
	Metadata     map[string]any `json:"metadata"`
}

type syncCancelBody struct {
	Reason string `json:"reason"`
}

type syncClaimBody struct {
	Consumer       string         `json:"consumer"`
	Target         string         `json:"target"`
	OrganizationID string         `json:"organizationId"`
	ProviderKey    string         `json:"providerKey"`
	Checkpoint     map[string]any `json:"checkpoint"`
	Metadata       map[string]any `json:"metadata"`
}

type syncProgressBody struct {
	Consumer   string          `json:"consumer"`
	Status     string          `json:"status"`
	Message    string          `json:"message"`
	Checkpoint map[string]any  `json:"checkpoint"`
	Metadata   map[string]any  `json:"metadata"`
	Sources    []syncSourceRef `json:"sources"`
}

type syncSourceRef struct {
	Provider   string `json:"provider"`
	Type       string `json:"type"`
	SourceID   string `json:"sourceId"`
	ExternalID string `json:"externalId"`
	Status     string `json:"status"`
	Title      string `json:"title"`
	URL        string `json:"url"`
}

type scimTokenBody struct {
	OrganizationID string     `json:"organizationId"`
	Name           string     `json:"name"`
	ExpiresAt      *time.Time `json:"expiresAt"`
}

type actionBody struct {
	Operation        string         `json:"operation"`
	Params           map[string]any `json:"params"`
	Body             map[string]any `json:"body"`
	WriteAttestation string         `json:"writeAttestation"`
	IdempotencyKey   string         `json:"idempotencyKey"`
}

type executeActionBody struct {
	ConnectionID     string         `json:"connectionId"`
	Operation        string         `json:"operation"`
	Params           map[string]any `json:"params"`
	Body             map[string]any `json:"body"`
	WriteAttestation string         `json:"writeAttestation"`
	IdempotencyKey   string         `json:"idempotencyKey"`
}

func scopedConnection(c *fiber.Ctx, cfg ServerConfig, connectionID string) (store.Connection, error) {
	connection, err := cfg.Repo.GetConnection(c.UserContext(), strings.TrimSpace(connectionID))
	if err != nil {
		return store.Connection{}, err
	}
	if connection.DeletedAt != nil {
		return store.Connection{}, store.ErrNotFound
	}
	if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
		return store.Connection{}, err
	}
	return connection, nil
}

func normalizeCapabilityUpdate(provider providers.Provider, requested []string) ([]string, error) {
	allowed := map[string]struct{}{}
	for _, capability := range provider.Capabilities {
		allowed[capability.Key] = struct{}{}
	}
	out := []string{}
	seen := map[string]struct{}{}
	for _, raw := range requested {
		key := strings.TrimSpace(raw)
		if key == "" {
			continue
		}
		if _, ok := allowed[key]; !ok {
			return nil, errors.New("unsupported capability: " + key)
		}
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		out = append(out, key)
	}
	return out, nil
}

func createSyncJob(ctx context.Context, cfg ServerConfig, connection store.Connection, body syncJobBody) (store.SyncJob, error) {
	needsMetaProvisioning := isMetaWebhookProvider(connection.ProviderKey) && hasMetaInboxCapability(connection.Capabilities)
	reason := strings.TrimSpace(body.Reason)
	if reason == "" {
		reason = "manual"
	}
	mode := strings.TrimSpace(body.Mode)
	if mode == "" {
		mode = "incremental"
	}
	now := time.Now().UTC()
	candidate := store.SyncJob{
		ID:             "sync_" + uuid.NewString(),
		OrganizationID: connection.OrganizationID,
		ConnectionID:   connection.ID,
		UserID:         connection.UserID,
		ProviderKey:    connection.ProviderKey,
		Status:         "queued",
		Reason:         reason,
		Mode:           mode,
		Checkpoint:     syncJobInitialCheckpoint(connection, body),
		Metadata:       body.Metadata,
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	var job store.SyncJob
	err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var createErr error
		job, createErr = tx.CreateSyncJob(ctx, candidate)
		if createErr != nil {
			return createErr
		}
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     job.ID,
			Type:      "sync.queued",
			Message:   "Sync job was queued.",
			Metadata:  map[string]any{"reason": reason, "mode": mode},
			CreatedAt: now,
		}); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             "audit:integration:sync-queued:" + job.ID,
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "connection.sync.queued",
			ProviderKey:    connection.ProviderKey,
			Metadata:       map[string]any{"syncJobId": job.ID, "reason": reason, "mode": mode},
			CreatedAt:      now,
		})
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	if needsMetaProvisioning {
		var provisionErr error
		if cfg.WebhookOrg == nil {
			provisionErr = fmt.Errorf("Meta webhook provisioning is not configured")
		} else {
			connection, provisionErr = cfg.WebhookOrg.ProvisionConnection(ctx, connection)
		}
		if provisionErr != nil {
			completedAt := time.Now().UTC()
			job.Status = "failed"
			job.UpdatedAt = completedAt
			job.CompletedAt = &completedAt
			job.Metadata = mergeMetadata(job.Metadata, map[string]any{
				"failureCode": "meta_inbox_provisioning_failed", "retryable": true,
			})
			if updated, updateErr := cfg.Repo.UpdateSyncJob(ctx, job); updateErr == nil {
				job = updated
			}
			_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
				ID: "sync_evt_" + uuid.NewString(), JobID: job.ID, Type: "sync.failed",
				Message:  "Meta inbox provisioning failed; retry is required.",
				Metadata: map[string]any{"failureCode": "meta_inbox_provisioning_failed", "retryable": true}, CreatedAt: completedAt,
			})
			return job, fmt.Errorf("provision Meta inbox webhooks: %w", provisionErr)
		}
	}
	publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration.sync_started", connection, map[string]any{
		"syncJobId": job.ID,
		"status":    job.Status,
		"reason":    reason,
		"mode":      mode,
	})
	recordUsage(cfg, connection.OrganizationID, "integration_sync_queued", 1, map[string]any{
		"providerKey":  connection.ProviderKey,
		"connectionId": connection.ID,
	})
	return advanceSyncJob(ctx, cfg, connection, job)
}

func hasMetaInboxCapability(capabilities []string) bool {
	for _, capability := range capabilities {
		switch strings.TrimSpace(strings.ToLower(capability)) {
		case "social.inbox.read", "social.messenger.manage", "social.whatsapp.manage":
			return true
		}
	}
	return false
}

func syncJobInitialCheckpoint(connection store.Connection, body syncJobBody) map[string]any {
	checkpoint := mergeMetadata(nil, body.Checkpoint)
	if connection.ProviderKey != "microsoft" {
		return checkpoint
	}
	return promoteMicrosoftSourceIdentifiers(checkpoint, body.Metadata)
}

func promoteMicrosoftSourceIdentifiers(checkpoint, metadata map[string]any) map[string]any {
	out := mergeMetadata(nil, checkpoint)
	promoteSyncString(out, metadata, "site_id", "siteId", "sharepointSiteId", "sharepoint_site_id")
	promoteSyncString(out, metadata, "drive_id", "driveId", "sharepointDriveId", "sharepoint_drive_id")
	promoteSyncString(out, metadata, "tenant_id", "tenantId")
	promoteSyncString(out, metadata, "site_web_url", "siteWebUrl", "url")
	promoteSyncString(out, metadata, "drive_name", "driveName", "title")
	promoteSyncString(out, metadata, "drive_type", "driveType")
	return out
}

func promoteSyncString(target, fallback map[string]any, canonicalKey string, aliases ...string) {
	if stringFromAny(target[canonicalKey]) != "" {
		return
	}
	for _, alias := range append([]string{canonicalKey}, aliases...) {
		if value := stringFromAny(target[alias]); value != "" {
			target[canonicalKey] = safeSyncString(value, 256)
			return
		}
	}
	for _, alias := range append([]string{canonicalKey}, aliases...) {
		if value := stringFromAny(fallback[alias]); value != "" {
			target[canonicalKey] = safeSyncString(value, 256)
			return
		}
	}
}

func safeSyncString(value string, maxLen int) string {
	value = strings.TrimSpace(value)
	if maxLen > 0 && len(value) > maxLen {
		return value[:maxLen]
	}
	return value
}

func advanceSyncJob(ctx context.Context, cfg ServerConfig, connection store.Connection, job store.SyncJob) (store.SyncJob, error) {
	now := time.Now().UTC()
	if job.StartedAt == nil {
		job.StartedAt = &now
	}
	job.Status = "running"
	job.Metadata = mergeMetadata(job.Metadata, map[string]any{
		"contentIngestion": "external_only",
		"sourceContent":    "not_stored_by_integration_corev2",
	})
	target := syncHandoffTarget(connection)
	var updated store.SyncJob
	err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		running, updateErr := tx.UpdateSyncJob(ctx, job)
		if updateErr != nil {
			return updateErr
		}
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     running.ID,
			Type:      "sync.running",
			Message:   "Sync job is being prepared for provider handoff.",
			Metadata:  map[string]any{"providerKey": connection.ProviderKey},
			CreatedAt: now,
		}); err != nil {
			return err
		}

		next := running
		next.Status = target.status
		next.Checkpoint = mergeMetadata(next.Checkpoint, target.checkpoint)
		next.Metadata = mergeMetadata(next.Metadata, target.metadata)
		updated, updateErr = tx.UpdateSyncJob(ctx, next)
		if updateErr != nil {
			return updateErr
		}
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     updated.ID,
			Type:      target.eventType,
			Message:   target.message,
			Metadata:  target.metadata,
			CreatedAt: time.Now().UTC(),
		}); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "sync-handoff", updated.ID),
			OrganizationID: updated.OrganizationID,
			UserID:         updated.UserID,
			ConnectionID:   updated.ConnectionID,
			EventType:      "connection.sync.handoff",
			ProviderKey:    updated.ProviderKey,
			Metadata: map[string]any{
				"syncJobId":     updated.ID,
				"status":        updated.Status,
				"handoffTarget": target.metadata["handoffTarget"],
			},
			CreatedAt: now,
		})
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration.sync_handoff", connection, map[string]any{
		"syncJobId": updated.ID,
		"status":    updated.Status,
		"target":    target.metadata["handoffTarget"],
	})
	return updated, nil
}

type syncHandoff struct {
	status     string
	eventType  string
	message    string
	checkpoint map[string]any
	metadata   map[string]any
}

func syncHandoffTarget(connection store.Connection) syncHandoff {
	switch connection.ProviderKey {
	case "microsoft":
		return syncHandoff{
			status:    "waiting_provider",
			eventType: "sync.waiting_provider",
			message:   "Microsoft SharePoint and OneDrive sync is delegated to finspo-core.",
			checkpoint: map[string]any{
				"provider": "microsoft",
				"adapter":  "finspo-core",
			},
			metadata: map[string]any{
				"handoffTarget": "finspo-core",
				"reason":        "sharepoint_onedrive_domain_logic",
				"tokenBroker":   "/internal/connectors/token",
			},
		}
	default:
		return syncHandoff{
			status:    "handoff_data_plane",
			eventType: "sync.handoff_data_plane",
			message:   "Provider metadata is ready for Data Plane source ingestion workers.",
			checkpoint: map[string]any{
				"provider": connection.ProviderKey,
				"adapter":  "data-plane-v2",
			},
			metadata: map[string]any{
				"handoffTarget": "data-plane-v2",
				"sourcePolicy":  "explicit_sync_only",
			},
		}
	}
}

func mergeMetadata(base map[string]any, extra map[string]any) map[string]any {
	merged := map[string]any{}
	for key, value := range base {
		merged[key] = value
	}
	for key, value := range extra {
		merged[key] = value
	}
	return merged
}

func cancelSyncJob(ctx context.Context, cfg ServerConfig, job store.SyncJob, reason string) (store.SyncJob, error) {
	if syncJobTerminal(job.Status) {
		return job, nil
	}
	reason = strings.TrimSpace(reason)
	if reason == "" {
		reason = "user_requested"
	}
	now := time.Now().UTC()
	job.Status = "cancelled"
	job.UpdatedAt = now
	job.CompletedAt = &now
	job.Metadata = mergeMetadata(job.Metadata, map[string]any{"cancelReason": reason})
	var updated store.SyncJob
	err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var updateErr error
		updated, updateErr = tx.UpdateSyncJob(ctx, job)
		if updateErr != nil {
			return updateErr
		}
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     updated.ID,
			Type:      "sync.cancelled",
			Message:   "Sync job was cancelled.",
			Metadata:  map[string]any{"reason": reason},
			CreatedAt: now,
		}); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "sync-cancelled", updated.ID),
			OrganizationID: updated.OrganizationID,
			UserID:         updated.UserID,
			ConnectionID:   updated.ConnectionID,
			EventType:      "connection.sync.cancelled",
			ProviderKey:    updated.ProviderKey,
			Metadata:       map[string]any{"syncJobId": updated.ID, "reason": reason},
			CreatedAt:      now,
		})
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	if cfg.Events != nil {
		_ = cfg.Events.Publish(ctx, events.Event{
			Type:           "velion.ingestion.integration.sync_cancelled",
			OrganizationID: updated.OrganizationID,
			ProviderKey:    updated.ProviderKey,
			Data:           map[string]any{"syncJobId": updated.ID, "reason": reason},
		})
	}
	return updated, nil
}

var errSyncJobNotRetryable = errors.New("sync job is not retryable")
var errSyncClaimMismatch = errors.New("sync job claim mismatch")
var errInvalidSyncStatus = errors.New("invalid sync status")
var errSyncJobTerminal = errors.New("sync job is terminal")

func retrySyncJob(ctx context.Context, c *fiber.Ctx, cfg ServerConfig, job store.SyncJob) (store.SyncJob, error) {
	if !syncJobRetryable(job.Status) {
		return store.SyncJob{}, errSyncJobNotRetryable
	}
	connection, err := scopedConnection(c, cfg, job.ConnectionID)
	if err != nil {
		return store.SyncJob{}, err
	}
	metadata := mergeMetadata(job.Metadata, map[string]any{
		"retryOf":      job.ID,
		"retryStatus":  job.Status,
		"retryStarted": time.Now().UTC().Format(time.RFC3339),
	})
	return createSyncJob(ctx, cfg, connection, syncJobBody{
		Reason:   "retry",
		Mode:     firstNonEmpty(job.Mode, "incremental"),
		Metadata: metadata,
	})
}

func syncJobTerminal(status string) bool {
	switch strings.TrimSpace(status) {
	case "completed", "failed", "cancelled":
		return true
	default:
		return false
	}
}

func syncJobRetryable(status string) bool {
	switch strings.TrimSpace(status) {
	case "failed", "cancelled":
		return true
	default:
		return false
	}
}

func claimSyncJob(ctx context.Context, cfg ServerConfig, body syncClaimBody) (store.SyncJob, error) {
	now := time.Now().UTC()
	target := firstNonEmpty(body.Target, body.Consumer)
	checkpoint := mergeMetadata(body.Checkpoint, map[string]any{
		"handoffClaimedAt": now.Format(time.RFC3339),
		"handoffTarget":    target,
	})
	metadata := mergeMetadata(body.Metadata, map[string]any{
		"claimedAt":     now.Format(time.RFC3339),
		"claimedBy":     strings.TrimSpace(body.Consumer),
		"handoffTarget": target,
	})
	claim := store.SyncJobClaim{
		Consumer:       strings.TrimSpace(body.Consumer),
		Target:         target,
		OrganizationID: strings.TrimSpace(body.OrganizationID),
		ProviderKey:    providers.NormalizeKey(body.ProviderKey),
		Checkpoint:     checkpoint,
		Metadata:       metadata,
	}
	var job store.SyncJob
	err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var claimErr error
		job, claimErr = tx.ClaimSyncJob(ctx, claim)
		if claimErr != nil {
			return claimErr
		}
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     job.ID,
			Type:      "sync.claimed",
			Message:   "Sync job was claimed by an internal worker.",
			Metadata:  map[string]any{"consumer": body.Consumer, "target": target},
			CreatedAt: now,
		}); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             "audit:integration:sync-claimed:" + job.ID,
			OrganizationID: job.OrganizationID,
			UserID:         job.UserID,
			ConnectionID:   job.ConnectionID,
			EventType:      "connection.sync.claimed",
			ProviderKey:    job.ProviderKey,
			Metadata:       map[string]any{"syncJobId": job.ID, "consumer": body.Consumer, "target": target},
			CreatedAt:      now,
		})
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	if connection, lookupErr := cfg.Repo.GetConnection(ctx, job.ConnectionID); lookupErr == nil {
		publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration.sync_claimed", connection, map[string]any{
			"syncJobId": job.ID,
			"consumer":  body.Consumer,
			"target":    target,
		})
	}
	return job, nil
}

func advanceWorkerSyncJob(ctx context.Context, cfg ServerConfig, id string, body syncProgressBody) (store.SyncJob, error) {
	job, err := cfg.Repo.GetSyncJob(ctx, strings.TrimSpace(id))
	if err != nil {
		return store.SyncJob{}, err
	}
	consumer := strings.TrimSpace(body.Consumer)
	if stringFromAny(job.Metadata["claimedBy"]) != consumer {
		return store.SyncJob{}, errSyncClaimMismatch
	}
	status := normalizeWorkerSyncStatus(body.Status)
	if status == "" {
		return store.SyncJob{}, errInvalidSyncStatus
	}
	if syncJobTerminal(job.Status) {
		if status == job.Status {
			return job, nil
		}
		return store.SyncJob{}, errSyncJobTerminal
	}
	now := time.Now().UTC()
	job.Status = status
	job.Checkpoint = mergeMetadata(job.Checkpoint, body.Checkpoint)
	job.Checkpoint = mergeSyncSourceRefs(job.Checkpoint, body.Sources)
	job.Checkpoint = mergeMetadata(job.Checkpoint, map[string]any{
		"lastCheckpointAt": now.Format(time.RFC3339),
		"lastCheckpointBy": consumer,
	})
	job.Metadata = mergeMetadata(job.Metadata, body.Metadata)
	job.Metadata = mergeMetadata(job.Metadata, map[string]any{
		"lastProgressAt": now.Format(time.RFC3339),
		"lastProgressBy": consumer,
	})
	if status == "completed" || status == "failed" || status == "cancelled" {
		job.CompletedAt = &now
	}
	var updated store.SyncJob
	err = withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var updateErr error
		updated, updateErr = tx.UpdateSyncJob(ctx, job)
		if updateErr != nil {
			return updateErr
		}
		eventType, message := workerSyncEvent(status, body.Message)
		if err := tx.InsertSyncEvent(ctx, store.SyncEvent{
			ID:        "sync_evt_" + uuid.NewString(),
			JobID:     updated.ID,
			Type:      eventType,
			Message:   message,
			Metadata:  map[string]any{"consumer": consumer, "status": status, "sourceRefs": syncSourceRefCount(updated.Checkpoint)},
			CreatedAt: now,
		}); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "sync-"+status, updated.ID),
			OrganizationID: updated.OrganizationID,
			UserID:         updated.UserID,
			ConnectionID:   updated.ConnectionID,
			EventType:      "connection.sync." + status,
			ProviderKey:    updated.ProviderKey,
			Metadata:       map[string]any{"syncJobId": updated.ID, "consumer": consumer, "sourceRefs": syncSourceRefCount(updated.Checkpoint)},
			CreatedAt:      now,
		})
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	eventType, _ := workerSyncEvent(status, body.Message)
	if connection, lookupErr := cfg.Repo.GetConnection(ctx, updated.ConnectionID); lookupErr == nil {
		publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration."+eventType, connection, map[string]any{
			"syncJobId": updated.ID,
			"consumer":  consumer,
			"status":    status,
		})
	}
	return updated, nil
}

func normalizeWorkerSyncStatus(status string) string {
	switch strings.TrimSpace(status) {
	case "", "running":
		return "running"
	case "completed", "failed", "cancelled":
		return strings.TrimSpace(status)
	default:
		return ""
	}
}

func workerSyncEvent(status, message string) (string, string) {
	status = normalizeWorkerSyncStatus(status)
	if strings.TrimSpace(message) != "" {
		switch status {
		case "completed":
			return "sync.completed", strings.TrimSpace(message)
		case "failed":
			return "sync.failed", strings.TrimSpace(message)
		case "cancelled":
			return "sync.cancelled", strings.TrimSpace(message)
		default:
			return "sync.checkpoint", strings.TrimSpace(message)
		}
	}
	switch status {
	case "completed":
		return "sync.completed", "Sync job completed."
	case "failed":
		return "sync.failed", "Sync job failed."
	case "cancelled":
		return "sync.cancelled", "Sync job was cancelled by worker."
	default:
		return "sync.checkpoint", "Sync checkpoint advanced."
	}
}

func syncClaimTargetAllowed(target string) bool {
	switch strings.TrimSpace(target) {
	case "finspo-core", "data-plane-v2":
		return true
	default:
		return false
	}
}

func mergeSyncSourceRefs(checkpoint map[string]any, incoming []syncSourceRef) map[string]any {
	if len(incoming) == 0 {
		return checkpoint
	}
	merged := cloneAnyMap(checkpoint)
	refs := sourceRefMaps(merged["sourceRefs"])
	seen := map[string]struct{}{}
	for _, ref := range refs {
		if key := sourceRefMapKey(ref); key != "" {
			seen[key] = struct{}{}
		}
	}
	for _, source := range incoming {
		ref := sourceRefMap(source)
		key := sourceRefMapKey(ref)
		if key == "" {
			continue
		}
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		refs = append(refs, ref)
	}
	merged["sourceRefs"] = refs
	return merged
}

func sourceRefMaps(value any) []map[string]any {
	switch items := value.(type) {
	case []map[string]any:
		out := make([]map[string]any, 0, len(items))
		for _, item := range items {
			copied := map[string]any{}
			for key, value := range item {
				if strings.TrimSpace(key) != "" {
					copied[key] = value
				}
			}
			out = append(out, copied)
		}
		return out
	case []any:
		out := make([]map[string]any, 0, len(items))
		for _, item := range items {
			ref, ok := item.(map[string]any)
			if !ok {
				continue
			}
			copied := map[string]any{}
			for key, value := range ref {
				if strings.TrimSpace(key) != "" {
					copied[key] = value
				}
			}
			out = append(out, copied)
		}
		return out
	default:
		return []map[string]any{}
	}
}

func sourceRefMap(source syncSourceRef) map[string]any {
	ref := map[string]any{}
	putSafeRefString(ref, "provider", source.Provider, 64)
	putSafeRefString(ref, "type", source.Type, 64)
	putSafeRefString(ref, "sourceId", source.SourceID, 160)
	putSafeRefString(ref, "externalId", source.ExternalID, 160)
	putSafeRefString(ref, "status", source.Status, 48)
	putSafeRefString(ref, "title", source.Title, 160)
	putSafeRefString(ref, "url", source.URL, 256)
	return ref
}

func putSafeRefString(target map[string]any, key, value string, maxLen int) {
	value = strings.TrimSpace(value)
	if value == "" {
		return
	}
	if maxLen > 0 && len(value) > maxLen {
		value = value[:maxLen]
	}
	target[key] = value
}

func sourceRefMapKey(ref map[string]any) string {
	sourceID := stringFromAny(ref["sourceId"])
	externalID := stringFromAny(ref["externalId"])
	if sourceID == "" && externalID == "" {
		return ""
	}
	return strings.Join([]string{
		stringFromAny(ref["provider"]),
		stringFromAny(ref["type"]),
		sourceID,
		externalID,
	}, "|")
}

func syncSourceRefCount(checkpoint map[string]any) int {
	return len(sourceRefMaps(checkpoint["sourceRefs"]))
}

func scopedSyncJob(c *fiber.Ctx, cfg ServerConfig, id string) (store.SyncJob, error) {
	job, err := cfg.Repo.GetSyncJob(c.UserContext(), strings.TrimSpace(id))
	if err != nil {
		return store.SyncJob{}, err
	}
	if err := auth.AssertOrgAccess(c, job.OrganizationID); err != nil {
		return store.SyncJob{}, err
	}
	return job, nil
}

func writeSSE(w *bufio.Writer, eventName string, data any) {
	payload, err := json.Marshal(data)
	if err != nil {
		payload = []byte(`{"error":"marshal_failed"}`)
	}
	_, _ = w.WriteString("event: " + strings.TrimSpace(eventName) + "\n")
	_, _ = w.WriteString("data: " + string(payload) + "\n\n")
}

func rateLimitHandlers(cfg config.Config) []fiber.Handler {
	if !cfg.RateLimitEnabled {
		return nil
	}
	max := cfg.RateLimitMax
	if max <= 0 {
		max = 120
	}
	expiration := cfg.RateLimitWindow
	if expiration <= 0 {
		expiration = time.Minute
	}
	return []fiber.Handler{
		limiter.New(limiter.Config{
			Max:        max,
			Expiration: expiration,
			LimitReached: func(c *fiber.Ctx) error {
				return apiError(c, fiber.StatusTooManyRequests, "rate_limited", "Too many requests. Please retry shortly.")
			},
		}),
	}
}

func chainHandlers(prefix []fiber.Handler, handlers ...fiber.Handler) []fiber.Handler {
	out := append([]fiber.Handler{}, prefix...)
	return append(out, handlers...)
}

// verifyProviderWebhook fails closed: a provider webhook is accepted only when
// its signature scheme is configured AND the signature verifies. An unset
// secret used to mean "accept anything", which let unsigned payloads into the
// normalize/store/event pipeline. ALLOW_UNVERIFIED_WEBHOOKS=true is a dev-only
// escape hatch for local testing without provider secrets.
func verifyProviderWebhook(c *fiber.Ctx, cfg config.Config, providerKey string) error {
	switch providerKey {
	case "github":
		if strings.TrimSpace(cfg.GitHubWebhookSecret) == "" {
			return unverifiedWebhookError(cfg, providerKey)
		}
		return verifyGitHubWebhookSignature(c, cfg.GitHubWebhookSecret)
	case "shopify":
		if strings.TrimSpace(cfg.ShopifyWebhookSecret) == "" {
			return unverifiedWebhookError(cfg, providerKey)
		}
		return verifyShopifyWebhookSignature(c, cfg.ShopifyWebhookSecret)
	case "slack":
		if strings.TrimSpace(cfg.SlackSigningSecret) == "" {
			return unverifiedWebhookError(cfg, providerKey)
		}
		return verifySlackWebhookSignature(c, cfg.SlackSigningSecret, time.Now)
	case "stripe":
		if strings.TrimSpace(cfg.StripeWebhookSecret) == "" {
			return unverifiedWebhookError(cfg, providerKey)
		}
		return verifyStripeWebhookSignature(c, cfg.StripeWebhookSecret)
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		secret := firstNonEmpty(cfg.MetaWebhookSecret, cfg.FacebookClientSecret, cfg.InstagramClientSecret)
		if strings.TrimSpace(secret) == "" {
			return unverifiedWebhookError(cfg, providerKey)
		}
		return verifyGitHubWebhookSignature(c, secret)
	default:
		// No signature scheme implemented for this provider — reject rather
		// than trust an unauthenticated payload.
		return unverifiedWebhookError(cfg, providerKey)
	}
}

func webhookRequiresResolvedOrganization(providerKey string) bool {
	switch providerKey {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads", "slack":
		return true
	default:
		return false
	}
}

func isMetaWebhookProvider(providerKey string) bool {
	switch providerKey {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return true
	default:
		return false
	}
}

func unverifiedWebhookError(cfg config.Config, providerKey string) error {
	if cfg.AllowUnverifiedWebhooks {
		return nil
	}
	return fmt.Errorf(
		"webhook signature verification is not configured for provider %q; rejecting unverified webhook",
		providerKey,
	)
}

func verifyProviderWebhookChallenge(c *fiber.Ctx, cfg config.Config, providerKey string) (string, error) {
	switch providerKey {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		if strings.TrimSpace(cfg.MetaWebhookVerifyToken) == "" {
			return "", fmt.Errorf("META_WEBHOOK_VERIFY_TOKEN is required for Meta webhook verification")
		}
		mode := strings.TrimSpace(c.Query("hub.mode"))
		token := strings.TrimSpace(c.Query("hub.verify_token"))
		challenge := strings.TrimSpace(c.Query("hub.challenge"))
		if mode != "subscribe" {
			return "", fmt.Errorf("hub.mode must be subscribe")
		}
		if token == "" || token != strings.TrimSpace(cfg.MetaWebhookVerifyToken) {
			return "", fmt.Errorf("hub.verify_token is invalid")
		}
		if challenge == "" {
			return "", fmt.Errorf("hub.challenge is required")
		}
		return challenge, nil
	default:
		return "", fmt.Errorf("webhook challenge verification is not configured for provider %q", providerKey)
	}
}

func verifyStripeWebhookSignature(c *fiber.Ctx, secret string) error {
	signatureHeader := strings.TrimSpace(c.Get("Stripe-Signature"))
	if signatureHeader == "" {
		return errors.New("Stripe-Signature is required")
	}
	parts := strings.Split(signatureHeader, ",")
	timestamp := ""
	signatures := []string{}
	for _, part := range parts {
		key, value, ok := strings.Cut(strings.TrimSpace(part), "=")
		if !ok {
			continue
		}
		switch key {
		case "t":
			timestamp = value
		case "v1":
			signatures = append(signatures, value)
		}
	}
	if timestamp == "" || len(signatures) == 0 {
		return errors.New("Stripe-Signature is incomplete")
	}
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(timestamp))
	_, _ = mac.Write([]byte("."))
	_, _ = mac.Write(c.Body())
	expected := hex.EncodeToString(mac.Sum(nil))
	for _, signature := range signatures {
		if hmac.Equal([]byte(expected), []byte(signature)) {
			return nil
		}
	}
	return errors.New("Stripe-Signature is invalid")
}

// slackURLVerificationChallenge extracts the challenge from a Slack Events
// API url_verification handshake body; empty when the body is any other
// event. Runs AFTER signature verification — Slack signs handshakes too.
func slackURLVerificationChallenge(body []byte) string {
	var probe struct {
		Type      string `json:"type"`
		Challenge string `json:"challenge"`
	}
	if err := json.Unmarshal(body, &probe); err != nil {
		return ""
	}
	if probe.Type != "url_verification" {
		return ""
	}
	return strings.TrimSpace(probe.Challenge)
}

func verifySlackWebhookSignature(c *fiber.Ctx, secret string, now func() time.Time) error {
	signature := strings.TrimSpace(c.Get("X-Slack-Signature"))
	timestamp := strings.TrimSpace(c.Get("X-Slack-Request-Timestamp"))
	if signature == "" {
		return errors.New("X-Slack-Signature is required")
	}
	if timestamp == "" {
		return errors.New("X-Slack-Request-Timestamp is required")
	}
	parsed, err := strconv.ParseInt(timestamp, 10, 64)
	if err != nil {
		return errors.New("X-Slack-Request-Timestamp is invalid")
	}
	if now == nil {
		now = time.Now
	}
	delta := now().Unix() - parsed
	if delta < 0 {
		delta = -delta
	}
	if delta > 300 {
		return errors.New("X-Slack-Request-Timestamp is outside replay window")
	}
	base := "v0:" + timestamp + ":" + string(c.Body())
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(base))
	expected := "v0=" + hex.EncodeToString(mac.Sum(nil))
	if hmac.Equal([]byte(expected), []byte(signature)) {
		return nil
	}
	return errors.New("X-Slack-Signature is invalid")
}

func verifyGitHubWebhookSignature(c *fiber.Ctx, secret string) error {
	signature := strings.TrimSpace(c.Get("X-Hub-Signature-256"))
	if signature == "" {
		return errors.New("X-Hub-Signature-256 is required")
	}
	prefix, received, ok := strings.Cut(signature, "=")
	if !ok || strings.ToLower(strings.TrimSpace(prefix)) != "sha256" || strings.TrimSpace(received) == "" {
		return errors.New("X-Hub-Signature-256 is incomplete")
	}
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write(c.Body())
	expected := hex.EncodeToString(mac.Sum(nil))
	if hmac.Equal([]byte(strings.ToLower(expected)), []byte(strings.ToLower(strings.TrimSpace(received)))) {
		return nil
	}
	return errors.New("X-Hub-Signature-256 is invalid")
}

func verifyShopifyWebhookSignature(c *fiber.Ctx, secret string) error {
	signature := strings.TrimSpace(c.Get("X-Shopify-Hmac-Sha256"))
	if signature == "" {
		return errors.New("X-Shopify-Hmac-Sha256 is required")
	}
	received, err := base64.StdEncoding.DecodeString(signature)
	if err != nil {
		return errors.New("X-Shopify-Hmac-Sha256 is invalid")
	}
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write(c.Body())
	if hmac.Equal(mac.Sum(nil), received) {
		return nil
	}
	return errors.New("X-Shopify-Hmac-Sha256 is invalid")
}

func normalizeWebhookEvent(c *fiber.Ctx, cfg ServerConfig, providerKey string) (hotpath.WebhookNormalization, error) {
	input := hotpath.WebhookInput{
		ProviderKey: providerKey,
		Headers:     webhookHeaders(c),
		Body:        append([]byte(nil), c.Body()...),
	}
	if cfg.HotPath != nil {
		normalized, err := cfg.HotPath.NormalizeWebhook(c.UserContext(), input)
		if err == nil {
			return normalized, nil
		}
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("provider", providerKey).Msg("webhook hot path unavailable; using go fallback")
		}
	}
	return hotpath.NormalizeWebhook(input)
}

func webhookHeaders(c *fiber.Ctx) map[string]string {
	return hotpath.WebhookHeaders(map[string]string{
		"Stripe-Signature":          c.Get("Stripe-Signature"),
		"X-Slack-Signature":         c.Get("X-Slack-Signature"),
		"X-Slack-Request-Timestamp": c.Get("X-Slack-Request-Timestamp"),
		"X-Hub-Signature-256":       c.Get("X-Hub-Signature-256"),
		"X-GitHub-Delivery":         c.Get("X-GitHub-Delivery"),
		"X-GitHub-Event":            c.Get("X-GitHub-Event"),
		"X-Shopify-Hmac-Sha256":     c.Get("X-Shopify-Hmac-Sha256"),
		"X-Shopify-Topic":           c.Get("X-Shopify-Topic"),
		"X-Webhook-Signature":       c.Get("X-Webhook-Signature"),
		"X-Event-Type":              c.Get("X-Event-Type"),
		"X-Org-ID":                  c.Get("X-Org-ID"),
	})
}

func signatureHash(c *fiber.Ctx) string {
	signature := firstNonEmpty(
		c.Get("Stripe-Signature"),
		c.Get("X-Slack-Signature"),
		c.Get("X-Hub-Signature-256"),
		c.Get("X-GitHub-Delivery"),
		c.Get("X-Shopify-Hmac-Sha256"),
		c.Get("X-Webhook-Signature"),
	)
	if signature == "" {
		return ""
	}
	sum := sha256.Sum256([]byte(signature))
	return hex.EncodeToString(sum[:])
}

func tenantScopedWebhookEventID(baseID, connectionID string) string {
	sum := sha256.Sum256([]byte(baseID + "\x00" + connectionID))
	return baseID + ":tenant:" + hex.EncodeToString(sum[:8])
}

func webhookEventID(providerKey, eventType, sigHash string, payload map[string]any, body []byte) string {
	identity := firstNonEmpty(
		stringFromAny(payload["id"]),
		stringFromAny(payload["eventId"]),
		stringFromAny(payload["event_id"]),
		stringFromAny(payload["deliveryId"]),
		stringFromAny(payload["delivery_id"]),
		sigHash,
	)
	if identity == "" {
		bodyHash := sha256.Sum256(body)
		identity = hex.EncodeToString(bodyHash[:])
	}
	sum := sha256.Sum256([]byte(providerKey + "|" + eventType + "|" + identity))
	return "wh_" + providers.NormalizeKey(providerKey) + "_" + hex.EncodeToString(sum[:])[:32]
}

func organizationIDForSCIMTokenRequest(c *fiber.Ctx, requested string) (string, error) {
	organizationID := strings.TrimSpace(requested)
	if !auth.IsInternalCall(c) {
		principal, ok := auth.PrincipalFromContext(c)
		if !ok {
			return "", auth.NewError(fiber.StatusUnauthorized, "unauthorized", "Authentication required")
		}
		organizationID = principal.OrganizationID
	}
	if organizationID == "" {
		return "", auth.NewError(fiber.StatusBadRequest, "organization_required", "organizationId is required.")
	}
	if err := auth.AssertOrgAccess(c, organizationID); err != nil {
		return "", err
	}
	return organizationID, nil
}

func userIDForRequest(c *fiber.Ctx) string {
	if principal, ok := auth.PrincipalFromContext(c); ok {
		return strings.TrimSpace(principal.UserID)
	}
	return ""
}

func scimBearerAuthorized(c *fiber.Ctx, cfg ServerConfig, organizationID string) bool {
	authHeader := strings.TrimSpace(c.Get("Authorization"))
	if !strings.HasPrefix(authHeader, "Bearer ") {
		return false
	}
	token := strings.TrimSpace(strings.TrimPrefix(authHeader, "Bearer "))
	if token == "" {
		return false
	}
	if strings.TrimSpace(organizationID) != "" && cfg.Repo != nil {
		scimToken, err := cfg.Repo.FindActiveSCIMTokenByHash(c.UserContext(), strings.TrimSpace(organizationID), scimTokenHash(token))
		if err == nil {
			_ = cfg.Repo.MarkSCIMTokenUsed(c.UserContext(), scimToken.ID, time.Now().UTC())
			return true
		}
	}
	expected := strings.TrimSpace(cfg.Config.SCIMBearerToken)
	if len(cfg.Config.SCIMBearerTokens) > 0 {
		expected = strings.TrimSpace(cfg.Config.SCIMBearerTokens[strings.TrimSpace(organizationID)])
	}
	return expected != "" && hmac.Equal([]byte(token), []byte(expected))
}

func scimTokenHash(token string) string {
	sum := sha256.Sum256([]byte(strings.TrimSpace(token)))
	return hex.EncodeToString(sum[:])
}

func scimTokenPrefix(token string) string {
	token = strings.TrimSpace(token)
	if len(token) <= 12 {
		return token
	}
	return token[:12]
}

func scimEventType(method, resourcePath string) string {
	resource := strings.ToLower(strings.Trim(resourcePath, "/"))
	if resource == "" {
		resource = "root"
	}
	resource = strings.ReplaceAll(resource, "/", ".")
	return "scim." + strings.ToLower(method) + "." + resource
}

func scimResponse(c *fiber.Ctx, event store.WebhookEvent, resourcePath string, payload map[string]any) error {
	resourceType := scimResourceType(resourcePath)
	switch c.Method() {
	case fiber.MethodGet:
		return c.JSON(fiber.Map{
			"schemas":      []string{"urn:ietf:params:scim:api:messages:2.0:ListResponse"},
			"totalResults": 0,
			"startIndex":   1,
			"itemsPerPage": 0,
			"Resources":    []fiber.Map{},
		})
	case fiber.MethodDelete:
		return c.SendStatus(fiber.StatusNoContent)
	case fiber.MethodPost:
		return c.Status(fiber.StatusCreated).JSON(scimResource(event, resourceType, payload))
	default:
		return c.JSON(scimResource(event, resourceType, payload))
	}
}

func scimResource(event store.WebhookEvent, resourceType string, payload map[string]any) fiber.Map {
	return fiber.Map{
		"schemas":    []string{"urn:ietf:params:scim:schemas:core:2.0:" + resourceType},
		"id":         firstNonEmpty(stringFromAny(payload["id"]), stringFromAny(payload["externalId"]), event.ID),
		"externalId": stringFromAny(payload["externalId"]),
		"userName":   stringFromAny(payload["userName"]),
		"active":     payload["active"],
		"meta": fiber.Map{
			"resourceType": resourceType,
			"created":      event.ReceivedAt,
			"lastModified": event.ReceivedAt,
		},
	}
}

func scimResourceType(resourcePath string) string {
	switch {
	case strings.HasPrefix(strings.ToLower(resourcePath), "groups"):
		return "Group"
	default:
		return "User"
	}
}

func scimError(c *fiber.Ctx, status int, scimType, detail string) error {
	return c.Status(status).JSON(fiber.Map{
		"schemas":  []string{"urn:ietf:params:scim:api:messages:2.0:Error"},
		"scimType": scimType,
		"detail":   detail,
		"status":   status,
	})
}

func recordTokenLease(ctx context.Context, cfg ServerConfig, token oauth.AccessTokenResult, consumer string) error {
	connection, err := cfg.Repo.GetConnection(ctx, token.ConnectionID)
	if err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("connection_id", token.ConnectionID).Msg("record token lease connection lookup")
		}
		return err
	}
	lease := store.TokenLease{
		ID:             "lease_" + uuid.NewString(),
		OrganizationID: connection.OrganizationID,
		ConnectionID:   connection.ID,
		UserID:         connection.UserID,
		ProviderKey:    connection.ProviderKey,
		ConnectorType:  connection.ConnectorType,
		Consumer:       strings.TrimSpace(consumer),
		ExpiresAt:      token.ExpiresAt,
		CreatedAt:      time.Now().UTC(),
	}
	if err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		if err := tx.InsertTokenLease(ctx, lease); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             "audit:integration:token-lease:" + lease.ID,
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "connection.token_leased",
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"consumer":  lease.Consumer,
				"leaseId":   lease.ID,
				"expiresAt": lease.ExpiresAt,
			},
		})
	}); err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("connection_id", connection.ID).Msg("record token lease with audit")
		}
		return err
	}
	publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration.token_lease_created", connection, map[string]any{
		"consumer":  lease.Consumer,
		"leaseId":   lease.ID,
		"expiresAt": lease.ExpiresAt,
	})
	return nil
}

func tokenLeaseConsumerAllowed(cfg config.Config, consumer string) bool {
	consumer = strings.TrimSpace(consumer)
	if consumer == "" {
		return false
	}
	if len(cfg.TokenLeaseConsumers) == 0 {
		return true
	}
	for _, allowed := range cfg.TokenLeaseConsumers {
		if strings.EqualFold(strings.TrimSpace(allowed), consumer) {
			return true
		}
	}
	return false
}

func filterConnectionsByCategory(connections []store.Connection, category string) []store.Connection {
	category = strings.ToLower(strings.TrimSpace(category))
	if category == "" || category == "all" {
		return connections
	}
	filtered := make([]store.Connection, 0, len(connections))
	for _, connection := range connections {
		provider, ok := providers.Find(firstNonEmpty(connection.ProviderKey, connection.ConnectorType))
		if !ok || provider.Category != category {
			continue
		}
		filtered = append(filtered, connection)
	}
	return filtered
}

func buildGDPRExport(ctx context.Context, cfg ServerConfig, organizationID, userID string) (fiber.Map, error) {
	connections, err := cfg.Repo.ListConnections(ctx, store.ConnectionFilter{
		OrganizationID: strings.TrimSpace(organizationID),
		UserID:         strings.TrimSpace(userID),
	})
	if err != nil {
		return nil, err
	}
	connectionIDs := map[string]struct{}{}
	exportedConnections := make([]fiber.Map, 0, len(connections))
	exportedConsents := []store.ConnectionConsent{}
	for _, connection := range connections {
		connectionIDs[connection.ID] = struct{}{}
		exportedConnections = append(exportedConnections, redactedConnectionExport(connection))
		consents, err := cfg.Repo.ListConnectionConsents(ctx, connection.ID)
		if err != nil {
			return nil, err
		}
		exportedConsents = append(exportedConsents, consents...)
	}
	jobs, err := cfg.Repo.ListSyncJobs(ctx, store.SyncJobFilter{OrganizationID: strings.TrimSpace(organizationID)})
	if err != nil {
		return nil, err
	}
	exportedJobs := make([]store.SyncJob, 0, len(jobs))
	for _, job := range jobs {
		if strings.TrimSpace(userID) != "" {
			if _, ok := connectionIDs[job.ConnectionID]; !ok {
				continue
			}
		}
		exportedJobs = append(exportedJobs, job)
	}
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		OrganizationID: strings.TrimSpace(organizationID),
		UserID:         strings.TrimSpace(userID),
		EventType:      "gdpr.integration.exported",
		Metadata: map[string]any{
			"connectionCount": len(exportedConnections),
			"syncJobCount":    len(exportedJobs),
			"tokenPolicy":     "tokens_not_exported",
		},
	})
	return fiber.Map{
		"organizationId": strings.TrimSpace(organizationID),
		"userId":         strings.TrimSpace(userID),
		"generatedAt":    time.Now().UTC(),
		"policy": fiber.Map{
			"tokens":        "not_exported",
			"sourceContent": "owned_by_data_plane",
			"providerData":  "provider_content_not_stored_by_integration_corev2",
		},
		"connections": exportedConnections,
		"consents":    exportedConsents,
		"syncJobs":    exportedJobs,
	}, nil
}

func runGDPRDelete(ctx context.Context, cfg ServerConfig, body gdprRequestBody) (fiber.Map, error) {
	organizationID := strings.TrimSpace(body.OrganizationID)
	userID := strings.TrimSpace(body.UserID)
	now := time.Now().UTC()
	deletedIDs := []string{}
	revokedConsentIDs := []string{}
	err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		connections, err := tx.ListConnections(ctx, store.ConnectionFilter{
			OrganizationID: organizationID,
			UserID:         userID,
		})
		if err != nil {
			return err
		}
		for _, connection := range connections {
			consents, err := tx.ListConnectionConsents(ctx, connection.ID)
			if err != nil {
				return err
			}
			for _, consent := range consents {
				consent.Granted = false
				consent.RevokedAt = &now
				updated, updateErr := tx.UpsertConnectionConsent(ctx, consent)
				if updateErr != nil {
					return updateErr
				}
				revokedConsentIDs = append(revokedConsentIDs, updated.ID)
			}
			if connection.DeletedAt != nil {
				continue
			}
			deleted, deleteErr := tx.MarkConnectionDeleted(ctx, connection.ID)
			if deleteErr != nil {
				return deleteErr
			}
			deletedIDs = append(deletedIDs, deleted.ID)
			if err := persist(store.AuditEvent{
				ID:             "audit:integration:gdpr-connection-deleted:" + deleted.ID,
				OrganizationID: deleted.OrganizationID,
				UserID:         deleted.UserID,
				ConnectionID:   deleted.ID,
				EventType:      "gdpr.integration.connection_deleted",
				ProviderKey:    deleted.ProviderKey,
				Metadata: map[string]any{
					"reason":          strings.TrimSpace(body.Reason),
					"tokenPolicy":     "encrypted_tokens_cleared",
					"consentsRevoked": len(consents),
				},
				CreatedAt: now,
			}); err != nil {
				return err
			}
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "gdpr-delete-completed", organizationID, userID),
			OrganizationID: organizationID,
			UserID:         userID,
			EventType:      "gdpr.integration.delete_completed",
			Metadata: map[string]any{
				"connectionCount": len(deletedIDs),
				"consentCount":    len(revokedConsentIDs),
				"reason":          strings.TrimSpace(body.Reason),
			},
			CreatedAt: now,
		})
	})
	if err != nil {
		return nil, err
	}
	return fiber.Map{
		"organizationId":      organizationID,
		"userId":              userID,
		"deletedConnections":  deletedIDs,
		"revokedConsents":     revokedConsentIDs,
		"tokens":              "cleared",
		"sourceContentAction": "orchestrate_data_plane_delete",
		"completedAt":         now,
	}, nil
}

func redactedConnectionExport(connection store.Connection) fiber.Map {
	return fiber.Map{
		"id":                   connection.ID,
		"providerKey":          connection.ProviderKey,
		"connectorType":        connection.ConnectorType,
		"organizationId":       connection.OrganizationID,
		"workspaceId":          connection.WorkspaceID,
		"userId":               connection.UserID,
		"userEmail":            connection.UserEmail,
		"status":               connection.Status,
		"displayName":          connection.DisplayName,
		"providerAccountId":    connection.ProviderAccountID,
		"tenantId":             connection.TenantID,
		"providerContext":      connection.ProviderContext,
		"capabilities":         connection.Capabilities,
		"scopes":               connection.Scopes,
		"accessTokenExpiresAt": connection.AccessTokenExpiresAt,
		"lastRefreshedAt":      connection.LastRefreshedAt,
		"lastSyncStatus":       connection.LastSyncStatus,
		"createdAt":            connection.CreatedAt,
		"updatedAt":            connection.UpdatedAt,
		"deletedAt":            connection.DeletedAt,
		"tokenPolicy":          "redacted",
	}
}

func executeConnectionAction(c *fiber.Ctx, cfg ServerConfig, connectionID string, body actionBody) (actions.ExecuteResult, error) {
	if cfg.Actions == nil {
		return actions.ExecuteResult{}, errActionUnavailable
	}
	ctx := c.UserContext()
	connection, err := cfg.Repo.GetConnection(ctx, connectionID)
	if err != nil {
		return actions.ExecuteResult{}, err
	}
	if err := auth.AssertOrgAccess(c, connection.OrganizationID); err != nil {
		return actions.ExecuteResult{}, err
	}
	if err := requireActionCapability(c, connection, body); err != nil {
		return actions.ExecuteResult{}, err
	}
	idempotencyKey := strings.TrimSpace(body.IdempotencyKey)
	writeAction := actionRequiresApproval(connection.ProviderKey, body.Operation)
	if writeAction {
		if _, err := requireWritePresenter(c); err != nil {
			return actions.ExecuteResult{}, err
		}
	}
	if writeAction && !validActionIdempotencyKey(idempotencyKey) {
		return actions.ExecuteResult{}, errActionIdempotencyRequired
	}
	requestSHA256 := ""
	var verifiedAttestation attestation.Verified
	var actionReceipt store.ActionReceipt
	if writeAction {
		var err error
		requestSHA256, err = actionRequestSHA256(connection, body)
		if err != nil {
			return actions.ExecuteResult{}, err
		}
		verifiedAttestation, err = verifyWriteAttestation(c, cfg, connection, body, requestSHA256)
		if err != nil {
			return actions.ExecuteResult{}, err
		}
		claimedReceipt, acquired, err := cfg.Repo.ClaimActionReceipt(ctx, store.ActionReceipt{
			OrganizationID:    connection.OrganizationID,
			IdempotencyKey:    idempotencyKey,
			RequestSHA256:     requestSHA256,
			ConnectionID:      connection.ID,
			ProviderKey:       connection.ProviderKey,
			Operation:         strings.TrimSpace(body.Operation),
			AttestationIssuer: verifiedAttestation.Issuer,
			AttestationKeyID:  verifiedAttestation.KeyID,
			AuthorizationKind: verifiedAttestation.AuthorizationKind,
			AuthorizationID:   verifiedAttestation.AuthorizationID,
			ApprovalID:        verifiedAttestation.ApprovalID,
			ActionID:          verifiedAttestation.ActionID,
			ActorID:           verifiedAttestation.ActorID,
			AttestationJTI:    verifiedAttestation.JWTID,
			PayloadSHA256:     verifiedAttestation.PayloadSHA256,
		})
		if err != nil {
			if errors.Is(err, store.ErrConflict) {
				return actions.ExecuteResult{}, errActionIdempotencyConflict
			}
			return actions.ExecuteResult{}, err
		}
		actionReceipt = claimedReceipt
		if !acquired {
			if !actionReceiptMatchesAuthorization(actionReceipt, connection, body, verifiedAttestation, requestSHA256) {
				return actions.ExecuteResult{}, errActionIdempotencyConflict
			}
			if actionReceipt.Status == "completed" {
				result := map[string]any{}
				if actionReceipt.ProviderMessageID != "" {
					result["provider_message_id"] = actionReceipt.ProviderMessageID
				}
				return actions.ExecuteResult{
					ProviderKey: actionReceipt.ProviderKey,
					Operation:   actionReceipt.Operation,
					Result:      result,
				}, nil
			}
			if actionReceipt.Status == "executing" {
				if reconcileErr := markActionOutcomeUnknown(ctx, cfg, connection, actionReceipt); reconcileErr != nil && cfg.Logger != nil {
					cfg.Logger.Error().Err(reconcileErr).Str("connection_id", connection.ID).Msg("stale provider action could not be durably marked unknown")
				}
			}
			if actionReceipt.Status != "pending" {
				return actions.ExecuteResult{}, errActionOutcomeUnknown
			}
		}
	}
	token, err := cfg.OAuth.AccessTokenForConnection(ctx, connection.ID)
	if err != nil {
		if writeAction {
			return actions.ExecuteResult{}, errActionPreProviderRetryable
		}
		return actions.ExecuteResult{}, err
	}
	if writeAction {
		if err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			executingReceipt, err := tx.BeginActionReceiptExecution(ctx, connection.OrganizationID, idempotencyKey)
			if err != nil {
				return err
			}
			actionReceipt = executingReceipt
			return persist(store.AuditEvent{
				ID:             actionAuditEventID("requested", actionReceipt),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "connection.action.requested",
				ProviderKey:    connection.ProviderKey,
				Metadata: map[string]any{
					"operation": strings.TrimSpace(body.Operation),
				},
			})
		}); err != nil {
			return actions.ExecuteResult{}, errActionPreProviderRetryable
		}
	}
	result, err := cfg.Actions.Execute(ctx, actions.ExecuteInput{
		Connection:  connection,
		AccessToken: token.AccessToken,
		Operation:   body.Operation,
		Params:      body.Params,
		Body:        body.Body,
	})
	if err != nil {
		if writeAction {
			unknownErr := markActionOutcomeUnknown(ctx, cfg, connection, actionReceipt)
			if unknownErr != nil && cfg.Logger != nil {
				cfg.Logger.Error().Err(unknownErr).Str("connection_id", connection.ID).Msg("provider action outcome could not be durably marked unknown")
			}
		}
		return actions.ExecuteResult{}, err
	}
	if writeAction {
		if err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
			completedReceipt, err := tx.CompleteActionReceipt(ctx, connection.OrganizationID, idempotencyKey, actionProviderMessageID(result.Result))
			if err != nil {
				return err
			}
			actionReceipt = completedReceipt
			return persist(store.AuditEvent{
				ID:             actionAuditEventID("executed", actionReceipt),
				OrganizationID: connection.OrganizationID,
				UserID:         connection.UserID,
				ConnectionID:   connection.ID,
				EventType:      "connection.action.executed",
				ProviderKey:    connection.ProviderKey,
				Metadata: map[string]any{
					"operation": result.Operation,
				},
			})
		}); err != nil {
			// The provider may already have accepted the action. Never turn a
			// receipt persistence failure into a blind retry.
			return actions.ExecuteResult{}, errActionOutcomeUnknown
		}
	} else {
		recordAuditEvent(ctx, cfg, store.AuditEvent{
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "connection.action.executed",
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"operation": result.Operation,
			},
		})
	}
	return result, nil
}

func markActionOutcomeUnknown(
	ctx context.Context,
	cfg ServerConfig,
	connection store.Connection,
	receipt store.ActionReceipt,
) error {
	return withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		if err := tx.MarkActionReceiptUnknown(ctx, connection.OrganizationID, receipt.IdempotencyKey); err != nil {
			return err
		}
		return persist(store.AuditEvent{
			ID:             actionAuditEventID("unknown", receipt),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "connection.action.unknown",
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"operation": strings.TrimSpace(receipt.Operation),
			},
		})
	})
}

func actionAuditEventID(stage string, receipt store.ActionReceipt) string {
	fields := [...]string{
		receipt.OrganizationID,
		receipt.IdempotencyKey,
		receipt.RequestSHA256,
		receipt.ConnectionID,
		receipt.ProviderKey,
		receipt.Operation,
		receipt.AttestationIssuer,
		receipt.AuthorizationKind,
		receipt.AuthorizationID,
		receipt.ApprovalID,
		receipt.ActionID,
		receipt.ActorID,
		receipt.PayloadSHA256,
	}
	var canonical strings.Builder
	for _, field := range fields {
		canonical.WriteString(strconv.Itoa(len(field)))
		canonical.WriteByte(':')
		canonical.WriteString(field)
	}
	digest := sha256.Sum256([]byte(canonical.String()))
	return "audit:integration:action-" + strings.TrimSpace(stage) + ":" + hex.EncodeToString(digest[:])
}

func actionReceiptMatchesAuthorization(receipt store.ActionReceipt, connection store.Connection, body actionBody, verified attestation.Verified, payloadSHA256 string) bool {
	return receipt.OrganizationID == connection.OrganizationID &&
		receipt.IdempotencyKey == strings.TrimSpace(body.IdempotencyKey) &&
		receipt.RequestSHA256 == payloadSHA256 &&
		receipt.ConnectionID == connection.ID &&
		receipt.ProviderKey == connection.ProviderKey &&
		receipt.Operation == strings.TrimSpace(body.Operation) &&
		receipt.AttestationIssuer == verified.Issuer &&
		receipt.AuthorizationKind == verified.AuthorizationKind &&
		receipt.AuthorizationID == verified.AuthorizationID &&
		receipt.ApprovalID == verified.ApprovalID &&
		receipt.ActionID == verified.ActionID &&
		receipt.ActorID == verified.ActorID &&
		receipt.PayloadSHA256 == verified.PayloadSHA256
}

func requireActionCapability(c *fiber.Ctx, connection store.Connection, body actionBody) error {
	required, sensitive := requiredCapabilityForOperation(connection.ProviderKey, body.Operation)
	if required == "" {
		return errActionOperationUnsupported
	}
	hasRequiredCapability := hasCapability(connection.Capabilities, required) || (len(connection.Capabilities) == 0 && !sensitive)
	if !hasRequiredCapability {
		return auth.NewError(
			fiber.StatusForbidden,
			"capability_required",
			"Connection does not grant the capability required for this provider action: "+required,
		)
	}
	return nil
}

func verifyWriteAttestation(c *fiber.Ctx, cfg ServerConfig, connection store.Connection, body actionBody, payloadSHA256 string) (attestation.Verified, error) {
	principal, err := requireWritePresenter(c)
	if err != nil {
		return attestation.Verified{}, err
	}
	compact := body.WriteAttestation
	if strings.TrimSpace(compact) == "" {
		return attestation.Verified{}, errActionAttestationRequired
	}
	if cfg.WriteAttestations == nil {
		return attestation.Verified{}, errActionAttestationUnconfigured
	}
	verified, err := cfg.WriteAttestations.Verify(compact, attestation.Binding{
		PresenterService: strings.TrimSpace(principal.UserID),
		OrganizationID:   strings.TrimSpace(connection.OrganizationID),
		ConnectionID:     strings.TrimSpace(connection.ID),
		ProviderKey:      strings.TrimSpace(connection.ProviderKey),
		Operation:        strings.TrimSpace(body.Operation),
		Params:           body.Params,
		Body:             body.Body,
		PayloadSHA256:    payloadSHA256,
		IdempotencyKey:   strings.TrimSpace(body.IdempotencyKey),
	})
	if err != nil {
		return attestation.Verified{}, errActionAttestationInvalid
	}
	return verified, nil
}

func requireWritePresenter(c *fiber.Ctx) (auth.Principal, error) {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || principal.PrincipalType != "service" || !principal.HasScope("integration:write") {
		return auth.Principal{}, errActionAttestationAuthority
	}
	return principal, nil
}

func validActionIdempotencyKey(value string) bool {
	if len(value) < 16 || len(value) > 200 {
		return false
	}
	for _, char := range value {
		if (char < 'a' || char > 'z') && (char < 'A' || char > 'Z') &&
			(char < '0' || char > '9') && char != '-' && char != '_' && char != ':' && char != '.' {
			return false
		}
	}
	return true
}

func actionRequestSHA256(connection store.Connection, body actionBody) (string, error) {
	return attestation.PayloadSHA256(attestation.Binding{
		OrganizationID: connection.OrganizationID,
		ConnectionID:   connection.ID,
		ProviderKey:    connection.ProviderKey,
		Operation:      body.Operation,
		Params:         body.Params,
		Body:           body.Body,
	})
}

func actionProviderMessageID(value any) string {
	return actionProviderMessageIDAtDepth(value, 0)
}

func actionProviderMessageIDAtDepth(value any, depth int) string {
	if depth > 4 {
		return ""
	}
	switch typed := value.(type) {
	case map[string]any:
		for _, key := range []string{"provider_message_id", "providerMessageId", "message_id", "messageId", "id", "ts"} {
			if candidate, ok := typed[key].(string); ok && strings.TrimSpace(candidate) != "" {
				return strings.TrimSpace(candidate)
			}
		}
		for _, key := range []string{"message", "messages", "data", "result"} {
			if candidate := actionProviderMessageIDAtDepth(typed[key], depth+1); candidate != "" {
				return candidate
			}
		}
	case []any:
		for _, item := range typed {
			if candidate := actionProviderMessageIDAtDepth(item, depth+1); candidate != "" {
				return candidate
			}
		}
	}
	return ""
}

func requiredCapabilityForOperation(providerKey, operation string) (string, bool) {
	normalized := strings.TrimSpace(strings.ToLower(operation))
	switch providerKey {
	case "microsoft":
		switch normalized {
		case "profile", "microsoft.profile":
			return "profile.read", false
		case "calendar.events", "microsoft.calendar.events":
			return "calendar.read", true
		case "mail.messages", "microsoft.mail.messages":
			return "mail.read", true
		case "drive.files", "microsoft.drive.files":
			return "sharepoint.read", false
		case "mail.send", "microsoft.mail.send":
			return "mail.send", true
		}
	case "slack":
		switch normalized {
		case "channels.list", "slack.channels.list":
			return "channels.read", false
		case "user", "slack.user", "users.info", "slack.users.info", "users.list", "slack.users.list":
			return "users.read", false
		case "messages.list", "slack.messages.list":
			return "channels.history", true
		case "message.send", "slack.message.send":
			return "messages.write", true
		}
	case "google":
		switch normalized {
		case "profile", "google.profile":
			return "profile.read", false
		case "gmail.messages", "google.gmail.messages":
			return "gmail.read", true
		case "gmail.send", "google.gmail.send":
			return "gmail.send", true
		case "calendar.events", "google.calendar.events":
			return "calendar.read", true
		case "drive.files", "google.drive.files":
			return "drive.read", false
		}
	case "github":
		switch normalized {
		case "user", "github.user", "emails", "github.emails":
			return "profile.read", false
		case "orgs", "github.orgs", "teams", "github.teams":
			return "org.read", false
		case "repos", "github.repos", "repo", "github.repo":
			return "repo.public.read", false
		case "contents.get", "github.contents.get", "readme.get", "github.readme.get", "branches", "github.branches":
			return "repo.contents.read", true
		case "commits", "github.commits":
			return "commits.read", true
		case "pulls", "pulls.list", "github.pulls", "github.pulls.list":
			return "pulls.read", true
		case "issues", "issues.list", "github.issues", "github.issues.list":
			return "issues.read", true
		case "issues.create", "github.issues.create", "issues.update", "github.issues.update", "issues.comment.create", "github.issues.comment.create":
			return "issues.write", true
		}
	case "notion":
		switch normalized {
		case "user", "notion.user":
			return "workspace.read", false
		case "databases", "notion.databases", "pages", "notion.pages":
			return "content.read", true
		}
	case "shopify":
		switch normalized {
		case "shop", "shopify.shop":
			return "store.read", false
		case "products", "shopify.products":
			return "products.read", false
		case "orders", "shopify.orders":
			return "orders.read", true
		}
	case "stripe":
		switch normalized {
		case "account", "stripe.account":
			return "account.read", false
		case "customers", "stripe.customers":
			return "customers.read", true
		case "subscriptions", "stripe.subscriptions", "invoices", "stripe.invoices":
			return "billing.read", true
		}
	case "linkedin":
		switch normalized {
		case "profile", "linkedin.profile":
			return "social.profile.read", false
		case "identity", "linkedin.identity":
			return "social.profile.verify", true
		case "verification.report", "linkedin.verification.report":
			return "social.verification.read", true
		case "organization.acls", "organizations", "linkedin.organization.acls":
			return "social.organization.read", true
		case "posts.list", "linkedin.posts.list":
			return "social.post.read", true
		case "posts.create", "linkedin.posts.create":
			return "social.post.write", true
		case "events.get", "linkedin.events.get":
			return "social.organization.read", true
		case "events.create", "linkedin.events.create", "events.update", "linkedin.events.update":
			return "social.events.manage", true
		case "ads.accounts", "linkedin.ads.accounts", "ads.account", "linkedin.ads.account":
			return "social.ads.read", true
		case "ads.campaigns", "linkedin.ads.campaigns", "ads.campaign", "linkedin.ads.campaign":
			return "social.ads.read", true
		case "ads.campaign.create", "linkedin.ads.campaign.create", "ads.campaign.update", "linkedin.ads.campaign.update":
			return "social.ads.manage", true
		case "conversions.list", "linkedin.conversions.list":
			return "social.ads.read", true
		case "conversions.create", "linkedin.conversions.create":
			return "social.conversions.manage", true
		case "lead.forms", "linkedin.lead.forms", "lead.responses", "linkedin.lead.responses":
			return "social.leads.read", true
		}
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		switch normalized {
		case "profile", "meta.profile", "facebook.profile",
			"pages.list", "facebook.pages", "meta.pages",
			"ads.businesses", "meta.businesses":
			return "social.profile.read", false
		case "instagram.accounts", "meta.instagram.accounts",
			"instagram.media.status", "instagram.insights":
			return "social.instagram.read", true
		case "pages.post", "facebook.page.post":
			return "social.post.write", true
		case "pages.photo", "facebook.page.photo",
			"instagram.media.create", "instagram.media.publish":
			return "social.media.upload", true
		case "whatsapp.business_accounts", "whatsapp.accounts", "whatsapp.phone_numbers", "whatsapp.templates", "whatsapp.messages.send":
			return "social.whatsapp.manage", true
		case "messenger.messages.send", "messenger.subscribed_apps", "instagram.messages.send":
			return "social.messenger.manage", true
		case "ads.adaccounts", "meta.adaccounts", "ads.campaigns", "ads.campaign.create", "app_ads.campaign.create",
			"ads.adsets", "ads.ads", "ads.creatives":
			return "social.ads.manage", true
		case "audience_network.apps", "meta.audience_network.apps":
			return "social.audience_network.read", true
		case "ads.insights":
			return "social.analytics.read", true
		case "catalogs.list", "catalog.list", "catalog.products", "catalog.product.upsert", "catalog.batch":
			return "social.catalog.manage", true
		case "threads.profile", "threads.container.create", "threads.publish":
			return "social.threads.manage", true
		case "threads.container.status":
			return "social.threads.manage", true
		case "threads.insights":
			return "social.analytics.read", true
		case "oembed", "meta.oembed":
			return "social.oembed.read", false
		case "live.create", "facebook.live.create", "live.list", "facebook.live.list", "live.get", "facebook.live.get":
			return "social.live.manage", true
		}
	case "snapchat":
		switch normalized {
		case "organizations", "snapchat.organizations",
			"profile.stories", "snapchat.profile.stories",
			"profile.spotlights", "snapchat.profile.spotlights",
			"profile.saved_stories", "snapchat.profile.saved_stories",
			"spotlight.get", "snapchat.spotlight.get":
			return "social.profile.read", false
		case "adaccounts", "snapchat.adaccounts",
			"media.list", "snapchat.media.list",
			"creatives.list", "snapchat.creatives.list",
			"ads.media.create", "snapchat.ads.media.create",
			"ads.creative.create", "snapchat.ads.creative.create":
			return "social.ads.manage", true
		case "ads.stats", "snapchat.ads.stats":
			return "social.analytics.read", true
		case "profile.media.create", "snapchat.profile.media.create":
			return "social.media.upload", true
		case "story.post", "snapchat.story.post",
			"spotlight.post", "snapchat.spotlight.post",
			"saved_story.create", "snapchat.saved_story.create":
			return "social.post.write", true
		}
	case "okta":
		switch normalized {
		case "org", "okta.org":
			return "tenant.read", false
		case "users", "okta.users", "groups", "okta.groups":
			return "directory.read", true
		case "user.suspend", "okta.user.suspend", "user.activate", "okta.user.activate":
			return "directory.write", true
		}
	}
	return "", false
}

func actionRequiresApproval(providerKey, operation string) bool {
	normalized := strings.TrimSpace(strings.ToLower(operation))
	switch providerKey {
	case "microsoft":
		return normalized == "mail.send" || normalized == "microsoft.mail.send"
	case "slack":
		return normalized == "message.send" || normalized == "slack.message.send"
	case "google":
		return normalized == "gmail.send" || normalized == "google.gmail.send"
	case "github":
		switch normalized {
		case "issues.create", "github.issues.create",
			"issues.update", "github.issues.update",
			"issues.comment.create", "github.issues.comment.create":
			return true
		default:
			return false
		}
	case "notion":
		return normalized == "content.write" || normalized == "notion.content.write"
	case "shopify":
		return normalized == "orders.write" || normalized == "shopify.orders.write"
	case "linkedin":
		switch normalized {
		case "posts.create", "linkedin.posts.create",
			"events.create", "linkedin.events.create",
			"events.update", "linkedin.events.update",
			"ads.campaign.create", "linkedin.ads.campaign.create",
			"ads.campaign.update", "linkedin.ads.campaign.update",
			"conversions.create", "linkedin.conversions.create":
			return true
		default:
			return false
		}
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		switch normalized {
		case "pages.post", "facebook.page.post",
			"pages.photo", "facebook.page.photo",
			"live.create", "facebook.live.create",
			"instagram.media.create", "instagram.media.publish",
			"whatsapp.messages.send",
			"messenger.messages.send", "messenger.subscribed_apps",
			"instagram.messages.send",
			"ads.campaign.create", "app_ads.campaign.create",
			"catalog.product.upsert", "catalog.batch",
			"threads.container.create", "threads.publish":
			return true
		default:
			return false
		}
	case "snapchat":
		switch normalized {
		case "ads.media.create", "snapchat.ads.media.create",
			"ads.creative.create", "snapchat.ads.creative.create",
			"profile.media.create", "snapchat.profile.media.create",
			"story.post", "snapchat.story.post",
			"spotlight.post", "snapchat.spotlight.post",
			"saved_story.create", "snapchat.saved_story.create":
			return true
		default:
			return false
		}
	case "okta":
		return normalized == "user.suspend" || normalized == "okta.user.suspend" ||
			normalized == "user.activate" || normalized == "okta.user.activate"
	default:
		return false
	}
}

func hasCapability(capabilities []string, required string) bool {
	for _, capability := range capabilities {
		if strings.TrimSpace(capability) == required {
			return true
		}
	}
	return false
}

var (
	errActionUnavailable             = errors.New("provider actions are not configured")
	errActionOperationUnsupported    = errors.New("provider operation is not supported")
	errActionIdempotencyRequired     = errors.New("provider writes require a valid idempotency key")
	errActionIdempotencyConflict     = errors.New("idempotency key or authorization was already used for a different provider action")
	errActionOutcomeUnknown          = errors.New("provider action outcome is unknown; reconciliation is required")
	errActionPreProviderRetryable    = errors.New("provider action was not attempted; retry with a fresh attestation")
	errActionAttestationAuthority    = errors.New("provider writes require a scoped service bearer")
	errActionAttestationRequired     = errors.New("provider writes require a signed write attestation")
	errActionAttestationInvalid      = errors.New("provider-write attestation is invalid")
	errActionAttestationUnconfigured = errors.New("provider-write attestation verifier is not configured")
)

func actionError(c *fiber.Ctx, err error) error {
	switch {
	case errors.Is(err, store.ErrNotFound):
		return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this action.")
	case errors.Is(err, errActionUnavailable):
		return apiError(c, fiber.StatusServiceUnavailable, "actions_unavailable", "Provider actions are not configured.")
	case errors.Is(err, errActionOperationUnsupported):
		return apiError(c, fiber.StatusBadRequest, "operation_not_supported", "The provider operation is not supported.")
	case errors.Is(err, errActionIdempotencyRequired):
		return apiError(c, fiber.StatusBadRequest, "idempotency_key_required", "Provider writes require a valid idempotencyKey.")
	case errors.Is(err, errActionIdempotencyConflict):
		return apiError(c, fiber.StatusConflict, "idempotency_conflict", "The idempotency key is already bound to a different action.")
	case errors.Is(err, errActionOutcomeUnknown):
		return apiError(c, fiber.StatusConflict, "action_outcome_unknown", "The provider action may already have been accepted; reconcile before retrying.")
	case errors.Is(err, errActionPreProviderRetryable):
		return apiError(c, fiber.StatusServiceUnavailable, "action_pre_provider_retryable", "The provider action was not attempted. Retry with a fresh signed attestation.")
	case errors.Is(err, errActionAttestationAuthority):
		return apiError(c, fiber.StatusForbidden, "write_attestation_authority_required", "Provider writes require a scoped service bearer.")
	case errors.Is(err, errActionAttestationRequired):
		return apiError(c, fiber.StatusForbidden, "write_attestation_required", "Provider writes require a signed write attestation.")
	case errors.Is(err, errActionAttestationInvalid):
		return apiError(c, fiber.StatusForbidden, "write_attestation_invalid", "Provider-write attestation is invalid.")
	case errors.Is(err, errActionAttestationUnconfigured):
		return apiError(c, fiber.StatusServiceUnavailable, "write_attestation_unconfigured", "Provider-write attestation verification is unavailable.")
	default:
		var authErr auth.Error
		if errors.As(err, &authErr) {
			return authAwareError(c, authErr)
		}
		return apiError(c, fiber.StatusBadGateway, "action_failed", err.Error())
	}
}

func authAwareError(c *fiber.Ctx, err error) error {
	var authErr auth.Error
	if errors.As(err, &authErr) {
		return apiError(c, authErr.Status, authErr.Code, authErr.Message)
	}
	return apiError(c, fiber.StatusInternalServerError, "internal_error", err.Error())
}

func recordUsage(cfg ServerConfig, orgID, metric string, quantity float64, metadata map[string]any) {
	if cfg.Billing == nil {
		return
	}
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := cfg.Billing.RecordUsage(ctx, orgID, controlplane.UsageEvent{
			Metric:   metric,
			Quantity: quantity,
			Metadata: metadata,
		}); err != nil && cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("org_id", orgID).Str("metric", metric).Msg("record billing usage")
		}
	}()
}

func prepareAuditEvent(ctx context.Context, event store.AuditEvent) store.AuditEvent {
	if strings.TrimSpace(event.ID) == "" {
		event.ID = "audit_" + uuid.NewString()
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	if strings.TrimSpace(event.RequestID) == "" {
		event.RequestID = requestIDFromContext(ctx)
	}
	return event
}

func auditMutationID(_ context.Context, operation string, resourceParts ...string) string {
	// X-Request-ID is correlation data, not an idempotency credential. Include
	// fresh server entropy so a caller cannot reuse a request ID to suppress the
	// audit row for a distinct mutation.
	identity := append([]string{strings.TrimSpace(operation), uuid.NewString()}, resourceParts...)
	digest := sha256.Sum256([]byte(strings.Join(identity, "\x00")))
	return "audit:integration:" + strings.TrimSpace(operation) + ":" + hex.EncodeToString(digest[:16])
}

func dispatchAuditOutbox(ctx context.Context, cfg ServerConfig, event store.AuditEvent) {
	if cfg.AuditOutbox != nil {
		forwardContext, cancel := context.WithTimeout(ctx, 3*time.Second)
		defer cancel()
		if _, err := cfg.AuditOutbox.DispatchOne(forwardContext); err != nil && cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("org_id", event.OrganizationID).Str("event", event.EventType).Msg("audit event retained for retry")
		}
	}
}

func withAuditTransaction(
	ctx context.Context,
	cfg ServerConfig,
	mutation func(store.AuditTransaction, func(store.AuditEvent) error) error,
) error {
	if cfg.Repo == nil {
		return fmt.Errorf("repository is unavailable")
	}
	persisted := make([]store.AuditEvent, 0, 1)
	err := cfg.Repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		persist := func(event store.AuditEvent) error {
			event = prepareAuditEvent(ctx, event)
			if err := tx.InsertAuditEvent(ctx, event); err != nil {
				return err
			}
			persisted = append(persisted, event)
			return nil
		}
		return mutation(tx, persist)
	})
	if err != nil {
		return err
	}
	for _, event := range persisted {
		dispatchAuditOutbox(ctx, cfg, event)
	}
	return nil
}

func recordAuditEvent(ctx context.Context, cfg ServerConfig, event store.AuditEvent) {
	event = prepareAuditEvent(ctx, event)
	if cfg.Repo == nil {
		return
	}
	if err := cfg.Repo.InsertAuditEvent(ctx, event); err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("org_id", event.OrganizationID).Str("event_type", event.EventType).Msg("record local audit event")
		}
		return
	}
	dispatchAuditOutbox(ctx, cfg, event)
}

func auditSubject(event store.AuditEvent) string {
	if strings.TrimSpace(event.ConnectionID) != "" {
		return "integration_connection"
	}
	if strings.HasPrefix(strings.TrimSpace(event.EventType), "gdpr.") {
		return "gdpr"
	}
	return "integration"
}

func auditDetails(event store.AuditEvent) map[string]any {
	details := map[string]any{}
	if strings.TrimSpace(event.ProviderKey) != "" {
		details["providerKey"] = strings.TrimSpace(event.ProviderKey)
	}
	if strings.TrimSpace(event.ConnectionID) != "" {
		details["connectionId"] = strings.TrimSpace(event.ConnectionID)
	}
	if len(event.Metadata) > 0 {
		details["metadata"] = cloneAuditMetadata(event.Metadata)
	}
	return details
}

func cloneAuditMetadata(input map[string]any) map[string]any {
	out := make(map[string]any, len(input))
	for key, value := range input {
		out[key] = value
	}
	return out
}

func redactSensitiveStrings(input []string) []string {
	out := make([]string, 0, len(input))
	for _, item := range input {
		if strings.TrimSpace(item) != "" {
			out = append(out, item)
		}
	}
	return out
}

func publishIntegrationEvent(ctx context.Context, cfg ServerConfig, eventType string, connection store.Connection, data map[string]any) {
	if cfg.Events == nil {
		return
	}
	if err := cfg.Events.Publish(ctx, events.Event{
		Type:           eventType,
		OrganizationID: connection.OrganizationID,
		WorkspaceID:    connection.WorkspaceID,
		UserID:         connection.UserID,
		ConnectionID:   connection.ID,
		ProviderKey:    connection.ProviderKey,
		Data:           data,
	}); err != nil && cfg.Logger != nil {
		cfg.Logger.Warn().Err(err).Str("type", eventType).Str("connection_id", connection.ID).Msg("publish integration event")
	}
}

func attachMetaSDKConfig(catalog []providers.Provider, cfg config.Config) []providers.Provider {
	meta := providers.MetaSDK{
		Enabled:       strings.TrimSpace(cfg.MetaJSSDKAppID) != "",
		AppID:         strings.TrimSpace(cfg.MetaJSSDKAppID),
		APIVersion:    strings.TrimSpace(cfg.MetaJSSDKAPIVersion),
		Locale:        strings.TrimSpace(cfg.MetaJSSDKLocale),
		LoginConfigID: strings.TrimSpace(cfg.MetaBusinessLoginConfigID),
	}
	if meta.APIVersion == "" {
		meta.APIVersion = "v25.0"
	}
	if meta.Locale == "" {
		meta.Locale = "en_US"
	}

	out := make([]providers.Provider, 0, len(catalog))
	for _, provider := range catalog {
		if isMetaSDKProvider(provider.Key) {
			provider.MetaSDK = &meta
		}
		out = append(out, provider)
	}
	return out
}

func isMetaSDKProvider(providerKey string) bool {
	switch providers.NormalizeKey(providerKey) {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return true
	default:
		return false
	}
}

func callbackHTML(result oauth.CallbackResult) string {
	status := "error"
	if result.Success {
		status = "success"
	}
	payload, _ := json.Marshal(map[string]string{
		"type":         "velion.integration.connected",
		"status":       status,
		"sessionToken": result.SessionID,
		"connectionId": result.ConnectionID,
		"provider":     result.ProviderKey,
		"errorCode":    result.ErrorCode,
		"message":      result.Message,
	})
	payloadScript := strings.ReplaceAll(string(payload), "<", "\\u003c")
	message := html.EscapeString(result.Message)
	returnURL := html.EscapeString(result.ReturnURL)
	heading := "Tilkoblingen feilet"
	if result.Success {
		heading = "Tilkoblet"
	}
	// Auto-close ONLY on success. Providers with Cross-Origin-Opener-Policy
	// (X, LinkedIn, Microsoft) sever window.opener, so the postMessage below is
	// best-effort — the SPA's authoritative signal is the connect-session
	// status endpoint. On error the window stays open so the user can actually
	// read what went wrong (it used to close itself after 250ms).
	autoClose := ""
	if result.Success {
		autoClose = `
  window.setTimeout(function () {
    try { window.close(); } catch (_) {}
    if ("` + returnURL + `") window.location.href = "` + returnURL + `";
  }, 900);`
	}
	return `<!doctype html>
<html>
<head><meta charset="utf-8"><title>Velion integration</title></head>
<body style="font-family: system-ui, sans-serif; padding: 32px; max-width: 32rem;">
<script>
(function () {
  var payload = ` + payloadScript + `;
  try {
    if (window.opener) window.opener.postMessage(payload, "*");
  } catch (_) {}` + autoClose + `
})();
</script>
<h1 style="font-size: 1.1rem; margin: 0 0 8px;">` + heading + `</h1>
<p style="margin: 0 0 20px; color: #444;">` + message + `</p>
<button onclick="window.close()" style="padding: 8px 16px; font: inherit; cursor: pointer;">Lukk vinduet</button>
</body>
</html>`
}

func sessionStatus(session store.ConnectSession) string {
	if session.ConsumedAt != nil && session.ErrorCode != "" {
		return "failed"
	}
	if session.ConsumedAt != nil {
		return "completed"
	}
	return "pending"
}

func success(c *fiber.Ctx, data any) error {
	body := fiber.Map{"success": true, "data": data}
	if meta := responseMeta(c); len(meta) > 0 {
		body["meta"] = meta
	}
	return c.JSON(body)
}

func responseMeta(c *fiber.Ctx) fiber.Map {
	meta := fiber.Map{}
	if requestID := requestIDFromFiber(c); requestID != "" {
		meta["requestId"] = requestID
	}
	return meta
}

func storeError(c *fiber.Ctx, err error, code string) error {
	if errors.Is(err, store.ErrNotFound) {
		return apiError(c, fiber.StatusNotFound, code, "Resource was not found.")
	}
	return apiError(c, fiber.StatusInternalServerError, code, err.Error())
}

func apiError(c *fiber.Ctx, status int, code, message string) error {
	body := fiber.Map{
		"success": false,
		"error": fiber.Map{
			"code":    code,
			"message": message,
		},
	}
	if requestID := requestIDFromFiber(c); requestID != "" {
		body["meta"] = fiber.Map{"requestId": requestID}
	}
	return c.Status(status).JSON(body)
}

func requestIDMiddleware(c *fiber.Ctx) error {
	requestID := sanitizeRequestID(c.Get(requestIDHeader))
	if requestID == "" {
		requestID = "req_" + uuid.NewString()
	}
	c.Locals("requestID", requestID)
	c.Set(requestIDHeader, requestID)
	c.SetUserContext(context.WithValue(c.UserContext(), requestIDContextKey{}, requestID))
	return c.Next()
}

func requestIDFromFiber(c *fiber.Ctx) string {
	if c == nil {
		return ""
	}
	if requestID, ok := c.Locals("requestID").(string); ok {
		return requestID
	}
	return ""
}

func requestIDFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	if requestID, ok := ctx.Value(requestIDContextKey{}).(string); ok {
		return requestID
	}
	return ""
}

func sanitizeRequestID(input string) string {
	input = strings.TrimSpace(input)
	if input == "" || len(input) > 128 {
		return ""
	}
	for _, char := range input {
		if char < 33 || char > 126 {
			return ""
		}
	}
	return input
}

type requestMetricKey struct {
	method string
	path   string
	status int
}

type requestMetricValue struct {
	count           int64
	durationSeconds float64
}

type requestMetrics struct {
	mu   sync.Mutex
	rows map[requestMetricKey]requestMetricValue
}

func newRequestMetrics() *requestMetrics {
	return &requestMetrics{rows: map[requestMetricKey]requestMetricValue{}}
}

func (m *requestMetrics) middleware(c *fiber.Ctx) error {
	started := time.Now()
	err := c.Next()
	status := c.Response().StatusCode()
	path := c.Path()
	if route := c.Route(); route != nil && strings.TrimSpace(route.Path) != "" {
		path = route.Path
	}
	m.record(requestMetricKey{
		method: c.Method(),
		path:   path,
		status: status,
	}, time.Since(started).Seconds())
	return err
}

func (m *requestMetrics) record(key requestMetricKey, durationSeconds float64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	value := m.rows[key]
	value.count++
	value.durationSeconds += durationSeconds
	m.rows[key] = value
}

func (m *requestMetrics) handler(c *fiber.Ctx) error {
	c.Set("Content-Type", "text/plain; version=0.0.4")
	return c.SendString(m.render())
}

func (m *requestMetrics) render() string {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out strings.Builder
	out.WriteString("# HELP integration_http_requests_total Total HTTP requests handled by integration-corev2.\n")
	out.WriteString("# TYPE integration_http_requests_total counter\n")
	out.WriteString("# HELP integration_http_request_duration_seconds_total Total request duration seconds by route.\n")
	out.WriteString("# TYPE integration_http_request_duration_seconds_total counter\n")
	for key, value := range m.rows {
		labels := `method="` + prometheusLabel(key.method) + `",path="` + prometheusLabel(key.path) + `",status="` + strconv.Itoa(key.status) + `"`
		out.WriteString("integration_http_requests_total{" + labels + "} " + strconv.FormatInt(value.count, 10) + "\n")
		out.WriteString("integration_http_request_duration_seconds_total{" + labels + "} " + strconv.FormatFloat(value.durationSeconds, 'f', 6, 64) + "\n")
	}
	return out.String()
}

func renderAuditOutboxMetrics(ctx context.Context, monitor AuditOutboxMonitor) string {
	status, err := monitor.Status(ctx)
	if err != nil {
		return "# HELP integration_audit_outbox_status_error Whether audit outbox status collection failed.\n" +
			"# TYPE integration_audit_outbox_status_error gauge\n" +
			"integration_audit_outbox_status_error 1\n"
	}
	return "# HELP integration_audit_outbox_pending Pending durable audit events.\n" +
		"# TYPE integration_audit_outbox_pending gauge\n" +
		"integration_audit_outbox_pending " + strconv.Itoa(status.Pending) + "\n" +
		"# HELP integration_audit_outbox_terminal Terminal audit events requiring operator recovery.\n" +
		"# TYPE integration_audit_outbox_terminal gauge\n" +
		"integration_audit_outbox_terminal " + strconv.Itoa(status.Terminal) + "\n" +
		"# HELP integration_audit_outbox_oldest_pending_seconds Age of the oldest pending audit event.\n" +
		"# TYPE integration_audit_outbox_oldest_pending_seconds gauge\n" +
		"integration_audit_outbox_oldest_pending_seconds " + strconv.FormatFloat(status.OldestPendingAge.Seconds(), 'f', -1, 64) + "\n" +
		"# HELP integration_audit_outbox_oldest_terminal_seconds Age of the oldest terminal audit event.\n" +
		"# TYPE integration_audit_outbox_oldest_terminal_seconds gauge\n" +
		"integration_audit_outbox_oldest_terminal_seconds " + strconv.FormatFloat(status.OldestTerminalAge.Seconds(), 'f', -1, 64) + "\n" +
		"# HELP integration_audit_outbox_status_error Whether audit outbox status collection failed.\n" +
		"# TYPE integration_audit_outbox_status_error gauge\n" +
		"integration_audit_outbox_status_error 0\n"
}

func prometheusLabel(input string) string {
	input = strings.ReplaceAll(input, `\`, `\\`)
	return strings.ReplaceAll(input, `"`, `\"`)
}

func requestLogger(logger zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		err := c.Next()
		logger.Info().
			Str("request_id", requestIDFromFiber(c)).
			Str("method", c.Method()).
			Str("path", c.Path()).
			Int("status", c.Response().StatusCode()).
			Msg("request")
		return err
	}
}

func Shutdown(ctx context.Context, app *fiber.App) error {
	done := make(chan error, 1)
	go func() {
		done <- app.Shutdown()
	}()
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return ctx.Err()
	}
}
