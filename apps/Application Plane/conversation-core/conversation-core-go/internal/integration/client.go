// Package integration is conversation-core's outbound client to
// integration-corev2 (Ingestion Plane). It is the adapter-confirmation point for
// the HITL act-leg: a draft.reply is only "sent" once integration-corev2 returns
// a 2xx for the per-provider send operation.
//
// Plane rule: conversation-core (Application Plane) NEVER talks to a provider
// API directly. It always goes through integration-corev2, which owns the
// OAuth-connected connection and the stored access token.
package integration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	neturl "net/url"
	"strings"
	"sync"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
)

// defaultTimeout bounds every send so a hung provider call surfaces as a
// transient failure (retryable) rather than blocking the consumer forever.
const (
	defaultTimeout          = 20 * time.Second
	authResponseMaxBytes    = 64 << 10
	actionResponseMaxBytes  = 1 << 20
	serviceTokenMaxBytes    = 16 << 10
	serviceTokenRefreshSkew = 30 * time.Second
)

// ErrUnsupportedProvider is returned (as a terminal error) when a conversation's
// channel provider has no known send operation. It is permanent — retrying will
// not help — so the executor must surface send_failed rather than loop.
var ErrUnsupportedProvider = errors.New("integration: unsupported send provider")

// SendError carries whether a failure is terminal (do not retry; surface
// send_failed) or transient (retry via JetStream redelivery). 4xx + unsupported
// provider are terminal; timeouts + 5xx + transport errors are transient.
type SendError struct {
	Terminal    bool
	SafeToRetry bool
	Status      int
	Code        string
	Message     string
	err         error
}

func (e *SendError) Error() string {
	if e == nil {
		return "integration: <nil>"
	}
	kind := "transient"
	if e.Terminal {
		kind = "terminal"
	}
	if e.Message != "" {
		return fmt.Sprintf("integration send (%s, status=%d, code=%s): %s", kind, e.Status, e.Code, e.Message)
	}
	if e.err != nil {
		return fmt.Sprintf("integration send (%s): %v", kind, e.err)
	}
	return fmt.Sprintf("integration send (%s, status=%d)", kind, e.Status)
}

func (e *SendError) Unwrap() error { return e.err }

// IsTerminal reports whether err is a permanent send failure (the executor must
// emit send_failed and ack instead of retrying forever). Any non-SendError is
// treated conservatively as transient so an unexpected error gets a retry.
func IsTerminal(err error) bool {
	var se *SendError
	if errors.As(err, &se) {
		return se.Terminal
	}
	return false
}

// IsSafeToRetry is true only for failures proven to occur before the action
// route, receipt ledger, provider adapter, or provider could be reached.
func IsSafeToRetry(err error) bool {
	var sendErr *SendError
	return errors.As(err, &sendErr) && sendErr.SafeToRetry
}

// ErrorCode returns a bounded, non-sensitive code safe for lifecycle events and
// logs. Upstream/provider response text must never cross that boundary.
func ErrorCode(err error) string {
	var sendErr *SendError
	if errors.As(err, &sendErr) {
		return sanitizeErrorCode(sendErr.Code)
	}
	return "send_failed"
}

// SendRequest is one outbound reply addressed by a resolved channel thread ref.
type SendRequest struct {
	OrgID            string
	ActorUserID      string
	Provider         string
	ConnectionID     string
	ProviderThreadID string
	BodyText         string
	BodyHTML         string
	Subject          string
	// The authorization fields are copied from the content-free outbound intent
	// claimed before this call. Send recomputes PayloadSHA256 and refuses to sign
	// if any effect differs from the durable binding.
	AuthorizationKind string
	AuthorizationID   string
	ApprovalID        string
	ActionID          string
	PayloadSHA256     string
	IdempotencyKey    string
	// To is an optional list of recipient addresses (email providers).
	To []string
}

// SendResult is the adapter confirmation: the provider message id (when the
// provider returns one) plus the operation that was executed.
type SendResult struct {
	ProviderMessageID string
	Operation         string
}

// Client speaks the integration-corev2 connections-actions contract.
type Client struct {
	baseURL           string
	apiKey            string
	apiKeyHdr         string
	authCoreURL       string
	serviceID         string
	serviceCredential string
	httpClient        *http.Client
	now               func() time.Time
	tokenMu           sync.Mutex
	tokens            map[string]cachedServiceToken
	tokenFlights      map[string]*tokenFlight
	writeAttestor     WriteAttestor
}

type cachedServiceToken struct {
	value     string
	expiresAt time.Time
}

type tokenFlight struct {
	done      chan struct{}
	token     string
	expiresAt time.Time
	err       *SendError
}

// Option configures the Client.
type Option func(*Client)

// WriteAttestor signs a short-lived, effect-bound proof. *attestation.Signer
// satisfies it; keeping the interface narrow makes fail-closed tests pure.
type WriteAttestor interface {
	Sign(attestation.Authorization) (string, error)
}

// WithWriteAttestor configures the conversation-core provider-write signer.
func WithWriteAttestor(signer WriteAttestor) Option {
	return func(cl *Client) { cl.writeAttestor = signer }
}

// WithHTTPClient injects a custom *http.Client (used by tests / for tuning).
func WithHTTPClient(c *http.Client) Option {
	return func(cl *Client) {
		if c != nil {
			cl.httpClient = clientWithoutRedirects(c)
		}
	}
}

func clientWithoutRedirects(client *http.Client) *http.Client {
	clone := *client
	clone.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &clone
}

// WithAPIKeyHeader overrides the internal-api-key header name (default
// x-internal-api-key, matching integration-corev2's INTERNAL_API_KEY_HEADER).
func WithAPIKeyHeader(name string) Option {
	return func(cl *Client) {
		if strings.TrimSpace(name) != "" {
			cl.apiKeyHdr = strings.TrimSpace(name)
		}
	}
}

// WithServicePrincipal configures the durable conversation-core identity used
// only to mint short-lived, tenant-bound Auth Core tokens. The credential is
// never forwarded to integration-corev2.
func WithServicePrincipal(authCoreURL, serviceID, credential string) Option {
	return func(cl *Client) {
		cl.authCoreURL = strings.TrimRight(strings.TrimSpace(authCoreURL), "/")
		cl.serviceID = strings.TrimSpace(serviceID)
		cl.serviceCredential = strings.TrimSpace(credential)
	}
}

// WithClock supplies a deterministic clock for token-expiry tests.
func WithClock(now func() time.Time) Option {
	return func(cl *Client) {
		if now != nil {
			cl.now = now
		}
	}
}

// NewClient builds the integration client. The durable apiKey is restricted to
// internal webhook-event reads. Tenant routes additionally require
// WithServicePrincipal; Send never falls back to the durable key.
func NewClient(baseURL, apiKey string, opts ...Option) *Client {
	cl := &Client{
		baseURL:      strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:       strings.TrimSpace(apiKey),
		apiKeyHdr:    "x-internal-api-key",
		httpClient:   clientWithoutRedirects(&http.Client{Timeout: defaultTimeout}),
		now:          time.Now,
		tokens:       make(map[string]cachedServiceToken),
		tokenFlights: make(map[string]*tokenFlight),
	}
	for _, opt := range opts {
		if opt != nil {
			opt(cl)
		}
	}
	return cl
}

// actionRequestBody mirrors integration-corev2's actionBody: {operation, params, body}.
type actionRequestBody struct {
	Operation        string         `json:"operation"`
	Params           map[string]any `json:"params,omitempty"`
	Body             map[string]any `json:"body,omitempty"`
	IdempotencyKey   string         `json:"idempotencyKey,omitempty"`
	WriteAttestation string         `json:"writeAttestation"`
}

// actionResponse mirrors integration-corev2's success envelope:
// {"success": true, "data": {"action": {providerKey, operation, result}}}.
type actionResponse struct {
	Success bool `json:"success"`
	Data    struct {
		Action struct {
			ProviderKey string          `json:"providerKey"`
			Operation   string          `json:"operation"`
			Result      json.RawMessage `json:"result"`
		} `json:"action"`
	} `json:"data"`
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// Send executes the provider-appropriate send operation on the conversation's
// connection. It returns a *SendError (Terminal flag set) on failure so the
// executor can decide ack-with-send_failed vs retry.
func (c *Client) Send(ctx context.Context, req SendRequest) (*SendResult, error) {
	if c == nil || c.baseURL == "" || c.authCoreURL == "" || c.serviceID == "" || c.serviceCredential == "" {
		// Misconfiguration is terminal: provider writes cannot fall back to the
		// legacy shared key or caller-supplied tenant headers.
		return nil, &SendError{Terminal: true, Code: "not_configured", Message: "integration client is not configured"}
	}
	if strings.TrimSpace(req.OrgID) == "" {
		return nil, &SendError{Terminal: true, Code: "missing_organization", Message: "org_id is required to mint a tenant-bound integration token"}
	}
	if strings.TrimSpace(req.ConnectionID) == "" {
		return nil, &SendError{Terminal: true, Code: "missing_connection", Message: "connection_id is required to send"}
	}
	if c.writeAttestor == nil {
		return nil, &SendError{Terminal: true, Code: "attestation_not_configured", Message: "provider-write attestation signer is not configured"}
	}

	prepared, err := PrepareSend(req)
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "unsupported_provider", Message: err.Error(), err: err}
	}
	providedDigest := strings.TrimSpace(req.PayloadSHA256)
	if len(providedDigest) != len(prepared.PayloadSHA256) || subtle.ConstantTimeCompare([]byte(providedDigest), []byte(prepared.PayloadSHA256)) != 1 {
		return nil, &SendError{Terminal: true, Code: "payload_binding_mismatch", Message: "provider-write effect does not match the durable outbound intent"}
	}
	writeAttestation, err := c.writeAttestor.Sign(attestation.Authorization{
		AuthorizationKind: req.AuthorizationKind,
		AuthorizationID:   req.AuthorizationID,
		ApprovalID:        req.ApprovalID,
		ActionID:          req.ActionID,
		OrgID:             strings.TrimSpace(req.OrgID),
		ConnectionID:      strings.TrimSpace(req.ConnectionID),
		ProviderKey:       strings.TrimSpace(req.Provider),
		Operation:         prepared.Operation,
		ActorID:           strings.TrimSpace(req.ActorUserID),
		PayloadSHA256:     prepared.PayloadSHA256,
		IdempotencyKey:    strings.TrimSpace(req.IdempotencyKey),
	})
	if err != nil || strings.TrimSpace(writeAttestation) == "" {
		return nil, &SendError{Terminal: true, Code: "attestation_invalid", Message: "provider-write authorization could not be signed", err: err}
	}

	payload, err := json.Marshal(actionRequestBody{
		Operation:        prepared.Operation,
		Params:           prepared.Params,
		Body:             prepared.Body,
		IdempotencyKey:   strings.TrimSpace(req.IdempotencyKey),
		WriteAttestation: writeAttestation,
	})
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "marshal", Message: err.Error(), err: err}
	}

	url := fmt.Sprintf("%s/api/v1/connections/%s/actions", c.baseURL, req.ConnectionID)
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "build_request", Message: err.Error(), err: err}
	}
	httpReq.Header.Set("Content-Type", "application/json")
	token, tokenErr := c.serviceToken(ctx, req.OrgID)
	if tokenErr != nil {
		return nil, tokenErr
	}
	httpReq.Header.Set("Authorization", "Bearer "+token)

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		// Transport error or context timeout/cancel — transient, retry.
		return nil, &SendError{Terminal: false, Code: "transport", Message: err.Error(), err: err}
	}
	defer resp.Body.Close()
	respBytes, overflow, readErr := readBounded(resp.Body, actionResponseMaxBytes)
	if readErr != nil {
		return nil, &SendError{Terminal: false, Status: resp.StatusCode, Code: "response_read", Message: "integration outcome requires reconciliation", err: readErr}
	}
	if overflow {
		return nil, invalidSendResponse(resp.StatusCode)
	}

	switch {
	case resp.StatusCode >= 200 && resp.StatusCode < 300:
		trimmed := bytes.TrimSpace(respBytes)
		if len(trimmed) == 0 {
			// Microsoft Graph and Gmail may acknowledge mail submission with an
			// empty 202. This is the only empty-success shape permitted by the
			// integration contract; arbitrary empty/malformed 2xx must not create
			// a false sent row.
			if resp.StatusCode == http.StatusAccepted && (prepared.Operation == "mail.send" || prepared.Operation == "gmail.send") {
				return &SendResult{Operation: prepared.Operation}, nil
			}
			return nil, invalidSendResponse(resp.StatusCode)
		}
		var decoded actionResponse
		if err := json.Unmarshal(trimmed, &decoded); err != nil || !decoded.Success {
			return nil, invalidSendResponse(resp.StatusCode)
		}
		providerMessageID := extractProviderMessageID(decoded.Data.Action.Result)
		if providerMessageID == "" && prepared.Operation != "mail.send" && prepared.Operation != "gmail.send" {
			return nil, invalidSendResponse(resp.StatusCode)
		}
		return &SendResult{ProviderMessageID: providerMessageID, Operation: prepared.Operation}, nil
	case resp.StatusCode >= 400 && resp.StatusCode < 500:
		// Client error: bad request, unauthorized, connection not found, capability
		// denied — none fixable by a blind retry. Terminal.
		return nil, classifyError(true, resp.StatusCode, respBytes)
	default:
		// 5xx (or any non-2xx/non-4xx) — provider/integration unavailable. Retry.
		return nil, classifyError(false, resp.StatusCode, respBytes)
	}
}

// WebhookEvent is the stored payload integration-corev2 returns for
// GET /internal/webhooks/events/{id} — the full body a webhook_received NATS
// event (metadata-only, by design) points at via its webhookEventId.
type WebhookEvent struct {
	ID          string         `json:"id"`
	ProviderKey string         `json:"providerKey"`
	EventType   string         `json:"eventType"`
	Payload     map[string]any `json:"payload"`
}

// FetchWebhookEvent retrieves the full payload for a webhook_received event
// by id, org-scoped. Returns an error (not a *SendError — this isn't an
// outbound send) on any non-2xx response, including "not found" for a
// missing/mismatched-org event.
func (c *Client) FetchWebhookEvent(ctx context.Context, orgID, webhookEventID string) (*WebhookEvent, error) {
	if c == nil || c.baseURL == "" || c.apiKey == "" {
		return nil, fmt.Errorf("integration client is not configured")
	}
	if strings.TrimSpace(webhookEventID) == "" {
		return nil, fmt.Errorf("webhookEventID is required")
	}
	url := fmt.Sprintf("%s/internal/webhooks/events/%s?organizationId=%s",
		c.baseURL, webhookEventID, neturl.QueryEscape(orgID))
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("build webhook event request: %w", err)
	}
	req.Header.Set(c.apiKeyHdr, c.apiKey)
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call webhook event lookup: %w", err)
	}
	defer resp.Body.Close()
	respBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("webhook event lookup returned status %d: %s", resp.StatusCode, string(respBytes))
	}
	var decoded struct {
		Data struct {
			WebhookEvent WebhookEvent `json:"webhookEvent"`
		} `json:"data"`
	}
	if err := json.Unmarshal(respBytes, &decoded); err != nil {
		return nil, fmt.Errorf("decode webhook event response: %w", err)
	}
	return &decoded.Data.WebhookEvent, nil
}

// FetchActiveConnectionID resolves the connection to reply through for an
// org, trying each candidate provider key in order (e.g. "whatsapp" then the
// unified "meta" fallback) and returning the first active connection's id.
// Inbound Meta webhooks are account-wide, not connection-scoped, so replies
// must resolve a connection out-of-band; returns "" (no error) when no
// candidate has an active connection, letting the caller store the inbound
// event without a reply target rather than fail ingestion over it.
func (c *Client) FetchActiveConnectionID(ctx context.Context, orgID string, providerKeys ...string) (string, error) {
	if c == nil || c.baseURL == "" || c.authCoreURL == "" || c.serviceID == "" || c.serviceCredential == "" {
		return "", fmt.Errorf("integration client is not configured")
	}
	token, tokenErr := c.serviceToken(ctx, orgID)
	if tokenErr != nil {
		return "", tokenErr
	}
	for _, providerKey := range providerKeys {
		providerKey = strings.TrimSpace(providerKey)
		if providerKey == "" {
			continue
		}
		url := fmt.Sprintf("%s/api/v1/connections?organizationId=%s&providerKey=%s",
			c.baseURL, neturl.QueryEscape(orgID), neturl.QueryEscape(providerKey))
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return "", fmt.Errorf("build connections list request: %w", err)
		}
		req.Header.Set("Authorization", "Bearer "+token)
		resp, err := c.httpClient.Do(req)
		if err != nil {
			return "", fmt.Errorf("call connections list: %w", err)
		}
		respBytes, overflow, readErr := readBounded(resp.Body, actionResponseMaxBytes)
		resp.Body.Close()
		if readErr != nil {
			return "", fmt.Errorf("read connections list response: %w", readErr)
		}
		if overflow {
			return "", fmt.Errorf("connections list response exceeded the configured limit")
		}
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return "", fmt.Errorf("connections list returned status %d", resp.StatusCode)
		}
		var decoded struct {
			Data struct {
				Connections []struct {
					ID     string `json:"id"`
					Status string `json:"status"`
				} `json:"connections"`
			} `json:"data"`
		}
		if err := json.Unmarshal(respBytes, &decoded); err != nil {
			return "", fmt.Errorf("decode connections list response: %w", err)
		}
		for _, conn := range decoded.Data.Connections {
			if strings.EqualFold(conn.Status, "active") {
				return conn.ID, nil
			}
		}
	}
	return "", nil
}

func classifyError(terminal bool, status int, respBytes []byte) *SendError {
	var decoded actionResponse
	_ = json.Unmarshal(respBytes, &decoded)
	code := sanitizeErrorCode(decoded.Error.Code)
	// integration-corev2 has already durably claimed this provider write and
	// cannot prove whether the provider accepted it. A 409 with this exact code
	// is ambiguous, not a definite client rejection, and must never be retried
	// automatically with a fresh request.
	if code == "action_outcome_unknown" {
		terminal = false
	}
	safeToRetry := code == "action_pre_provider_retryable"
	if safeToRetry {
		terminal = false
	}
	message := "integration request failed"
	if safeToRetry {
		message = "integration rejected request before provider dispatch"
	} else if !terminal {
		message = "integration outcome requires reconciliation"
	}
	return &SendError{
		Terminal:    terminal,
		SafeToRetry: safeToRetry,
		Status:      status,
		Code:        code,
		Message:     message,
	}
}

func invalidSendResponse(status int) *SendError {
	return &SendError{
		Terminal: false,
		Status:   status,
		Code:     "invalid_response",
		Message:  "integration response did not prove provider submission",
	}
}

type serviceTokenRequest struct {
	OrgID  string   `json:"orgId"`
	Scopes []string `json:"scopes"`
	Reason string   `json:"reason"`
}

type serviceTokenResponse struct {
	Token     string `json:"token"`
	ExpiresAt string `json:"expiresAt"`
}

func (c *Client) serviceToken(ctx context.Context, orgID string) (string, *SendError) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", &SendError{Terminal: true, Code: "missing_organization", Message: "organization is required for integration authorization"}
	}
	now := c.now().UTC()
	c.tokenMu.Lock()
	if cached, ok := c.tokens[orgID]; ok && now.Add(serviceTokenRefreshSkew).Before(cached.expiresAt) {
		c.tokenMu.Unlock()
		return cached.value, nil
	}
	if flight := c.tokenFlights[orgID]; flight != nil {
		c.tokenMu.Unlock()
		select {
		case <-flight.done:
			return flight.token, flight.err
		case <-ctx.Done():
			return "", &SendError{SafeToRetry: true, Code: "auth_token_unavailable", Message: "service-token wait was canceled before provider dispatch", err: ctx.Err()}
		}
	}
	flight := &tokenFlight{done: make(chan struct{})}
	c.tokenFlights[orgID] = flight
	c.tokenMu.Unlock()

	flight.token, flight.expiresAt, flight.err = c.fetchServiceToken(ctx, orgID, now)
	c.tokenMu.Lock()
	if flight.err == nil {
		c.tokens[orgID] = cachedServiceToken{value: flight.token, expiresAt: flight.expiresAt}
	}
	delete(c.tokenFlights, orgID)
	close(flight.done)
	c.tokenMu.Unlock()
	return flight.token, flight.err
}

func (c *Client) fetchServiceToken(ctx context.Context, orgID string, now time.Time) (string, time.Time, *SendError) {
	payload, err := json.Marshal(serviceTokenRequest{
		OrgID:  orgID,
		Scopes: []string{"integration:read", "integration:write"},
		Reason: "execute approved conversation provider action",
	})
	if err != nil {
		return "", time.Time{}, &SendError{Terminal: true, Code: "auth_token_invalid", Message: "service-token request is invalid", err: err}
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.authCoreURL+"/api/ingestion/internal-token", bytes.NewReader(payload))
	if err != nil {
		return "", time.Time{}, &SendError{Terminal: true, Code: "auth_token_invalid", Message: "service-token request is invalid", err: err}
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-service-id", c.serviceID)
	request.Header.Set("x-service-api-key", c.serviceCredential)
	response, err := c.httpClient.Do(request)
	if err != nil {
		return "", time.Time{}, &SendError{SafeToRetry: true, Code: "auth_token_unavailable", Message: "service-token authority is unavailable before provider dispatch", err: err}
	}
	defer response.Body.Close()
	body, overflow, err := readBounded(response.Body, authResponseMaxBytes)
	if err != nil || overflow {
		return "", time.Time{}, &SendError{SafeToRetry: true, Status: response.StatusCode, Code: "auth_token_invalid", Message: "service-token authority returned an invalid pre-provider response", err: err}
	}
	if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
		return "", time.Time{}, &SendError{Terminal: true, Status: response.StatusCode, Code: "auth_token_rejected", Message: "service principal was rejected"}
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		retryable := response.StatusCode >= http.StatusInternalServerError ||
			response.StatusCode == http.StatusRequestTimeout || response.StatusCode == http.StatusTooEarly ||
			response.StatusCode == http.StatusTooManyRequests ||
			(response.StatusCode >= http.StatusMultipleChoices && response.StatusCode < http.StatusBadRequest)
		return "", time.Time{}, &SendError{Terminal: !retryable, SafeToRetry: retryable, Status: response.StatusCode, Code: "auth_token_unavailable", Message: "service-token authority is unavailable before provider dispatch"}
	}
	var decoded serviceTokenResponse
	if err := json.Unmarshal(body, &decoded); err != nil {
		return "", time.Time{}, &SendError{SafeToRetry: true, Status: response.StatusCode, Code: "auth_token_invalid", Message: "service-token authority returned malformed pre-provider JSON", err: err}
	}
	decoded.Token = strings.TrimSpace(decoded.Token)
	expiresAt, err := time.Parse(time.RFC3339, strings.TrimSpace(decoded.ExpiresAt))
	if err != nil || decoded.Token == "" || len(decoded.Token) > serviceTokenMaxBytes || !now.Add(serviceTokenRefreshSkew).Before(expiresAt) {
		return "", time.Time{}, &SendError{SafeToRetry: true, Status: response.StatusCode, Code: "auth_token_invalid", Message: "service-token authority returned an unusable pre-provider token", err: err}
	}
	return decoded.Token, expiresAt.UTC(), nil
}

func readBounded(reader io.Reader, maximum int64) ([]byte, bool, error) {
	limited := &io.LimitedReader{R: reader, N: maximum + 1}
	body, err := io.ReadAll(limited)
	if err != nil {
		return nil, false, err
	}
	if int64(len(body)) > maximum {
		return body[:maximum], true, nil
	}
	return body, false, nil
}

func sanitizeErrorCode(value string) string {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 64 {
		return "integration_error"
	}
	for _, char := range value {
		if (char < 'a' || char > 'z') && (char < 'A' || char > 'Z') &&
			(char < '0' || char > '9') && char != '_' && char != '-' && char != '.' {
			return "integration_error"
		}
	}
	return value
}

// PreparedSend is the exact integration-corev2 action effect produced from a
// channel reply. PayloadSHA256 binds the tenant, connection, provider,
// operation, params, and body using a deterministic encoding/json struct.
type PreparedSend struct {
	Operation     string
	Params        map[string]any
	Body          map[string]any
	PayloadSHA256 string
}

// PrepareSend maps and hashes a provider action before a durable intent is
// claimed. encoding/json sorts map keys, while the struct fixes field order and
// names; both signer and verifier therefore hash the same canonical bytes.
func PrepareSend(req SendRequest) (PreparedSend, error) {
	operation, params, body, err := buildSendOperation(req)
	if err != nil {
		return PreparedSend{}, err
	}
	canonical := struct {
		OrgID        string         `json:"org_id"`
		ConnectionID string         `json:"connection_id"`
		ProviderKey  string         `json:"provider_key"`
		Operation    string         `json:"operation"`
		Params       map[string]any `json:"params"`
		Body         map[string]any `json:"body"`
	}{
		OrgID: strings.TrimSpace(req.OrgID), ConnectionID: strings.TrimSpace(req.ConnectionID),
		ProviderKey: strings.TrimSpace(req.Provider), Operation: strings.TrimSpace(operation),
		Params: params, Body: body,
	}
	encoded, err := json.Marshal(canonical)
	if err != nil {
		return PreparedSend{}, fmt.Errorf("canonicalize provider-write effect: %w", err)
	}
	digest := sha256.Sum256(encoded)
	return PreparedSend{
		Operation: strings.TrimSpace(operation), Params: params, Body: body,
		PayloadSHA256: fmt.Sprintf("%x", digest[:]),
	}, nil
}

// SupportsSend reports whether buildSendOperation has a real send mapping for
// the given channel provider (whatsapp, messenger, microsoft, slack, google —
// anything but discord/unknown). The human-reply path in conversation-core's
// Service uses it to decide up front whether a conversation is deliverable
// through this client, so a channel with no send op (e.g. a plain email inbox)
// stays store-only instead of surfacing a terminal "unsupported provider"
// error. It delegates to buildSendOperation so the supported-provider set can
// never drift from the real send mapping.
func SupportsSend(provider string) bool {
	_, _, _, err := buildSendOperation(SendRequest{Provider: provider})
	return !errors.Is(err, ErrUnsupportedProvider)
}

// buildSendOperation maps the channel provider to its integration-corev2 send
// operation + request shape. Unknown providers are a terminal error.
func buildSendOperation(req SendRequest) (operation string, params, body map[string]any, err error) {
	text := req.BodyText
	if strings.TrimSpace(text) == "" {
		text = req.BodyHTML
	}
	switch strings.TrimSpace(strings.ToLower(req.Provider)) {
	case "microsoft":
		// Outlook / Graph mail send: reply into the existing thread when present.
		operation = "mail.send"
		body = map[string]any{
			"subject":  req.Subject,
			"bodyText": req.BodyText,
			"bodyHtml": req.BodyHTML,
			"to":       req.To,
		}
		params = map[string]any{}
		if req.ProviderThreadID != "" {
			params["threadId"] = req.ProviderThreadID
			body["conversationId"] = req.ProviderThreadID
		}
		return operation, params, body, nil
	case "slack":
		// Slack replies address a channel, optionally inside a thread. The
		// inbound consumer writes "channel" for top-level messages and
		// "channel:thread_ts" for threaded ones — sending the whole composite
		// as thread_ts (the previous behavior) produced an invalid ts and
		// broke threaded replies.
		operation = "message.send"
		body = map[string]any{
			"text": text,
		}
		params = map[string]any{}
		channel, threadTS := splitChannelThreadID(req.ProviderThreadID)
		if channel == "" {
			// Un-threaded refs carry just the channel id (no separator).
			channel, threadTS = threadTS, ""
		}
		if channel == "" {
			return "", nil, nil, fmt.Errorf("slack send requires a channel id on the thread ref")
		}
		params["channel"] = channel
		body["channel"] = channel
		if threadTS != "" {
			body["thread_ts"] = threadTS
		}
		return operation, params, body, nil
	case "google":
		operation = "gmail.send"
		body = map[string]any{
			"subject":  req.Subject,
			"bodyText": req.BodyText,
			"bodyHtml": req.BodyHTML,
			"to":       req.To,
		}
		params = map[string]any{}
		if req.ProviderThreadID != "" {
			params["threadId"] = req.ProviderThreadID
			body["threadId"] = req.ProviderThreadID
		}
		return operation, params, body, nil
	case "whatsapp":
		// WhatsApp Cloud API send via integration-corev2 (which auto-injects
		// messaging_product and validates to/type). The gateway needs the
		// business WABA phone-number id in params and the customer address in
		// the body. ProviderThreadID is the composite "phoneNumberId:waId"
		// written by the inbound webhook consumer; To[0] overrides the
		// recipient when the caller supplies one explicitly.
		businessID, recipient := splitChannelThreadID(req.ProviderThreadID)
		if len(req.To) > 0 && strings.TrimSpace(req.To[0]) != "" {
			recipient = strings.TrimSpace(req.To[0])
		}
		if businessID == "" {
			return "", nil, nil, fmt.Errorf("whatsapp send requires a WABA phone-number id (composite provider_thread_id)")
		}
		if recipient == "" {
			return "", nil, nil, fmt.Errorf("whatsapp send requires a recipient wa_id")
		}
		operation = "whatsapp.messages.send"
		params = map[string]any{"phoneNumberId": businessID}
		body = map[string]any{
			"to":   recipient,
			"type": "text",
			"text": map[string]any{"body": text},
		}
		return operation, params, body, nil
	case "messenger":
		// Messenger Send API via integration-corev2 (which exchanges the page
		// token server-side). params carries the page id; body is the Send
		// API envelope. ProviderThreadID is the composite "pageId:psid" from
		// the inbound webhook consumer; To[0] overrides the recipient PSID.
		pageID, psid := splitChannelThreadID(req.ProviderThreadID)
		if len(req.To) > 0 && strings.TrimSpace(req.To[0]) != "" {
			psid = strings.TrimSpace(req.To[0])
		}
		if pageID == "" {
			return "", nil, nil, fmt.Errorf("messenger send requires a page id (composite provider_thread_id)")
		}
		if psid == "" {
			return "", nil, nil, fmt.Errorf("messenger send requires a recipient PSID")
		}
		operation = "messenger.messages.send"
		params = map[string]any{"pageId": pageID}
		body = map[string]any{
			"recipient":      map[string]any{"id": psid},
			"message":        map[string]any{"text": text},
			"messaging_type": "RESPONSE",
		}
		return operation, params, body, nil
	case "instagram":
		// Instagram DM send rides the Messenger Platform through the LINKED
		// Facebook Page — integration-corev2 resolves the page + page token
		// from the IG business-account id server-side. ProviderThreadID is the
		// composite "igAccountId:igsid" written by the inbound webhook
		// consumer; To[0] overrides the recipient IGSID.
		igAccountID, igsid := splitChannelThreadID(req.ProviderThreadID)
		if len(req.To) > 0 && strings.TrimSpace(req.To[0]) != "" {
			igsid = strings.TrimSpace(req.To[0])
		}
		if igAccountID == "" {
			return "", nil, nil, fmt.Errorf("instagram send requires an IG business-account id (composite provider_thread_id)")
		}
		if igsid == "" {
			return "", nil, nil, fmt.Errorf("instagram send requires a recipient IGSID")
		}
		operation = "instagram.messages.send"
		params = map[string]any{"igAccountId": igAccountID}
		body = map[string]any{
			"recipient":      map[string]any{"id": igsid},
			"message":        map[string]any{"text": text},
			"messaging_type": "RESPONSE",
		}
		return operation, params, body, nil
	case "discord":
		// Discord message delivery requires a bot token + gateway/REST bot
		// integration, not the per-user OAuth token this path leases. Fail
		// honestly rather than attempt an unsupported send.
		return "", nil, nil, fmt.Errorf("%w: discord outbound requires a bot integration, not user OAuth", ErrUnsupportedProvider)
	default:
		return "", nil, nil, fmt.Errorf("%w: %q", ErrUnsupportedProvider, req.Provider)
	}
}

// splitChannelThreadID splits a composite provider thread id of the form
// "businessId:recipientId" (used by the WhatsApp/Messenger inbound consumer to
// carry both the business-side sender id and the customer address on a single
// thread ref). A value with no separator is treated as the recipient with an
// empty business id, letting callers surface a clear addressing error.
func splitChannelThreadID(threadID string) (businessID, recipientID string) {
	threadID = strings.TrimSpace(threadID)
	if threadID == "" {
		return "", ""
	}
	if biz, rec, found := strings.Cut(threadID, ":"); found {
		return strings.TrimSpace(biz), strings.TrimSpace(rec)
	}
	return "", threadID
}

// extractProviderMessageID best-effort pulls a provider message id out of the
// adapter result. Different providers return different shapes (Slack: ts /
// message.ts; Gmail/Graph: id / messageId), so we probe the common keys. An
// empty result (e.g. Graph sendMail's 202-no-body) yields "" — still a success.
func extractProviderMessageID(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var generic map[string]any
	if err := json.Unmarshal(raw, &generic); err != nil {
		return ""
	}
	for _, key := range []string{"provider_message_id", "providerMessageId", "messageId", "message_id", "id", "ts"} {
		if v, ok := generic[key].(string); ok && strings.TrimSpace(v) != "" {
			return strings.TrimSpace(v)
		}
	}
	// Nested {message: {ts|id}} (Slack chat.postMessage).
	if msg, ok := generic["message"].(map[string]any); ok {
		for _, key := range []string{"ts", "id"} {
			if v, ok := msg[key].(string); ok && strings.TrimSpace(v) != "" {
				return strings.TrimSpace(v)
			}
		}
	}
	// WhatsApp Cloud API: {"messages":[{"id":"wamid..."}]}.
	if msgs, ok := generic["messages"].([]any); ok && len(msgs) > 0 {
		if first, ok := msgs[0].(map[string]any); ok {
			if v, ok := first["id"].(string); ok && strings.TrimSpace(v) != "" {
				return strings.TrimSpace(v)
			}
		}
	}
	return ""
}
