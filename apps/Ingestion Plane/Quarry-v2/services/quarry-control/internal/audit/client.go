// Package audit is quarry-control's client for the Control Plane's audit-core
// (WS14.S7b). It writes one event per call to POST /v1/audit as the
// quarry-control producer, pinned to the ingestion plane.
//
// It follows integration-corev2's AuditClient.RecordAudit
// (apps/Ingestion Plane/integration-corev2/internal/controlplane/clients.go):
// the same X-Service-Id / X-Service-Token headers, the same body shape, the
// same refusal to follow redirects. audit-core accepts the write only when the
// event's producer equals the authenticated principal and its plane is one the
// principal's credential row allows; the credential row for quarry-control
// (audit:write, planes ["ingestion"]) is in Control's compose registry
// (W2-CP-R13-2).
//
// The client never reads a token from anywhere but its constructor, and never
// prints it.
package audit

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const (
	// Producer is the producer authority quarry-control writes as. It must equal
	// the audit-core credential row's principal, or audit-core answers 403.
	Producer = "quarry-control"
	// Plane is the only plane quarry-control's credential row allows.
	Plane = "ingestion"

	// EnvURL and EnvToken name the two settings FromEnv reads.
	EnvURL   = "AUDIT_CORE_URL"
	EnvToken = "AUDIT_CORE_SERVICE_TOKEN"

	defaultTimeout = 5 * time.Second
	maxEventIDLen  = 128
)

// ErrNotConfigured is returned by RecordAudit on a nil client or one without a
// base URL or token.
var ErrNotConfigured = errors.New("audit-core is not configured")

// Event is one audit row. It mirrors audit-core's AuditEvent wire shape
// (apps/Control Plane/audit-core/internal/events). Plane and Producer are set by
// the client; whatever the caller puts there is overwritten.
type Event struct {
	EventID    string         `json:"event_id"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Plane      string         `json:"plane"`
	Producer   string         `json:"producer"`
	Event      string         `json:"event"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome,omitempty"`
	Details    map[string]any `json:"details,omitempty"`
	RequestID  string         `json:"request_id,omitempty"`
}

// Client writes audit events to audit-core.
type Client struct {
	baseURL    string
	token      string
	httpClient *http.Client
}

// New builds a client. baseURL must be an absolute http(s) URL and token must
// be non-empty. httpClient may be nil (a 5 s timeout client is used); the
// client given is copied and never follows a redirect.
func New(baseURL, token string, httpClient *http.Client) (*Client, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	token = strings.TrimSpace(token)
	if baseURL == "" || token == "" {
		return nil, ErrNotConfigured
	}
	parsed, err := url.Parse(baseURL)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" {
		return nil, fmt.Errorf("%s must be an absolute http(s) URL", EnvURL)
	}
	return &Client{baseURL: baseURL, token: token, httpClient: noRedirect(httpClient)}, nil
}

// FromEnv reads AUDIT_CORE_URL and AUDIT_CORE_SERVICE_TOKEN. Both unset
// returns (nil, nil): the client is off and callers that need it refuse their
// work. Exactly one set, or an invalid URL, returns an error naming the
// setting (never its value).
func FromEnv(getenv func(string) string, httpClient *http.Client) (*Client, error) {
	rawURL := strings.TrimSpace(getenv(EnvURL))
	token := strings.TrimSpace(getenv(EnvToken))
	switch {
	case rawURL == "" && token == "":
		return nil, nil
	case rawURL == "":
		return nil, fmt.Errorf("%s is set but %s is not", EnvToken, EnvURL)
	case token == "":
		return nil, fmt.Errorf("%s is set but %s is not", EnvURL, EnvToken)
	}
	return New(rawURL, token, httpClient)
}

// Configured reports whether the client can write.
func (c *Client) Configured() bool {
	return c != nil && c.baseURL != "" && c.token != ""
}

// RecordAudit writes one event. It returns nil only when audit-core answered
// 2xx (202 for a new row, 200 for a duplicate of the same event_id and
// payload). Every other outcome, including a transport error, is an error.
func (c *Client) RecordAudit(ctx context.Context, event Event) error {
	if !c.Configured() {
		return ErrNotConfigured
	}
	if strings.TrimSpace(event.EventID) == "" || len(event.EventID) > maxEventIDLen ||
		event.OccurredAt.IsZero() || strings.TrimSpace(event.OrgID) == "" ||
		strings.TrimSpace(event.Event) == "" {
		return fmt.Errorf("audit event_id (at most %d bytes), occurred_at, org_id and event are required", maxEventIDLen)
	}
	event.Plane = Plane
	event.Producer = Producer
	event.OccurredAt = event.OccurredAt.UTC()
	if strings.TrimSpace(event.Outcome) == "" {
		event.Outcome = "ok"
	}
	body, err := json.Marshal(event)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/audit", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Id", Producer)
	req.Header.Set("X-Service-Token", c.token)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("audit-core request failed: %w", err)
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 64<<10))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("audit-core returned HTTP %d", resp.StatusCode)
	}
	return nil
}

func noRedirect(httpClient *http.Client) *http.Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultTimeout}
	}
	client := *httpClient
	client.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &client
}
