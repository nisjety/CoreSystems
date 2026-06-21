package briefs

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// NotificationRequest is the wire body notification-core's
// POST /api/v1/notification-requests expects (notification.Request). The `Type`
// becomes the Novu WorkflowID; `RecipientID` the Novu subscriber; `IdempotencyKey`
// the Novu idempotency key (Novu deduplicates retries on it).
type NotificationRequest struct {
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	RecipientID    string         `json:"recipient_id"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	Source         string         `json:"source,omitempty"`
}

// NotificationClient delivers a notification request to notification-core. The
// scheduler depends on this narrow interface so delivery is testable without a
// live notification-core.
type NotificationClient interface {
	Send(ctx context.Context, req NotificationRequest) error
}

// HTTPNotificationClient POSTs to notification-core's internal dispatch endpoint
// using the shared internal API key (the same trust boundary every Application
// Plane internal caller uses). It never carries client-supplied identity — the
// scheduler resolves the org/recipient server-side.
type HTTPNotificationClient struct {
	baseURL     string
	internalKey string
	http        *http.Client
}

func NewHTTPNotificationClient(baseURL, internalKey string) *HTTPNotificationClient {
	return &HTTPNotificationClient{
		baseURL:     strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		internalKey: strings.TrimSpace(internalKey),
		http:        &http.Client{Timeout: 10 * time.Second},
	}
}

func (c *HTTPNotificationClient) Send(ctx context.Context, req NotificationRequest) error {
	body, err := json.Marshal(req)
	if err != nil {
		return fmt.Errorf("marshal notification request: %w", err)
	}
	url := c.baseURL + "/api/v1/notification-requests"
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build notification request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("x-internal-api-key", c.internalKey)

	resp, err := c.http.Do(httpReq)
	if err != nil {
		return fmt.Errorf("post notification request: %w", err)
	}
	defer resp.Body.Close()

	// notification-core returns 202 Accepted on success; 502 with a body when
	// the upstream Novu dispatch failed (the request is still recorded). Treat
	// 2xx as delivered/accepted; anything else is an error to retry next tick.
	if resp.StatusCode >= 200 && resp.StatusCode < 300 {
		_, _ = io.Copy(io.Discard, resp.Body)
		return nil
	}
	snippet, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
	return fmt.Errorf("notification-core returned %d: %s", resp.StatusCode, strings.TrimSpace(string(snippet)))
}
