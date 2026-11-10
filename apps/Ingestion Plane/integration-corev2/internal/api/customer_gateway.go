package api

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"strconv"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/customergateway"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// Customer-gateway model connections (dark, CUSTOMER_GATEWAY_CONNECTIONS_ENABLED).
//
// An organisation admin registers their own OpenAI-compatible endpoint. The API key is sealed in
// this service (AAD = the connection id) and never leaves it: it is not returned, logged, leased
// or put in an error. Model Plane calls the broker routes below with a connection id and a model;
// Integration Core makes the call. The connection is never zero-data-retention, so its ZDR
// assessment is always not_supported: a form cannot assert ZDR or residency for an endpoint this
// service does not operate. Audit rows carry ids, the model and the outcome, never content.

// Audit event types written by the customer-gateway routes. Content-free.
const (
	auditEventGatewayCreated   = "model_connection.customer_gateway_created"
	auditEventGatewayRequested = "customer_gateway_inference.requested"
	auditEventGatewayCompleted = "customer_gateway_inference.completed"
	auditEventGatewayFailed    = "customer_gateway_inference.failed"
)

const (
	customerGatewayCreatePath = "/api/v1/model-connections/customer-gateway"
	customerGatewayInferPath  = "/internal/model-connections/customer-gateway/infer"
	customerGatewayStreamPath = "/internal/model-connections/customer-gateway/infer/stream"
)

type customerGatewayCreateBody struct {
	Name     string   `json:"name"`
	BaseURL  string   `json:"base_url"`
	APIKey   string   `json:"api_key"`
	ModelIDs []string `json:"model_ids"`
}

type customerGatewayInferBody struct {
	ConnectionID string                     `json:"connection_id"`
	OrgID        string                     `json:"org_id"`
	RequestID    string                     `json:"request_id"`
	Model        string                     `json:"model"`
	Messages     []json.RawMessage          `json:"messages"`
	Params       map[string]json.RawMessage `json:"params"`
	// ZDRRequired is set by a caller whose run must keep zero data retention. A customer gateway
	// can never give it, so the call is refused before anything is sent.
	ZDRRequired bool `json:"zdr_required"`
}

type customerGatewayStreamEvent struct {
	Type      string          `json:"type"`
	RequestID string          `json:"requestId,omitempty"`
	ModelUsed string          `json:"modelUsed,omitempty"`
	Code      string          `json:"code,omitempty"`
	Message   string          `json:"message,omitempty"`
	Usage     json.RawMessage `json:"usage,omitempty"`
}

// registerCustomerGatewayRoutes mounts the registration route (an org admin's bearer token only)
// and the Model-Plane-only broker routes. Nothing is mounted unless the switch is on and the broker
// was built.
func registerCustomerGatewayRoutes(
	app *fiber.App,
	cfg ServerConfig,
	modelPlaneAuth fiber.Handler,
	bearerOnly fiber.Handler,
	rateLimited []fiber.Handler,
) {
	if !cfg.Config.CustomerGatewayConnectionsEnabled || cfg.CustomerGateway == nil {
		return
	}
	broker := cfg.CustomerGateway

	app.Post(customerGatewayCreatePath, chainHandlers(rateLimited, bearerOnly, func(c *fiber.Ctx) error {
		return customerGatewayCreate(c, cfg, broker)
	})...)

	app.Post(customerGatewayInferPath, chainHandlers(rateLimited, modelPlaneAuth, func(c *fiber.Ctx) error {
		var body customerGatewayInferBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		connection, target, request, err := validateCustomerGatewayInference(c, cfg, broker, body)
		if err != nil {
			return customerGatewayError(c, cfg, err)
		}
		requestID := customerGatewayRequestID(body)
		if err := auditCustomerGatewayRequested(c.UserContext(), cfg, connection, requestID, request.Model, false); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "gateway_audit_unavailable", "The gateway invocation could not be audited safely.")
		}
		result, err := broker.Invoke(c.UserContext(), target, request)
		if err != nil {
			auditCustomerGatewayFailed(c.UserContext(), cfg, connection, requestID, request.Model, false, err)
			return customerGatewayError(c, cfg, err)
		}
		auditCustomerGatewayCompleted(c.UserContext(), cfg, connection, requestID, firstNonEmpty(result.Model, request.Model), false, result.Status, result.Usage)
		return success(c, fiber.Map{
			"response":     json.RawMessage(result.Body),
			"requestId":    requestID,
			"providerUsed": customergateway.ProviderKey,
		})
	})...)

	app.Post(customerGatewayStreamPath, chainHandlers(rateLimited, modelPlaneAuth, func(c *fiber.Ctx) error {
		var body customerGatewayInferBody
		if err := c.BodyParser(&body); err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		connection, target, request, err := validateCustomerGatewayInference(c, cfg, broker, body)
		if err != nil {
			return customerGatewayError(c, cfg, err)
		}
		requestID := customerGatewayRequestID(body)
		if err := auditCustomerGatewayRequested(c.UserContext(), cfg, connection, requestID, request.Model, true); err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "gateway_audit_unavailable", "The gateway invocation could not be audited safely.")
		}
		streamContext := c.UserContext()
		// The gateway is called, and its refusal (rate limit, rejected key, bad request) surfaced as
		// an ordinary error, before the 200 and the ready event are committed.
		stream, err := broker.OpenStream(streamContext, target, request)
		if err != nil {
			auditCustomerGatewayFailed(streamContext, cfg, connection, requestID, request.Model, true, err)
			return customerGatewayError(c, cfg, err)
		}
		c.Set("Content-Type", "text/event-stream")
		c.Set("Cache-Control", "no-cache, no-transform")
		c.Set("Connection", "keep-alive")
		c.Set("X-Accel-Buffering", "no")
		c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
			defer stream.Close()
			writeSSE(w, "gateway.ready", customerGatewayStreamEvent{Type: "ready", RequestID: requestID, ModelUsed: request.Model})
			if err := w.Flush(); err != nil {
				auditCustomerGatewayFailed(streamContext, cfg, connection, requestID, request.Model, true, errClientDisconnected)
				return
			}
			modelUsed := request.Model
			var usage json.RawMessage
			for {
				chunk, done, nextErr := stream.Next()
				if nextErr != nil {
					auditCustomerGatewayFailed(streamContext, cfg, connection, requestID, modelUsed, true, nextErr)
					failure := customerGatewayFailureOf(cfg, nextErr)
					writeSSE(w, "gateway.error", customerGatewayStreamEvent{Type: "error", RequestID: requestID, Code: failure.Code, Message: failure.Message})
					_ = w.Flush()
					return
				}
				if done {
					break
				}
				if model, chunkUsage := customergateway.ChunkInfo(chunk); model != "" || chunkUsage != nil {
					if model != "" {
						modelUsed = model
					}
					if chunkUsage != nil {
						usage = chunkUsage
					}
				}
				// The chunk is one line of validated JSON from the gateway, relayed as it came.
				_, _ = w.WriteString("event: gateway.chunk\ndata: " + string(chunk) + "\n\n")
				if err := w.Flush(); err != nil {
					auditCustomerGatewayFailed(streamContext, cfg, connection, requestID, modelUsed, true, errClientDisconnected)
					return
				}
			}
			auditCustomerGatewayCompleted(streamContext, cfg, connection, requestID, modelUsed, true, fiber.StatusOK, usage)
			writeSSE(w, "gateway.done", customerGatewayStreamEvent{Type: "done", RequestID: requestID, ModelUsed: modelUsed, Usage: usage})
			_ = w.Flush()
		})
		return nil
	})...)
}

// errClientDisconnected marks a stream the caller stopped reading.
var errClientDisconnected = errors.New("client disconnected")

// customerGatewayCreate registers a gateway for the caller's organisation.
func customerGatewayCreate(c *fiber.Ctx, cfg ServerConfig, broker *customergateway.Broker) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || strings.TrimSpace(principal.OrganizationID) == "" || strings.TrimSpace(principal.UserID) == "" {
		return apiError(c, fiber.StatusUnauthorized, "unauthorized", "A verified user is required.")
	}
	if !zdrHasOrgAdminRole(principal.Role) {
		return apiError(c, fiber.StatusForbidden, "org_admin_required", "Only an organisation owner or admin can register a customer gateway.")
	}
	var body customerGatewayCreateBody
	if err := c.BodyParser(&body); err != nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
	}
	name, err := customergateway.ValidateName(body.Name)
	if err != nil {
		return customerGatewayError(c, cfg, err)
	}
	baseURL, err := customergateway.NormalizeBaseURL(body.BaseURL)
	if err != nil {
		return customerGatewayError(c, cfg, err)
	}
	apiKey, err := customergateway.ValidateAPIKey(body.APIKey)
	body.APIKey = ""
	if err != nil {
		return customerGatewayError(c, cfg, err)
	}
	modelIDs, err := customergateway.ValidateModelIDs(body.ModelIDs)
	if err != nil {
		return customerGatewayError(c, cfg, err)
	}
	if err := broker.CheckDestination(c.UserContext(), baseURL); err != nil {
		return customerGatewayError(c, cfg, err)
	}

	connectionID := "conn_" + uuid.NewString()
	sealed, err := broker.SealAPIKey(connectionID, apiKey)
	apiKey = ""
	if err != nil {
		return customerGatewayError(c, cfg, err)
	}
	connection := store.Connection{
		ID:             connectionID,
		ProviderKey:    customergateway.ProviderKey,
		ConnectorType:  customergateway.ConnectorType,
		OrganizationID: principal.OrganizationID,
		WorkspaceID:    principal.WorkspaceID,
		UserID:         principal.UserID,
		UserEmail:      principal.Email,
		Status:         "active",
		DisplayName:    name,
		ProviderContext: map[string]string{
			"auth_mode":                     customergateway.AuthMode,
			customergateway.ContextBaseURL:  baseURL,
			customergateway.ContextModelIDs: strings.Join(modelIDs, ","),
		},
		Capabilities:         []string{customergateway.Capability},
		EncryptedAccessToken: sealed,
		LastSyncStatus:       "connected",
	}
	var zdrRecord store.ConnectionZDR
	if err := withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var saveErr error
		connection, saveErr = tx.UpsertConnection(c.UserContext(), connection)
		if saveErr != nil {
			return saveErr
		}
		// The assessment commits with the connection: a gateway is never created without its
		// not_supported record, and a failure here rolls the connection back.
		zdrRecord, saveErr = applyCustomerGatewayZDR(c.UserContext(), tx, persist, connection)
		if saveErr != nil {
			return saveErr
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "customer-gateway-created", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventGatewayCreated,
			ProviderKey:    connection.ProviderKey,
			Metadata:       map[string]any{"modelCount": len(modelIDs)},
		})
	}); err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "connection_persist_failed", "Could not register the gateway safely.")
	}
	publishIntegrationEvent(c.UserContext(), cfg, integrationEventConnUpd, connection, map[string]any{"change": "customer_gateway_created"})
	return c.Status(fiber.StatusCreated).JSON(fiber.Map{"success": true, "data": fiber.Map{
		"connection": connection,
		"zdr": fiber.Map{
			"capability": string(zdrRecord.Capability),
			"basis":      string(zdrRecord.Basis),
			"digest":     zdrRecord.Digest,
		},
	}})
}

// applyCustomerGatewayZDR records the fixed assessment of a customer gateway: not_supported, on no
// assessed basis. The contract table is the shared, pinned Model Plane contract and has no row for
// this provider, so the assessment is built here with the same Digest function every other one
// uses. The provider is not in the catalog, so the start-up reconcile never rewrites it.
func applyCustomerGatewayZDR(
	ctx context.Context,
	tx store.AuditTransaction,
	persist func(store.AuditEvent) error,
	connection store.Connection,
) (store.ConnectionZDR, error) {
	assessor, err := zdr.Default()
	if err != nil {
		return store.ConnectionZDR{}, err
	}
	assessment := zdr.Assessment{
		ProviderKey:    customergateway.ProviderKey,
		AuthMode:       customergateway.AuthMode,
		Capability:     zdr.CapabilityNotSupported,
		Basis:          zdr.BasisNotAssessed,
		BasisRef:       customergateway.ZDRBasisRef,
		CatalogVersion: assessor.CatalogVersion(),
	}
	assessment.Digest, err = zdr.Digest(assessment.ProviderKey, assessment.AuthMode, assessment.Capability, assessment.Basis, assessment.CatalogVersion)
	if err != nil {
		return store.ConnectionZDR{}, err
	}
	result, err := tx.ApplyConnectionZDRAssessment(ctx, store.NewConnectionZDRAssessment(connection.ID, assessment))
	if err != nil {
		return store.ConnectionZDR{}, err
	}
	if result.Created || result.Changed {
		if err := persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "zdr-assessed", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventZDRAssessed,
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"change":         auditChangeZDRCreated,
				"capability":     string(result.Record.Capability),
				"basis":          string(result.Record.Basis),
				"basisRef":       result.Record.BasisRef,
				"catalogVersion": result.Record.CatalogVersion,
				"digest":         result.Record.Digest,
				"waiverRoute":    false,
			},
		}); err != nil {
			return store.ConnectionZDR{}, err
		}
	}
	return result.Record, nil
}

// validateCustomerGatewayInference resolves the request to a connection the organisation owns, an
// active gateway that lists the model, and the target to call.
func validateCustomerGatewayInference(
	c *fiber.Ctx,
	cfg ServerConfig,
	broker *customergateway.Broker,
	body customerGatewayInferBody,
) (store.Connection, customergateway.Target, customergateway.InferRequest, error) {
	request := customergateway.InferRequest{
		Model:    strings.TrimSpace(body.Model),
		Messages: body.Messages,
		Params:   body.Params,
	}
	connectionID := strings.TrimSpace(body.ConnectionID)
	orgID := strings.TrimSpace(body.OrgID)
	if connectionID == "" || orgID == "" {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusBadRequest, "invalid_gateway_request", "connection_id, org_id, model, and messages are required")
	}
	if err := customergateway.CheckRequest(request); err != nil {
		return store.Connection{}, customergateway.Target{}, request, err
	}
	if cfg.Repo == nil {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusServiceUnavailable, "repository_unavailable", "The connection store is unavailable")
	}
	connection, err := cfg.Repo.GetConnection(c.UserContext(), connectionID)
	if err != nil {
		return store.Connection{}, customergateway.Target{}, request, err
	}
	if !customergateway.IsGatewayConnection(connection) {
		return store.Connection{}, customergateway.Target{}, request, store.ErrNotFound
	}
	if connection.OrganizationID != orgID {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusForbidden, "gateway_connection_scope_mismatch", "The selected gateway connection is not owned by this organization")
	}
	if connection.Status != "active" {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusConflict, "gateway_connection_inactive", "The selected gateway connection is not active")
	}
	if !hasCapability(connection.Capabilities, customergateway.Capability) {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusForbidden, "gateway_capability_not_granted", "The gateway connection does not permit inference")
	}
	if body.ZDRRequired {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusConflict, "gateway_zdr_not_supported", "A customer gateway cannot give zero data retention")
	}
	target, err := broker.TargetFor(connection)
	if err != nil {
		return store.Connection{}, customergateway.Target{}, request, err
	}
	if !target.HasModel(request.Model) {
		return store.Connection{}, customergateway.Target{}, request, auth.NewError(fiber.StatusForbidden, "gateway_model_not_listed", "The model is not listed on this gateway connection")
	}
	return connection, target, request, nil
}

func customerGatewayRequestID(body customerGatewayInferBody) string {
	if id := strings.TrimSpace(body.RequestID); id != "" && len(id) <= 128 {
		return id
	}
	return "gw_" + uuid.NewString()
}

// auditCustomerGatewayRequested commits the "requested" row before the gateway is called, so a call
// that cannot be audited is not made.
func auditCustomerGatewayRequested(ctx context.Context, cfg ServerConfig, connection store.Connection, requestID, model string, stream bool) error {
	return withAuditTransaction(ctx, cfg, func(_ store.AuditTransaction, persist func(store.AuditEvent) error) error {
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "customer-gateway-infer-requested", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventGatewayRequested,
			ProviderKey:    connection.ProviderKey,
			Metadata:       map[string]any{"requestId": requestID, "model": model, "stream": stream},
		})
	})
}

func auditCustomerGatewayCompleted(ctx context.Context, cfg ServerConfig, connection store.Connection, requestID, model string, stream bool, upstreamStatus int, usage json.RawMessage) {
	metadata := map[string]any{"requestId": requestID, "model": model, "stream": stream, "outcome": "completed", "upstreamStatus": upstreamStatus}
	if prompt, completion, total := customergateway.TokenCounts(usage); total > 0 || prompt > 0 || completion > 0 {
		metadata["promptTokens"] = prompt
		metadata["completionTokens"] = completion
		metadata["totalTokens"] = total
	}
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		ID:             auditMutationID(ctx, "customer-gateway-infer-completed", connection.ID),
		OrganizationID: connection.OrganizationID,
		UserID:         connection.UserID,
		ConnectionID:   connection.ID,
		EventType:      auditEventGatewayCompleted,
		ProviderKey:    connection.ProviderKey,
		Metadata:       metadata,
	})
}

func auditCustomerGatewayFailed(ctx context.Context, cfg ServerConfig, connection store.Connection, requestID, model string, stream bool, cause error) {
	outcome := "failed"
	code := "gateway_failed"
	if errors.Is(cause, errClientDisconnected) {
		outcome, code = "client_disconnected", "client_disconnected"
	} else if failure, ok := customergateway.IsFailure(cause); ok {
		code = failure.Code
	}
	recordAuditEvent(ctx, cfg, store.AuditEvent{
		ID:             auditMutationID(ctx, "customer-gateway-infer-failed", connection.ID),
		OrganizationID: connection.OrganizationID,
		UserID:         connection.UserID,
		ConnectionID:   connection.ID,
		EventType:      auditEventGatewayFailed,
		ProviderKey:    connection.ProviderKey,
		Metadata:       map[string]any{"requestId": requestID, "model": model, "stream": stream, "outcome": outcome, "code": code},
	})
}

// customerGatewayFailureOf returns the classified failure of err, logging its scrubbed cause. An
// error that is not a classified failure becomes a generic one: its text is never returned.
func customerGatewayFailureOf(cfg ServerConfig, err error) *customergateway.Failure {
	if failure, ok := customergateway.IsFailure(err); ok {
		if cfg.Logger != nil && failure.Cause != "" {
			cfg.Logger.Warn().Str("code", failure.Code).Str("cause", failure.Cause).Msg("customer gateway call failed")
		}
		return failure
	}
	return &customergateway.Failure{Status: fiber.StatusBadGateway, Code: "gateway_failed", Message: "The gateway request could not be completed."}
}

// customerGatewayError writes err as the API error envelope. Only fixed codes and messages are
// returned: a gateway's own error text, and the key, never reach the caller.
func customerGatewayError(c *fiber.Ctx, cfg ServerConfig, err error) error {
	var authErr auth.Error
	switch {
	case errors.Is(err, store.ErrNotFound):
		return apiError(c, fiber.StatusNotFound, "gateway_connection_not_found", "The requested gateway connection was not found.")
	case errors.As(err, &authErr):
		return authAwareError(c, authErr)
	}
	failure := customerGatewayFailureOf(cfg, err)
	details := fiber.Map{}
	if failure.RetryAfterSeconds > 0 {
		c.Set(fiber.HeaderRetryAfter, strconv.Itoa(failure.RetryAfterSeconds))
		details["retryAfterSeconds"] = failure.RetryAfterSeconds
	}
	return apiErrorWithDetails(c, failure.Status, failure.Code, failure.Message, details)
}
