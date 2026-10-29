// Package notifyclient posts delegated notification requests to
// notification-core's POST /notification-requests, signed with the HMAC
// delegation scheme its internal/delegation.Verifier requires
// (Application Plane/notification-core/internal/delegation/verifier.go).
// It is the outbound half of AUTO-2 ("notify me when a run finishes"):
// internal/runwatch calls Accept for every user who watched a run once that
// run reaches a terminal state.
//
// # Why this file is the highest-risk file in the whole feature
//
// A single wrong byte in the canonical string, an unexpected timestamp
// format, or the wrong base64 variant makes the verifier's HMAC comparison
// fail, notification-core answers 401/403, and Accept returns an error that
// internal/runwatch turns into a JetStream Nak — nothing in this package's
// own control flow looks broken, it just never notifies anyone. canonicalString
// and sign are kept as small, pure, independently-testable functions for
// exactly that reason: client_test.go pins them against an HMAC value derived
// independently of this package (a standalone script driving crypto/hmac),
// not against their own output.
//
// This mirrors the one other Go client in this monorepo that already signs
// this exact delegation scheme against a live verifier:
// Ingestion Plane/integration-corev2/internal/emailsync's
// conversationIngestDelegationHeaders (calling conversation-core's own
// delegation.Verifier) — same header names, same "v2" canonical join, same
// RawURLEncoding-everywhere convention.
package notifyclient

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// Header names notification-core's delegation.Verifier reads. x-user-id and
// x-user-role are deliberately never set by this client: the verifier treats
// an absent header exactly like an empty one (strings.TrimSpace(Get(...))),
// and this client only ever delegates as a service principal with no bound
// per-request user or role.
const (
	headerServiceID      = "x-service-id"
	headerOrganizationID = "x-org-id"
	headerTimestamp      = "x-delegation-timestamp"
	headerNonce          = "x-delegation-nonce"
	headerBodySHA256     = "x-delegation-body-sha256"
	headerSignature      = "x-delegation-signature"
)

const (
	// serviceID is this client's delegated identity. notification-core's
	// isNotificationTypeAuthorized allowlist and its principal switch in
	// CreateNotificationRequest both need a "capability-core" case for a
	// request signed with this identity to be accepted.
	serviceID = "capability-core"
	// audience MUST be the literal string notification-core's verifier is
	// configured with (cmd/server/main.go: delegation.NewVerifier(Config{
	// Audience: "notification-core", ...})) — never derived from BaseURL or
	// any other client-side configuration.
	audience = "notification-core"
	// notificationRequestsPath is the only endpoint this client calls, and
	// therefore the exact URI signed into every canonical string below. Must
	// match notification-core's actual mount point exactly — it lives inside
	// the versioned group (server.go: router.Group("/api/v1", delegated)),
	// not at the bare path.
	notificationRequestsPath = "/api/v1/notification-requests"
	// nonceBytes is the raw random byte count before base64url encoding: 18
	// bytes -> 24 base64url characters, comfortably inside the verifier's
	// required ^[A-Za-z0-9_-]{16,128}$ window.
	nonceBytes = 18
)

// RecipientKindUser is the only notification.Recipient.Kind this client
// sends; it mirrors notification-core's RecipientKindUser constant.
const RecipientKindUser = "user"

// Recipient mirrors notification-core's notification.Recipient
// (internal/notification/service.go).
type Recipient struct {
	Kind string `json:"kind"`
	ID   string `json:"id"`
}

// Request mirrors notification-core's notification.Request JSON shape
// (internal/notification/service.go), restricted to the fields a delegated
// caller is allowed to set. Source is populated server-side from the
// verified principal's ServiceID and is deliberately absent from this
// struct — this client must never send it.
type Request struct {
	OrganizationID string         `json:"organization_id"`
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	Recipient      Recipient      `json:"recipient"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	RetentionMode  string         `json:"retention_mode,omitempty"`
}

// Client posts delegated notification requests to notification-core.
//
// Now and Nonce are exported so tests can pin them (mirroring
// integration-corev2/internal/emailsync.IngestClient's Now/Nonce fields);
// New leaves them nil, and Accept falls back to time.Now / a fresh
// crypto/rand-backed nonce whenever they are nil.
type Client struct {
	BaseURL      string
	ServiceToken string
	HTTP         *http.Client
	Now          func() time.Time
	Nonce        func() (string, error)
}

// New constructs a Client, or reports that it is disabled.
//
// It returns (nil, false) — never an error — when baseURL or serviceToken is
// blank, mirroring this codebase's "absent config -> feature quietly
// disabled, not a startup failure" convention for optional external clients
// (e.g. lettatools.Config.Enabled, or dialBackends' nil pair in
// capability-core/cmd/main.go): a deployment that has not yet wired
// notification-core loses only the run-watch notify path, never the rest of
// capability-core.
func New(baseURL, serviceToken string, httpClient *http.Client) (*Client, bool) {
	baseURL = strings.TrimSpace(baseURL)
	serviceToken = strings.TrimSpace(serviceToken)
	if baseURL == "" || serviceToken == "" {
		return nil, false
	}
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 10 * time.Second}
	}
	return &Client{
		BaseURL:      strings.TrimRight(baseURL, "/"),
		ServiceToken: serviceToken,
		HTTP:         httpClient,
	}, true
}

// Accept posts req to notification-core's POST /notification-requests,
// signed as capability-core.
//
// req.OrganizationID doubles as the delegated x-org-id scope AND the body's
// organization_id field: CreateNotificationRequest 403s unless the two match
// exactly (trimmed-string equality), so this is a hard requirement of the
// endpoint, not a client-side courtesy default.
func (c *Client) Accept(ctx context.Context, req Request) error {
	if c == nil {
		return errors.New("notifyclient: client is disabled (no base URL/service token configured)")
	}
	orgID := strings.TrimSpace(req.OrganizationID)
	if orgID == "" {
		return errors.New("notifyclient: organization_id is required")
	}
	req.OrganizationID = orgID

	// Marshal EXACTLY once. The digest and the transmitted body must be the
	// identical bytes — re-marshaling an equal-looking struct a second time
	// could still reorder map keys (Payload is a map[string]any) and desync
	// the signed digest from what notification-core actually receives. That
	// failure mode is silent and fails CLOSED (a legitimate notification
	// rejected), which is exactly backwards from what a caller would expect.
	body, err := json.Marshal(req)
	if err != nil {
		return fmt.Errorf("notifyclient: marshal request: %w", err)
	}

	now := c.Now
	if now == nil {
		now = time.Now
	}
	nonceFn := c.Nonce
	if nonceFn == nil {
		nonceFn = randomNonce
	}
	nonce, err := nonceFn()
	if err != nil {
		return fmt.Errorf("notifyclient: generate nonce: %w", err)
	}
	nonce = strings.TrimSpace(nonce)
	if nonce == "" {
		return errors.New("notifyclient: empty delegation nonce")
	}

	timestamp := now().UTC().Format(time.RFC3339)
	digest := bodyDigest(body)
	canonical := canonicalString(canonicalFields{
		ServiceID:      serviceID,
		Audience:       audience,
		Timestamp:      timestamp,
		Nonce:          nonce,
		Method:         http.MethodPost,
		URI:            notificationRequestsPath,
		UserID:         "",
		OrganizationID: orgID,
		Role:           "",
		BodySHA256:     digest,
	})
	signature := sign([]byte(c.ServiceToken), canonical)

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.BaseURL+notificationRequestsPath, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("notifyclient: build request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set(headerServiceID, serviceID)
	httpReq.Header.Set(headerOrganizationID, orgID)
	httpReq.Header.Set(headerTimestamp, timestamp)
	httpReq.Header.Set(headerNonce, nonce)
	httpReq.Header.Set(headerBodySHA256, digest)
	httpReq.Header.Set(headerSignature, signature)

	httpClient := c.HTTP
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 10 * time.Second}
	}
	resp, err := httpClient.Do(httpReq)
	if err != nil {
		return fmt.Errorf("notifyclient: post %s: %w", notificationRequestsPath, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return fmt.Errorf("notifyclient: notification-core returned %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}
	return nil
}

// canonicalFields mirrors notification-core's delegation.CanonicalFields
// (Application Plane/notification-core/internal/delegation/verifier.go)
// field for field, in the same order — Canonical() joins them positionally,
// not by name, so a reordered struct here would silently sign the wrong
// string and every request would fail verification.
type canonicalFields struct {
	ServiceID, Audience, Timestamp, Nonce, Method, URI, UserID, OrganizationID, Role, BodySHA256 string
}

// canonicalString mirrors notification-core's delegation.Canonical byte for
// byte: the literal "v2" plus the ten fields above, joined with "\n". Both
// sides must agree on this exact string or every signature this client
// produces fails the verifier's HMAC comparison.
func canonicalString(f canonicalFields) string {
	return strings.Join([]string{
		"v2", f.ServiceID, f.Audience, f.Timestamp, f.Nonce,
		f.Method, f.URI, f.UserID, f.OrganizationID, f.Role, f.BodySHA256,
	}, "\n")
}

// bodyDigest is base64.RawURLEncoding(sha256(body)) — RawURLEncoding (no
// padding, URL-safe alphabet), never StdEncoding: the verifier compares this
// exact string against its own RawURLEncoding digest, and StdEncoding's
// '+'/'/' padding characters would never match.
func bodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

// sign is HMAC-SHA256(secret, canonical), base64.RawURLEncoding-encoded —
// the same construction and encoding the verifier applies to compute the
// value it compares this against.
func sign(secret []byte, canonical string) string {
	mac := hmac.New(sha256.New, secret)
	mac.Write([]byte(canonical))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

// randomNonce returns a fresh nonce satisfying the verifier's
// ^[A-Za-z0-9_-]{16,128}$ pattern and its per-(serviceID,nonce) freshness
// check: nonceBytes random bytes, base64.RawURLEncoding-encoded — the same
// unpadded, URL-safe alphabet the regex allows.
func randomNonce() (string, error) {
	buf := make([]byte, nonceBytes)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("notifyclient: read random nonce: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buf), nil
}
