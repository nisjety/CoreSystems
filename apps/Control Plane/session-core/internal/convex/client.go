package convex
// Package convex provides a lightweight HTTP client for writing session state
// into convex-core (Application Plane) so the frontend can subscribe
// reactively without polling Postgres over SSE.
//
// The client is best-effort: failures are logged but never returned as errors
// to callers, so a Convex outage never breaks session operations.

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

// Client sends session events to the Convex HTTP action endpoints exposed by
// convex-core on port 3211.
type Client struct {
	baseURL    string
	serviceKey string
	http       *http.Client
}

// NewClient returns a Client configured to talk to the Convex backend.
// Returns nil when url or serviceKey is empty so callers can gate on nil.
func NewClient(url, serviceKey string) *Client {
	if url == "" || serviceKey == "" {
		return nil
	}
	return &Client{
		baseURL:    url,
		serviceKey: serviceKey,
		http: &http.Client{
			Timeout: 5 * time.Second,
		},
	}
}

// CreateConversation calls POST /ingest/session on convex-core, which creates
// a Convex conversation document linked to the given session-core session ID.
// The returned conversationID is the Convex document ID (opaque string).
func (c *Client) CreateConversation(
	ctx context.Context,
	sessionID, externalOrgID, externalUserID, title string,
	planMode bool,
) (string, error) {
	body, _ := json.Marshal(map[string]any{
		"session_id":       sessionID,
		"external_org_id":  externalOrgID,
		"external_user_id": externalUserID,
		"title":            title,
		"plan_mode":        planMode,
	})

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/ingest/session", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Key", c.serviceKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("convex request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("convex responded %d", resp.StatusCode)
	}

	var result struct {
		ConversationID string `json:"conversation_id"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", fmt.Errorf("decode response: %w", err)
	}

	return result.ConversationID, nil
}

// PostMessage calls POST /ingest/session/message on convex-core, appending
// a message to the Convex conversation linked to sessionID.
func (c *Client) PostMessage(
	ctx context.Context,
	sessionID, role, content string,
) (string, error) {
	body, _ := json.Marshal(map[string]any{
		"session_id": sessionID,
		"role":       role,
		"content":    content,
	})

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/ingest/session/message", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Key", c.serviceKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("convex message request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("convex responded %d", resp.StatusCode)
	}

	var result struct {
		MessageID string `json:"message_id"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", fmt.Errorf("decode response: %w", err)
	}

	return result.MessageID, nil
}

// MirrorControlSession posts the aggregated Control Session snapshot to
// convex-core's `/ingest/control-session` HTTP action so verevon clients
// can subscribe reactively via `api.controlSessions.byUser` (G35).
//
// `snapshot` is the full opaque payload (ControlSession struct); we don't
// project it into typed fields here — the Convex side stores it as
// `v.any()` and decoders branch on shape, which keeps the contract loose
// enough that we don't have to ship a Convex schema migration every time
// session-core adds a field.
//
// Best-effort: errors logged by the caller, never returned as fatal.
func (c *Client) MirrorControlSession(
	ctx context.Context,
	externalUserID, externalOrgID string,
	snapshot any,
	fetchedAtMillis int64,
) error {
	if c == nil {
		return nil
	}
	if externalUserID == "" {
		return fmt.Errorf("MirrorControlSession: external_user_id required")
	}
	body, _ := json.Marshal(map[string]any{
		"external_user_id": externalUserID,
		"external_org_id":  externalOrgID,
		"snapshot":         snapshot,
		"fetched_at":       fetchedAtMillis,
	})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/ingest/control-session", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Key", c.serviceKey)
	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("convex control-session request: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("convex responded %d", resp.StatusCode)
	}
	return nil
}
