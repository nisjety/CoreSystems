package hotpath

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/providers"
)

var ErrInvalidJSON = errors.New("invalid webhook json")

type WebhookInput struct {
	ProviderKey string
	Headers     map[string]string
	Body        []byte
}

type WebhookNormalization struct {
	SchemaVersion  int            `json:"schemaVersion"`
	ProviderKey    string         `json:"providerKey"`
	EventType      string         `json:"eventType"`
	OrganizationID string         `json:"organizationId,omitempty"`
	SignatureHash  string         `json:"signatureHash,omitempty"`
	EventID        string         `json:"eventId"`
	ReplayKey      string         `json:"replayKey"`
	BodySHA256     string         `json:"bodySha256"`
	Payload        map[string]any `json:"payload"`
	NormalizedBy   string         `json:"normalizedBy"`
	Warnings       []string       `json:"warnings,omitempty"`
}

type WebhookNormalizer interface {
	NormalizeWebhook(ctx context.Context, input WebhookInput) (WebhookNormalization, error)
}

type HTTPWebhookNormalizer struct {
	baseURL    string
	httpClient *http.Client
}

func NewHTTPWebhookNormalizer(baseURL string, httpClient *http.Client) *HTTPWebhookNormalizer {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		return nil
	}
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 2 * time.Second}
	}
	return &HTTPWebhookNormalizer{baseURL: baseURL, httpClient: httpClient}
}

func (n *HTTPWebhookNormalizer) NormalizeWebhook(ctx context.Context, input WebhookInput) (WebhookNormalization, error) {
	if n == nil || n.baseURL == "" {
		return WebhookNormalization{}, errors.New("webhook hot path is not configured")
	}
	payload := map[string]any{
		"providerKey": input.ProviderKey,
		"headers":     input.Headers,
		"bodyBase64":  base64.StdEncoding.EncodeToString(input.Body),
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return WebhookNormalization{}, fmt.Errorf("marshal webhook normalize request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, n.baseURL+"/v1/webhooks/normalize", bytes.NewReader(body))
	if err != nil {
		return WebhookNormalization{}, fmt.Errorf("build webhook normalize request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := n.httpClient.Do(req)
	if err != nil {
		return WebhookNormalization{}, fmt.Errorf("call webhook hot path: %w", err)
	}
	defer resp.Body.Close()
	var decoded struct {
		Success bool                 `json:"success"`
		Data    WebhookNormalization `json:"data"`
		Error   string               `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		return WebhookNormalization{}, fmt.Errorf("decode webhook hot path response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 || !decoded.Success {
		if decoded.Error == "" {
			decoded.Error = resp.Status
		}
		if resp.StatusCode == http.StatusBadRequest {
			return WebhookNormalization{}, fmt.Errorf("%w: %s", ErrInvalidJSON, decoded.Error)
		}
		return WebhookNormalization{}, fmt.Errorf("webhook hot path rejected event: %s", decoded.Error)
	}
	return decoded.Data, nil
}

func NormalizeWebhook(input WebhookInput) (WebhookNormalization, error) {
	providerKey := providers.NormalizeKey(input.ProviderKey)
	payload := map[string]any{}
	if len(input.Body) > 0 {
		if err := json.Unmarshal(input.Body, &payload); err != nil {
			return WebhookNormalization{}, fmt.Errorf("%w: %v", ErrInvalidJSON, err)
		}
	}
	bodySHA := sha256.Sum256(input.Body)
	bodySHAHex := hex.EncodeToString(bodySHA[:])
	signatureHash := hashFirstHeader(input.Headers,
		"Stripe-Signature",
		"X-Slack-Signature",
		"X-Hub-Signature-256",
		"X-GitHub-Delivery",
		"X-Shopify-Hmac-Sha256",
		"X-Webhook-Signature",
	)
	eventType := firstNonEmpty(
		header(input.Headers, "X-Event-Type"),
		header(input.Headers, "X-GitHub-Event"),
		header(input.Headers, "X-Shopify-Topic"),
		stringFromAny(payload["type"]),
		stringFromAny(payload["event"]),
		"provider.webhook",
	)
	organizationID := firstNonEmpty(
		header(input.Headers, "X-Org-ID"),
		stringFromAny(payload["organizationId"]),
		stringFromAny(payload["organization_id"]),
	)
	replayKey := firstNonEmpty(
		stringFromAny(payload["id"]),
		stringFromAny(payload["eventId"]),
		stringFromAny(payload["event_id"]),
		stringFromAny(payload["deliveryId"]),
		stringFromAny(payload["delivery_id"]),
		header(input.Headers, "X-GitHub-Delivery"),
		signatureHash,
		bodySHAHex,
	)
	eventID := webhookEventID(providerKey, eventType, replayKey)
	return WebhookNormalization{
		SchemaVersion:  1,
		ProviderKey:    providerKey,
		EventType:      eventType,
		OrganizationID: organizationID,
		SignatureHash:  signatureHash,
		EventID:        eventID,
		ReplayKey:      replayKey,
		BodySHA256:     bodySHAHex,
		Payload:        payload,
		NormalizedBy:   "go-fallback",
	}, nil
}

func WebhookHeaders(headerValues map[string]string) map[string]string {
	out := map[string]string{}
	for _, name := range []string{
		"Stripe-Signature",
		"X-Slack-Signature",
		"X-Slack-Request-Timestamp",
		"X-Hub-Signature-256",
		"X-GitHub-Delivery",
		"X-GitHub-Event",
		"X-Shopify-Hmac-Sha256",
		"X-Shopify-Topic",
		"X-Webhook-Signature",
		"X-Event-Type",
		"X-Org-ID",
	} {
		if value := strings.TrimSpace(headerValues[name]); value != "" {
			out[name] = value
		}
	}
	return out
}

func hashFirstHeader(headers map[string]string, names ...string) string {
	value := ""
	for _, name := range names {
		if value = header(headers, name); value != "" {
			break
		}
	}
	if value == "" {
		return ""
	}
	sum := sha256.Sum256([]byte(value))
	return hex.EncodeToString(sum[:])
}

func webhookEventID(providerKey, eventType, replayKey string) string {
	sum := sha256.Sum256([]byte(providerKey + "|" + eventType + "|" + replayKey))
	return "wh_" + providers.NormalizeKey(providerKey) + "_" + hex.EncodeToString(sum[:])[:32]
}

func header(headers map[string]string, name string) string {
	for key, value := range headers {
		if strings.EqualFold(key, name) {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func stringFromAny(value any) string {
	switch typed := value.(type) {
	case string:
		return strings.TrimSpace(typed)
	case fmt.Stringer:
		return strings.TrimSpace(typed.String())
	default:
		return ""
	}
}
