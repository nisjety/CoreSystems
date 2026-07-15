package lago

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

type Config struct {
	BaseURL string
	APIKey  string
	Timeout time.Duration
}

type Adapter struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

func NewAdapter(cfg Config) *Adapter {
	if cfg.BaseURL == "" {
		cfg.BaseURL = "http://lago:3000"
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = 10 * time.Second
	}

	return &Adapter{
		baseURL: strings.TrimRight(cfg.BaseURL, "/"),
		apiKey:  cfg.APIKey,
		httpClient: &http.Client{
			Timeout: cfg.Timeout,
		},
	}
}

func (a *Adapter) ReportUsage(ctx context.Context, usage billing.UsageEvent) error {
	if a.apiKey == "" {
		return fmt.Errorf("lago api key is not configured")
	}

	payload := map[string]any{
		"event": map[string]any{
			"transaction_id":       usage.EventID,
			"external_customer_id": usage.OrgID,
			"code":                 usage.Metric,
			"timestamp":            usage.OccurredAt.UTC().Unix(), // Lago requires Unix epoch seconds
			"properties": map[string]any{
				"quantity": usage.Quantity,
				"source":   usage.Source,
			},
		},
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal lago payload: %w", err)
	}

	endpoint := a.baseURL + "/api/v1/events"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("create lago request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+a.apiKey)
	req.Header.Set("Content-Type", "application/json")

	resp, err := a.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("lago request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return fmt.Errorf("lago returned %d: %s", resp.StatusCode, string(respBody))
	}

	log.Printf("lago-adapter usage synced: event=%s org=%s metric=%s", usage.EventID, usage.OrgID, usage.Metric)
	return nil
}
