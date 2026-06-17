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
	"html"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/limiter"
	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/controlplane"
	"github.com/triodelab/integration-corev2/internal/discovery"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/hotpath"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

type ServerConfig struct {
	Config    config.Config
	Repo      store.Repository
	OAuth     *oauth.Service
	Auth      auth.TokenVerifier
	Org       auth.OrgPlanClient
	Billing   *controlplane.BillingClient
	Audit     *controlplane.AuditClient
	Events    events.Publisher
	Discovery *discovery.Service
	Actions   *actions.Service
	HotPath   hotpath.WebhookNormalizer
	Logger    *zerolog.Logger
}

const requestIDHeader = "X-Request-ID"

type requestIDContextKey struct{}

func NewServer(cfg ServerConfig) *fiber.App {
	app := fiber.New(fiber.Config{DisableStartupMessage: true})
	app.Use(requestIDMiddleware)
	metrics := newRequestMetrics()
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
		return c.JSON(fiber.Map{
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
		})
	})

	internalAuth := auth.InternalOnly(auth.Config{
		APIKey:       cfg.Config.InternalAPIKey,
		APIKeyHeader: cfg.Config.InternalAPIKeyHeader,
	})
	app.Get("/metrics", internalAuth, metrics.handler)
	internalOrBearerAuth := auth.InternalOrBearer(auth.Config{
		APIKey:        cfg.Config.InternalAPIKey,
		APIKeyHeader:  cfg.Config.InternalAPIKeyHeader,
		TokenVerifier: cfg.Auth,
	})
	proPlanAuth := []fiber.Handler{internalOrBearerAuth, auth.RequirePlan(cfg.Org, "pro")}
	rateLimited := rateLimitHandlers(cfg.Config)

	app.Get("/api/v1/providers", func(c *fiber.Ctx) error {
		return success(c, fiber.Map{"providers": providers.WithReadiness(providers.Catalog(), cfg.Config.ProviderReadiness())})
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
		if err == nil && result.Success && strings.TrimSpace(result.ConnectionID) != "" {
			if connection, lookupErr := cfg.Repo.GetConnection(c.UserContext(), result.ConnectionID); lookupErr == nil {
				forwardAuditEvent(c.UserContext(), cfg, store.AuditEvent{
					OrganizationID: connection.OrganizationID,
					UserID:         connection.UserID,
					ConnectionID:   connection.ID,
					EventType:      "connection.created",
					ProviderKey:    connection.ProviderKey,
					Metadata: map[string]any{
						"capabilities": connection.Capabilities,
						"scopes":       redactSensitiveStrings(connection.Scopes),
					},
				})
			} else if cfg.Logger != nil {
				cfg.Logger.Warn().Err(lookupErr).Str("connection_id", result.ConnectionID).Msg("oauth callback audit connection lookup")
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
		updated, err := cfg.Repo.UpdateConnectionCapabilities(c.UserContext(), connection.ID, capabilities)
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			OrganizationID: updated.OrganizationID,
			UserID:         updated.UserID,
			ConnectionID:   updated.ID,
			EventType:      "connection.capabilities.updated",
			ProviderKey:    updated.ProviderKey,
			Metadata:       map[string]any{"capabilities": capabilities},
		})
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
		consent, err := cfg.Repo.UpsertConnectionConsent(c.UserContext(), store.ConnectionConsent{
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
		})
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "consent_update_failed", err.Error())
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      "connection.consent_changed",
			ProviderKey:    connection.ProviderKey,
			Metadata:       map[string]any{"source": source, "purpose": purpose, "granted": body.Granted},
		})
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
			Operation: body.Operation,
			Params:    body.Params,
			Body:      body.Body,
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
		deleted, err := cfg.OAuth.DisconnectConnection(c.UserContext(), c.Params("id"), "user_requested")
		if err != nil {
			return storeError(c, err, "connection_not_found")
		}
		forwardAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			OrganizationID: deleted.OrganizationID,
			UserID:         deleted.UserID,
			ConnectionID:   deleted.ID,
			EventType:      "connection.deleted",
			ProviderKey:    deleted.ProviderKey,
			Metadata:       map[string]any{"reason": "user_requested"},
		})
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
		normalized, err := normalizeWebhookEvent(c, cfg, providerKey)
		if err != nil {
			if errors.Is(err, hotpath.ErrInvalidJSON) {
				return apiError(c, fiber.StatusBadRequest, "invalid_json", "Webhook payload must be valid JSON.")
			}
			return apiError(c, fiber.StatusBadGateway, "webhook_normalize_failed", err.Error())
		}
		event := store.WebhookEvent{
			ID:             normalized.EventID,
			OrganizationID: normalized.OrganizationID,
			ProviderKey:    normalized.ProviderKey,
			EventType:      normalized.EventType,
			SignatureHash:  normalized.SignatureHash,
			Payload:        normalized.Payload,
			ReceivedAt:     time.Now().UTC(),
		}
		if err := cfg.Repo.InsertWebhookEvent(c.UserContext(), event); err != nil {
			if errors.Is(err, store.ErrConflict) {
				return success(c, fiber.Map{"accepted": true, "duplicate": true, "webhookEventId": event.ID})
			}
			return apiError(c, fiber.StatusInternalServerError, "webhook_store_failed", err.Error())
		}
		if cfg.Events != nil {
			_ = cfg.Events.Publish(c.UserContext(), events.Event{
				Type:           "velion.ingestion.integration.webhook_received",
				OrganizationID: event.OrganizationID,
				ProviderKey:    event.ProviderKey,
				Data:           map[string]any{"eventType": event.EventType, "webhookEventId": event.ID, "normalizedBy": normalized.NormalizedBy},
			})
		}
		return success(c, fiber.Map{"accepted": true, "webhookEventId": event.ID, "normalizedBy": normalized.NormalizedBy})
	})...)

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
		token, err := cfg.Repo.CreateSCIMToken(c.UserContext(), store.SCIMToken{
			ID:             "scimtok_" + uuid.NewString(),
			OrganizationID: organizationID,
			Name:           name,
			TokenPrefix:    scimTokenPrefix(bearer),
			CreatedBy:      createdBy,
			ExpiresAt:      body.ExpiresAt,
			CreatedAt:      now,
			UpdatedAt:      now,
		}, scimTokenHash(bearer))
		if err != nil {
			return apiError(c, fiber.StatusInternalServerError, "scim_token_create_failed", err.Error())
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			OrganizationID: organizationID,
			UserID:         createdBy,
			EventType:      "scim.token.created",
			ProviderKey:    "scim",
			Metadata:       map[string]any{"tokenId": token.ID, "tokenPrefix": token.TokenPrefix},
		})
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
		revoked, err := cfg.Repo.RevokeSCIMToken(c.UserContext(), organizationID, c.Params("id"), time.Now().UTC())
		if err != nil {
			return storeError(c, err, "scim_token_not_found")
		}
		recordAuditEvent(c.UserContext(), cfg, store.AuditEvent{
			OrganizationID: organizationID,
			UserID:         userIDForRequest(c),
			EventType:      "scim.token.revoked",
			ProviderKey:    "scim",
			Metadata:       map[string]any{"tokenId": revoked.ID, "tokenPrefix": revoked.TokenPrefix},
		})
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
		recordTokenLease(c.UserContext(), cfg, token, body.Consumer)
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
	Operation string         `json:"operation"`
	Params    map[string]any `json:"params"`
	Body      map[string]any `json:"body"`
}

type executeActionBody struct {
	ConnectionID string         `json:"connectionId"`
	Operation    string         `json:"operation"`
	Params       map[string]any `json:"params"`
	Body         map[string]any `json:"body"`
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
	reason := strings.TrimSpace(body.Reason)
	if reason == "" {
		reason = "manual"
	}
	mode := strings.TrimSpace(body.Mode)
	if mode == "" {
		mode = "incremental"
	}
	now := time.Now().UTC()
	job, err := cfg.Repo.CreateSyncJob(ctx, store.SyncJob{
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
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     job.ID,
		Type:      "sync.queued",
		Message:   "Sync job was queued.",
		Metadata:  map[string]any{"reason": reason, "mode": mode},
		CreatedAt: now,
	})
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		OrganizationID: connection.OrganizationID,
		UserID:         connection.UserID,
		ConnectionID:   connection.ID,
		EventType:      "connection.sync.queued",
		ProviderKey:    connection.ProviderKey,
		Metadata:       map[string]any{"syncJobId": job.ID, "reason": reason, "mode": mode},
	})
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
	running, err := cfg.Repo.UpdateSyncJob(ctx, job)
	if err != nil {
		return store.SyncJob{}, err
	}
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     running.ID,
		Type:      "sync.running",
		Message:   "Sync job is being prepared for provider handoff.",
		Metadata:  map[string]any{"providerKey": connection.ProviderKey},
		CreatedAt: now,
	})

	target := syncHandoffTarget(connection)
	next := running
	next.Status = target.status
	next.Checkpoint = mergeMetadata(next.Checkpoint, target.checkpoint)
	next.Metadata = mergeMetadata(next.Metadata, target.metadata)
	updated, err := cfg.Repo.UpdateSyncJob(ctx, next)
	if err != nil {
		return store.SyncJob{}, err
	}
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     updated.ID,
		Type:      target.eventType,
		Message:   target.message,
		Metadata:  target.metadata,
		CreatedAt: time.Now().UTC(),
	})
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
	updated, err := cfg.Repo.UpdateSyncJob(ctx, job)
	if err != nil {
		return store.SyncJob{}, err
	}
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     updated.ID,
		Type:      "sync.cancelled",
		Message:   "Sync job was cancelled.",
		Metadata:  map[string]any{"reason": reason},
		CreatedAt: now,
	})
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
	job, err := cfg.Repo.ClaimSyncJob(ctx, store.SyncJobClaim{
		Consumer:       strings.TrimSpace(body.Consumer),
		Target:         target,
		OrganizationID: strings.TrimSpace(body.OrganizationID),
		ProviderKey:    providers.NormalizeKey(body.ProviderKey),
		Checkpoint:     checkpoint,
		Metadata:       metadata,
	})
	if err != nil {
		return store.SyncJob{}, err
	}
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     job.ID,
		Type:      "sync.claimed",
		Message:   "Sync job was claimed by an internal worker.",
		Metadata:  map[string]any{"consumer": body.Consumer, "target": target},
		CreatedAt: now,
	})
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		OrganizationID: job.OrganizationID,
		UserID:         job.UserID,
		ConnectionID:   job.ConnectionID,
		EventType:      "connection.sync.claimed",
		ProviderKey:    job.ProviderKey,
		Metadata:       map[string]any{"syncJobId": job.ID, "consumer": body.Consumer, "target": target},
	})
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
	updated, err := cfg.Repo.UpdateSyncJob(ctx, job)
	if err != nil {
		return store.SyncJob{}, err
	}
	eventType, message := workerSyncEvent(status, body.Message)
	_ = cfg.Repo.InsertSyncEvent(ctx, store.SyncEvent{
		ID:        "sync_evt_" + uuid.NewString(),
		JobID:     updated.ID,
		Type:      eventType,
		Message:   message,
		Metadata:  map[string]any{"consumer": consumer, "status": status, "sourceRefs": syncSourceRefCount(updated.Checkpoint)},
		CreatedAt: now,
	})
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		OrganizationID: updated.OrganizationID,
		UserID:         updated.UserID,
		ConnectionID:   updated.ConnectionID,
		EventType:      "connection.sync." + status,
		ProviderKey:    updated.ProviderKey,
		Metadata:       map[string]any{"syncJobId": updated.ID, "consumer": consumer, "sourceRefs": syncSourceRefCount(updated.Checkpoint)},
	})
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

func verifyProviderWebhook(c *fiber.Ctx, cfg config.Config, providerKey string) error {
	switch providerKey {
	case "github":
		if strings.TrimSpace(cfg.GitHubWebhookSecret) == "" {
			return nil
		}
		return verifyGitHubWebhookSignature(c, cfg.GitHubWebhookSecret)
	case "shopify":
		if strings.TrimSpace(cfg.ShopifyWebhookSecret) == "" {
			return nil
		}
		return verifyShopifyWebhookSignature(c, cfg.ShopifyWebhookSecret)
	case "slack":
		if strings.TrimSpace(cfg.SlackSigningSecret) == "" {
			return nil
		}
		return verifySlackWebhookSignature(c, cfg.SlackSigningSecret, time.Now)
	case "stripe":
		if strings.TrimSpace(cfg.StripeWebhookSecret) == "" {
			return nil
		}
		return verifyStripeWebhookSignature(c, cfg.StripeWebhookSecret)
	default:
		return nil
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

func recordTokenLease(ctx context.Context, cfg ServerConfig, token oauth.AccessTokenResult, consumer string) {
	connection, err := cfg.Repo.GetConnection(ctx, token.ConnectionID)
	if err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("connection_id", token.ConnectionID).Msg("record token lease connection lookup")
		}
		return
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
	if err := cfg.Repo.InsertTokenLease(ctx, lease); err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("connection_id", connection.ID).Msg("record token lease")
		}
		return
	}
	recordAuditEvent(ctx, cfg, store.AuditEvent{
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
	publishIntegrationEvent(ctx, cfg, "velion.ingestion.integration.token_lease_created", connection, map[string]any{
		"consumer":  lease.Consumer,
		"leaseId":   lease.ID,
		"expiresAt": lease.ExpiresAt,
	})
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
	connections, err := cfg.Repo.ListConnections(ctx, store.ConnectionFilter{
		OrganizationID: organizationID,
		UserID:         userID,
	})
	if err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	deletedIDs := []string{}
	revokedConsentIDs := []string{}
	for _, connection := range connections {
		consents, err := cfg.Repo.ListConnectionConsents(ctx, connection.ID)
		if err != nil {
			return nil, err
		}
		for _, consent := range consents {
			consent.Granted = false
			consent.RevokedAt = &now
			updated, err := cfg.Repo.UpsertConnectionConsent(ctx, consent)
			if err != nil {
				return nil, err
			}
			revokedConsentIDs = append(revokedConsentIDs, updated.ID)
		}
		if connection.DeletedAt != nil {
			continue
		}
		deleted, err := cfg.Repo.MarkConnectionDeleted(ctx, connection.ID)
		if err != nil {
			return nil, err
		}
		deletedIDs = append(deletedIDs, deleted.ID)
		recordAuditEvent(ctx, cfg, store.AuditEvent{
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
		})
	}
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		OrganizationID: organizationID,
		UserID:         userID,
		EventType:      "gdpr.integration.delete_completed",
		Metadata: map[string]any{
			"connectionCount": len(deletedIDs),
			"consentCount":    len(revokedConsentIDs),
			"reason":          strings.TrimSpace(body.Reason),
		},
	})
	return fiber.Map{
		"organizationId":      organizationID,
		"userId":              userID,
		"deletedConnections":  deletedIDs,
		"revokedConsents":     revokedConsentIDs,
		"tokens":              "cleared",
		"sourceContentAction": "orchestrate_data_plane_delete",
		"completedAt":         time.Now().UTC(),
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
	if err := requireActionCapability(connection, body); err != nil {
		return actions.ExecuteResult{}, err
	}
	token, err := cfg.OAuth.AccessTokenForConnection(ctx, connection.ID)
	if err != nil {
		return actions.ExecuteResult{}, err
	}
	result, err := cfg.Actions.Execute(ctx, actions.ExecuteInput{
		Connection:  connection,
		AccessToken: token.AccessToken,
		Operation:   body.Operation,
		Params:      body.Params,
		Body:        body.Body,
	})
	if err != nil {
		return actions.ExecuteResult{}, err
	}
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
	return result, nil
}

func requireActionCapability(connection store.Connection, body actionBody) error {
	required, sensitive := requiredCapabilityForOperation(connection.ProviderKey, body.Operation)
	if required == "" {
		return nil
	}
	hasRequiredCapability := hasCapability(connection.Capabilities, required) || (len(connection.Capabilities) == 0 && !sensitive)
	if !hasRequiredCapability {
		return auth.NewError(
			fiber.StatusForbidden,
			"capability_required",
			"Connection does not grant the capability required for this provider action: "+required,
		)
	}
	if actionRequiresApproval(connection.ProviderKey, body.Operation) && actionApprovalRef(body) == "" {
		return auth.NewError(
			fiber.StatusForbidden,
			"approval_required",
			"Write actions require human approval metadata.",
		)
	}
	return nil
}

func actionApprovalRef(body actionBody) string {
	return firstNonEmpty(
		actionStringParam(body.Params, "approvalId"),
		actionStringParam(body.Params, "approvalRef"),
		actionStringParam(body.Body, "approvalId"),
		actionStringParam(body.Body, "approvalRef"),
	)
}

func actionStringParam(values map[string]any, key string) string {
	if values == nil {
		return ""
	}
	return stringFromAny(values[key])
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
		case "user", "github.user":
			return "profile.read", false
		case "orgs", "github.orgs", "teams", "github.teams":
			return "org.read", false
		case "repos", "github.repos", "repo", "github.repo":
			return "repo.public.read", false
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
	case "notion":
		return normalized == "content.write" || normalized == "notion.content.write"
	case "shopify":
		return normalized == "orders.write" || normalized == "shopify.orders.write"
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

var errActionUnavailable = errors.New("provider actions are not configured")

func actionError(c *fiber.Ctx, err error) error {
	switch {
	case errors.Is(err, store.ErrNotFound):
		return apiError(c, fiber.StatusNotFound, "connection_not_found", "No active connection exists for this action.")
	case errors.Is(err, errActionUnavailable):
		return apiError(c, fiber.StatusServiceUnavailable, "actions_unavailable", "Provider actions are not configured.")
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

func recordAuditEvent(ctx context.Context, cfg ServerConfig, event store.AuditEvent) {
	if strings.TrimSpace(event.ID) == "" {
		event.ID = "audit_" + uuid.NewString()
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	if cfg.Repo != nil {
		if err := cfg.Repo.InsertAuditEvent(ctx, event); err != nil && cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("org_id", event.OrganizationID).Str("event_type", event.EventType).Msg("record local audit event")
		}
	}
	forwardAuditEvent(ctx, cfg, event)
}

func forwardAuditEvent(ctx context.Context, cfg ServerConfig, event store.AuditEvent) {
	if cfg.Audit == nil {
		return
	}
	auditEvent := controlplane.AuditEvent{
		OccurredAt: event.CreatedAt,
		OrgID:      strings.TrimSpace(event.OrganizationID),
		UserID:     strings.TrimSpace(event.UserID),
		Plane:      cfg.Config.ServiceName,
		Event:      strings.TrimSpace(event.EventType),
		Subject:    auditSubject(event),
		ResourceID: strings.TrimSpace(event.ConnectionID),
		Outcome:    "ok",
		Details:    auditDetails(event),
		RequestID:  requestIDFromContext(ctx),
	}
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := cfg.Audit.RecordAudit(ctx, auditEvent); err != nil && cfg.Logger != nil {
			cfg.Logger.Warn().Err(err).Str("org_id", auditEvent.OrgID).Str("event", auditEvent.Event).Msg("forward audit event")
		}
	}()
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
	return `<!doctype html>
<html>
<head><meta charset="utf-8"><title>Velion integration</title></head>
<body style="font-family: system-ui, sans-serif; padding: 32px;">
<script>
(function () {
  var payload = ` + payloadScript + `;
  try {
    if (window.opener) window.opener.postMessage(payload, "*");
  } catch (_) {}
  window.setTimeout(function () {
    try { window.close(); } catch (_) {}
    if ("` + returnURL + `") window.location.href = "` + returnURL + `";
  }, 250);
})();
</script>
<p>` + message + `</p>
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
