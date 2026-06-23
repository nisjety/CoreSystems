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
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// defaultTimeout bounds every send so a hung provider call surfaces as a
// transient failure (retryable) rather than blocking the consumer forever.
const defaultTimeout = 20 * time.Second

// ErrUnsupportedProvider is returned (as a terminal error) when a conversation's
// channel provider has no known send operation. It is permanent — retrying will
// not help — so the executor must surface send_failed rather than loop.
var ErrUnsupportedProvider = errors.New("integration: unsupported send provider")

// SendError carries whether a failure is terminal (do not retry; surface
// send_failed) or transient (retry via JetStream redelivery). 4xx + unsupported
// provider are terminal; timeouts + 5xx + transport errors are transient.
type SendError struct {
	Terminal bool
	Status   int
	Code     string
	Message  string
	err      error
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
	baseURL    string
	apiKey     string
	apiKeyHdr  string
	httpClient *http.Client
}

// Option configures the Client.
type Option func(*Client)

// WithHTTPClient injects a custom *http.Client (used by tests / for tuning).
func WithHTTPClient(c *http.Client) Option {
	return func(cl *Client) {
		if c != nil {
			cl.httpClient = c
		}
	}
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

// NewClient builds the outbound client. baseURL + apiKey are required for the
// client to be usable; callers gate construction on their presence (see config),
// so there is no false "send claim" without configuration.
func NewClient(baseURL, apiKey string, opts ...Option) *Client {
	cl := &Client{
		baseURL:    strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:     strings.TrimSpace(apiKey),
		apiKeyHdr:  "x-internal-api-key",
		httpClient: &http.Client{Timeout: defaultTimeout},
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
	Operation string         `json:"operation"`
	Params    map[string]any `json:"params,omitempty"`
	Body      map[string]any `json:"body,omitempty"`
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
	if c == nil || c.baseURL == "" || c.apiKey == "" {
		// Misconfiguration is terminal: without a base URL or key we can never
		// send, so retrying forever is dishonest. Surface send_failed instead.
		return nil, &SendError{Terminal: true, Code: "not_configured", Message: "integration client is not configured"}
	}
	if strings.TrimSpace(req.ConnectionID) == "" {
		return nil, &SendError{Terminal: true, Code: "missing_connection", Message: "connection_id is required to send"}
	}

	operation, params, body, err := buildSendOperation(req)
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "unsupported_provider", Message: err.Error(), err: err}
	}

	payload, err := json.Marshal(actionRequestBody{Operation: operation, Params: params, Body: body})
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "marshal", Message: err.Error(), err: err}
	}

	url := fmt.Sprintf("%s/api/v1/connections/%s/actions", c.baseURL, req.ConnectionID)
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return nil, &SendError{Terminal: true, Code: "build_request", Message: err.Error(), err: err}
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set(c.apiKeyHdr, c.apiKey)
	// Org/user headers so integration-corev2 can scope + audit the action to the
	// approving tenant and actor (AssertOrgAccess uses the request org context).
	if req.OrgID != "" {
		httpReq.Header.Set("x-org-id", req.OrgID)
	}
	if req.ActorUserID != "" {
		httpReq.Header.Set("x-user-id", req.ActorUserID)
	}

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		// Transport error or context timeout/cancel — transient, retry.
		return nil, &SendError{Terminal: false, Code: "transport", Message: err.Error(), err: err}
	}
	defer resp.Body.Close()
	respBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))

	switch {
	case resp.StatusCode >= 200 && resp.StatusCode < 300:
		var decoded actionResponse
		_ = json.Unmarshal(respBytes, &decoded)
		return &SendResult{
			ProviderMessageID: extractProviderMessageID(decoded.Data.Action.Result),
			Operation:         operation,
		}, nil
	case resp.StatusCode >= 400 && resp.StatusCode < 500:
		// Client error: bad request, unauthorized, connection not found, capability
		// denied — none fixable by a blind retry. Terminal.
		return nil, classifyError(true, resp.StatusCode, respBytes)
	default:
		// 5xx (or any non-2xx/non-4xx) — provider/integration unavailable. Retry.
		return nil, classifyError(false, resp.StatusCode, respBytes)
	}
}

func classifyError(terminal bool, status int, respBytes []byte) *SendError {
	var decoded actionResponse
	_ = json.Unmarshal(respBytes, &decoded)
	return &SendError{
		Terminal: terminal,
		Status:   status,
		Code:     decoded.Error.Code,
		Message:  decoded.Error.Message,
	}
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
		operation = "message.send"
		body = map[string]any{
			"text": text,
		}
		params = map[string]any{}
		if req.ProviderThreadID != "" {
			// Slack threads reply by parent message ts.
			params["channel"] = req.ProviderThreadID
			body["channel"] = req.ProviderThreadID
			body["thread_ts"] = req.ProviderThreadID
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
	default:
		return "", nil, nil, fmt.Errorf("%w: %q", ErrUnsupportedProvider, req.Provider)
	}
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
	return ""
}
