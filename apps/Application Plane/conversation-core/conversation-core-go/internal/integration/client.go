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
	neturl "net/url"
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
	if c == nil || c.baseURL == "" || c.apiKey == "" {
		return "", fmt.Errorf("integration client is not configured")
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
		req.Header.Set(c.apiKeyHdr, c.apiKey)
		resp, err := c.httpClient.Do(req)
		if err != nil {
			return "", fmt.Errorf("call connections list: %w", err)
		}
		respBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
		resp.Body.Close()
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return "", fmt.Errorf("connections list returned status %d: %s", resp.StatusCode, string(respBytes))
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
	return &SendError{
		Terminal: terminal,
		Status:   status,
		Code:     decoded.Error.Code,
		Message:  decoded.Error.Message,
	}
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
