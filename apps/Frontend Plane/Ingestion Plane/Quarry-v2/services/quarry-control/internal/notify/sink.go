// Package notify delivers in-product notifications to the Application Plane's
// notification-core over HTTP. Quarry never writes notification-core's tables
// directly — the plane boundary is the HTTP intake. Each call is signed with
// notification-core's delegation contract (HMAC v2) as the quarry-control
// service principal, because notification-core rejects the legacy internal key.
package notify

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
	"net/url"
	"strings"
	"time"
)

// ServiceID is the delegation principal notification-core knows this caller by.
const ServiceID = "quarry-control"

// Audience is the audience notification-core checks the delegation against.
const Audience = "notification-core"

// ChangeDetectedType is the notification type notification-core allows this service to send.
const ChangeDetectedType = "change_detected"

const intakePath = "/api/v1/notification-requests"

// Sink delivers a single in-product notification for a detected change.
// Implementations must be safe for concurrent use and idempotent on eventID
// (so a replayed event batch doesn't create duplicate notifications).
type Sink interface {
	NotifyChange(ctx context.Context, orgID, recipientID, eventID string, payload map[string]any) error
}

// HTTPSink posts to notification-core's intake (POST /api/v1/notification-requests),
// signed with the quarry-control delegation secret.
type HTTPSink struct {
	baseURL string
	secret  []byte
	client  *http.Client
	now     func() time.Time
	nonce   func() (string, error)
}

// NewHTTPSink builds a sink targeting notification-core at baseURL. The secret is
// the delegation key notification-core holds for quarry-control.
func NewHTTPSink(baseURL, secret string) *HTTPSink {
	return &HTTPSink{
		baseURL: strings.TrimRight(baseURL, "/"),
		secret:  []byte(strings.TrimSpace(secret)),
		client:  &http.Client{Timeout: 5 * time.Second},
		now:     time.Now,
		nonce:   randomNonce,
	}
}

// NotifyChange creates a `change_detected` feed entry for recipientID in orgID.
// idempotency_key = the source event id, so notification-core dedupes a replayed
// event batch into a single notification.
func (s *HTTPSink) NotifyChange(ctx context.Context, orgID, recipientID, eventID string, payload map[string]any) error {
	if len(s.secret) == 0 {
		return errors.New("notification-core delegation secret is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	recipientID = strings.TrimSpace(recipientID)
	eventID = strings.TrimSpace(eventID)
	if orgID == "" || recipientID == "" || eventID == "" {
		return errors.New("change notification needs an organization, a recipient and an event id")
	}

	body, err := json.Marshal(map[string]any{
		"organization_id": orgID,
		"idempotency_key": eventID,
		"recipient":       map[string]string{"kind": "user", "id": recipientID},
		"type":            ChangeDetectedType,
		"payload":         changePayload(payload),
		"source":          "quarry",
		"retention_mode":  "standard",
	})
	if err != nil {
		return err
	}

	timestamp := s.now().UTC().Format(time.RFC3339)
	nonce, err := s.nonce()
	if err != nil {
		return err
	}
	digest := sha256.Sum256(body)
	bodyDigest := base64.RawURLEncoding.EncodeToString(digest[:])
	// The role is empty: this principal acts for the recipient, not for a role.
	canonical := canonicalDelegation(ServiceID, Audience, timestamp, nonce, http.MethodPost, intakePath, recipientID, orgID, "", bodyDigest)
	mac := hmac.New(sha256.New, s.secret)
	_, _ = mac.Write([]byte(canonical))
	signature := base64.RawURLEncoding.EncodeToString(mac.Sum(nil))

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.baseURL+intakePath, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("content-type", "application/json")
	req.Header.Set("x-service-id", ServiceID)
	req.Header.Set("x-user-id", recipientID)
	req.Header.Set("x-org-id", orgID)
	req.Header.Set("x-delegation-timestamp", timestamp)
	req.Header.Set("x-delegation-nonce", nonce)
	req.Header.Set("x-delegation-body-sha256", bodyDigest)
	req.Header.Set("x-delegation-signature", signature)

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

// canonicalDelegation is the string notification-core signs for a delegated call. It
// mirrors notification-core's delegation.Canonical field for field and in order.
func canonicalDelegation(serviceID, audience, timestamp, nonce, method, uri, userID, orgID, role, bodySHA256 string) string {
	return strings.Join([]string{
		"v2",
		serviceID,
		audience,
		timestamp,
		nonce,
		method,
		uri,
		userID,
		orgID,
		role,
		bodySHA256,
	}, "\n")
}

// changePayload turns the orchestrator's change event into what the feed shows: the
// title and body, which notification-core reads by those keys, and the references the
// feed keeps (the URL, the status, the diff and the baseline). The fingerprint and any
// page content are left out.
func changePayload(event map[string]any) map[string]any {
	pageURL := stringField(event, "url")
	return map[string]any{
		"title":       "Endring på " + hostOf(pageURL),
		"body":        "Siden har endret seg. Åpne Innhenting for å se endringen.",
		"url":         pageURL,
		"status":      stringField(event, "status"),
		"diff_id":     stringField(event, "diff_id"),
		"baseline_id": stringField(event, "baseline_id"),
	}
}

func stringField(event map[string]any, key string) string {
	value, _ := event[key].(string)
	return strings.TrimSpace(value)
}

// hostOf names the page by its host, so the title reads without the full URL.
func hostOf(raw string) string {
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Host == "" {
		return "en overvåket side"
	}
	return parsed.Host
}

// randomNonce returns 16 random bytes, base64url without padding: 22 characters,
// inside notification-core's nonce pattern.
func randomNonce() (string, error) {
	var raw [16]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(raw[:]), nil
}
