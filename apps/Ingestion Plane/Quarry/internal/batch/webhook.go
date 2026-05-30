package batch

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/models"
)

// WebhookDelivery handles webhook delivery with signature verification
type WebhookDelivery struct {
	secret string
	client *http.Client
}

// NewWebhookDelivery creates a new webhook delivery handler
func NewWebhookDelivery(secret string) *WebhookDelivery {
	return &WebhookDelivery{
		secret: secret,
		client: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// Send delivers a webhook payload with HMAC-SHA256 signature
func (w *WebhookDelivery) Send(url string, payload *models.WebhookPayload) error {
	// Marshal payload
	data, err := json.Marshal(payload)
	if err != nil {
		log.Error().Err(err).Msg("Failed to marshal webhook payload")
		return err
	}

	// Generate signature
	signature := w.generateSignature(data)

	// Create request
	req, err := http.NewRequest("POST", url, bytes.NewBuffer(data))
	if err != nil {
		log.Error().Err(err).Str("url", url).Msg("Failed to create webhook request")
		return err
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Firecrawl-Signature", fmt.Sprintf("sha256=%s", signature))
	req.Header.Set("User-Agent", "SkinSecretScraper-Webhook/1.0")

	// Send request
	resp, err := w.client.Do(req)
	if err != nil {
		log.Error().Err(err).Str("url", url).Msg("Failed to send webhook")
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		log.Warn().
			Int("status", resp.StatusCode).
			Str("url", url).
			Str("type", payload.Type).
			Msg("Webhook returned non-2xx status")
		return fmt.Errorf("webhook returned status %d", resp.StatusCode)
	}

	log.Info().
		Str("url", url).
		Str("type", payload.Type).
		Int("status", resp.StatusCode).
		Msg("Webhook delivered successfully")

	return nil
}

// generateSignature creates an HMAC-SHA256 signature of the payload
func (w *WebhookDelivery) generateSignature(data []byte) string {
	if w.secret == "" {
		return ""
	}

	mac := hmac.New(sha256.New, []byte(w.secret))
	mac.Write(data)
	return hex.EncodeToString(mac.Sum(nil))
}

// VerifySignature verifies a webhook signature (for incoming webhooks if needed)
func (w *WebhookDelivery) VerifySignature(signature string, data []byte) bool {
	if w.secret == "" {
		return false
	}

	// Remove "sha256=" prefix if present
	if len(signature) > 7 && signature[:7] == "sha256=" {
		signature = signature[7:]
	}

	expectedSignature := w.generateSignature(data)
	return hmac.Equal([]byte(signature), []byte(expectedSignature))
}
