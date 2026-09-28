package mailtask

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// GatewayIngestRequest is the normalized event forwarded to the Frontend
// Plane gateway's POST /internal/mail-task/ingest, once the sender is
// verified and attachments are extracted.
type GatewayIngestRequest struct {
	OrgID             string                    `json:"org_id"`
	UserID            string                    `json:"user_id"`
	ProviderMessageID string                    `json:"provider_message_id"`
	Subject           string                    `json:"subject"`
	BodyText          string                    `json:"body_text"`
	Attachments       []GatewayIngestAttachment `json:"attachments"`
	OccurredAt        time.Time                 `json:"occurred_at"`
	// ThreadDecision rides inside this same HMAC-delegated body (the
	// existing body_sha256 covers it automatically, no canonical-string
	// change needed) -- pure pass-through, this client never reads it
	// (delegated-identity design, Gap B §2.3). omitempty so a caller that
	// resolved before user-core added this field keeps sending an
	// identical wire body.
	ThreadDecision string `json:"thread_decision,omitempty"`
}

type GatewayIngestAttachment struct {
	Filename      string `json:"filename"`
	ExtractedText string `json:"extracted_text,omitempty"`
	Extracted     bool   `json:"extracted"`
}

// GatewayIngestClient forwards one normalized mail-task event on to the
// gateway.
type GatewayIngestClient interface {
	Ingest(ctx context.Context, req GatewayIngestRequest) error
}

// HTTPGatewayIngestClient calls the gateway with the same HMAC
// service-delegation scheme conversation-ingest-rs already verifies
// (conversation-ingest-rs/src/lib.rs:308-367) and emailsync.go already sends
// (internal/emailsync/emailsync.go's conversationIngestDelegationHeaders) --
// service id "integration-mail-task-worker", audience "verevon-gateway".
type HTTPGatewayIngestClient struct {
	BaseURL      string
	ServiceToken string
	Client       *http.Client
}

const gatewayIngestServiceID = "integration-mail-task-worker"
const gatewayIngestAudience = "verevon-gateway"
const gatewayIngestPath = "/internal/mail-task/ingest"

func (c *HTTPGatewayIngestClient) Ingest(ctx context.Context, req GatewayIngestRequest) error {
	if c == nil || strings.TrimSpace(c.BaseURL) == "" {
		return fmt.Errorf("mail task gateway ingest client is not configured")
	}
	client := c.Client
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Second}
	}
	body, err := json.Marshal(req)
	if err != nil {
		return fmt.Errorf("encode gateway ingest request: %w", err)
	}
	timestamp := time.Now().UTC()
	nonce := newDelegationNonce()
	headers := mailTaskIngestDelegationHeaders(c.ServiceToken, http.MethodPost, gatewayIngestPath, body, req.OrgID, timestamp, nonce)

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.BaseURL, "/")+gatewayIngestPath, bytes.NewReader(body))
	if err != nil {
		return err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	for name, value := range headers {
		httpReq.Header.Set(name, value)
	}
	resp, err := client.Do(httpReq)
	if err != nil {
		return fmt.Errorf("call gateway mail-task ingest: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2048))
		return fmt.Errorf("gateway mail-task ingest returned HTTP %d", resp.StatusCode)
	}
	return nil
}

// mailTaskIngestDelegationHeaders mirrors
// internal/emailsync/emailsync.go's conversationIngestDelegationHeaders byte
// for byte (same "v2" canonical-string shape), with this endpoint's own
// service id/audience/path substituted in. Kept as a free function (not a
// method) so it is directly unit-testable against a fixed nonce/timestamp,
// the same way conversation-ingest-rs's Rust side of an equivalent contract
// is (conversation_delegation_matches_go_cross_language_contract).
func mailTaskIngestDelegationHeaders(
	serviceToken string,
	method string,
	requestURI string,
	body []byte,
	organizationID string,
	timestamp time.Time,
	nonce string,
) map[string]string {
	timestampText := timestamp.UTC().Format(time.RFC3339)
	digest := bodyDigest(body)
	canonical := strings.Join([]string{
		"v2",
		gatewayIngestServiceID,
		gatewayIngestAudience,
		timestampText,
		nonce,
		method,
		requestURI,
		"",
		strings.TrimSpace(organizationID),
		"",
		digest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(strings.TrimSpace(serviceToken)))
	_, _ = mac.Write([]byte(canonical))
	return map[string]string{
		"x-service-id":             gatewayIngestServiceID,
		"x-org-id":                 strings.TrimSpace(organizationID),
		"x-delegation-timestamp":   timestampText,
		"x-delegation-nonce":       nonce,
		"x-delegation-body-sha256": digest,
		"x-delegation-signature":   base64.RawURLEncoding.EncodeToString(mac.Sum(nil)),
	}
}

func bodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func newDelegationNonce() string {
	buf := make([]byte, 18)
	if _, err := rand.Read(buf); err != nil {
		return ""
	}
	return base64.RawURLEncoding.EncodeToString(buf)
}
