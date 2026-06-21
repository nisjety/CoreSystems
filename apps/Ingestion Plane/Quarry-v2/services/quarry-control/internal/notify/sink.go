// Package notify delivers in-product notifications to the Application Plane's
// notification-core over HTTP. Quarry never writes notification-core's tables
// directly — the plane boundary is the HTTP intake.
package notify

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

// Sink delivers a single in-product notification. Implementations must be
// safe for concurrent use and idempotent on eventID (so a replayed event
// batch doesn't create duplicate notifications).
type Sink interface {
	NotifyChange(ctx context.Context, recipientID, eventID string, payload map[string]any) error
}

// HTTPSink posts to notification-core's internal intake
// (POST /api/v1/notification-requests, gated on x-internal-api-key).
type HTTPSink struct {
	baseURL     string
	internalKey string
	client      *http.Client
}

// NewHTTPSink builds a sink targeting notification-core at baseURL.
func NewHTTPSink(baseURL, internalKey string) *HTTPSink {
	return &HTTPSink{
		baseURL:     strings.TrimRight(baseURL, "/"),
		internalKey: internalKey,
		client:      &http.Client{Timeout: 5 * time.Second},
	}
}

// NotifyChange creates a `change_detected` feed entry for recipientID.
// idempotency_key = the source event id, so notification-core dedupes a
// replayed event batch into a single notification.
func (s *HTTPSink) NotifyChange(ctx context.Context, recipientID, eventID string, payload map[string]any) error {
	body, err := json.Marshal(map[string]any{
		"recipient_id":    recipientID,
		"type":            "change_detected",
		"payload":         payload,
		"idempotency_key": eventID,
		"source":          "quarry",
	})
	if err != nil {
		return err
	}
	url := s.baseURL + "/api/v1/notification-requests"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("content-type", "application/json")
	if s.internalKey != "" {
		req.Header.Set("x-internal-api-key", s.internalKey)
	}
	resp, err := s.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("notification-core returned %d: %s", resp.StatusCode, string(b))
	}
	return nil
}
