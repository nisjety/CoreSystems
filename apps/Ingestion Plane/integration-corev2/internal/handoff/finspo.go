package handoff

import (
	"context"
	"fmt"
	"net/http"

	"github.com/triodelab/integration-corev2/internal/config"
)

type FinspoClient struct {
	baseURL      string
	apiKey       string
	apiKeyHeader string
	httpClient   httpClient
}

type FinspoSourceRequest struct {
	SiteID     string `json:"site_id"`
	DriveID    string `json:"drive_id"`
	TenantID   string `json:"tenant_id,omitempty"`
	SiteWebURL string `json:"site_web_url,omitempty"`
	DriveName  string `json:"drive_name,omitempty"`
	DriveType  string `json:"drive_type,omitempty"`
	Enabled    *bool  `json:"enabled,omitempty"`
}

type FinspoSource struct {
	ID        string `json:"id"`
	SiteID    string `json:"site_id,omitempty"`
	DriveID   string `json:"drive_id,omitempty"`
	DriveName string `json:"drive_name,omitempty"`
	Status    string `json:"status,omitempty"`
}

type FinspoSyncResult struct {
	ID       string `json:"id,omitempty"`
	SourceID string `json:"source_id,omitempty"`
	Status   string `json:"status,omitempty"`
	JobID    string `json:"job_id,omitempty"`
}

// FinspoSourceList mirrors finspo-core's GET /api/v1/sources data payload
// (listSourcesHandler): `{count, sources}` inside the standard envelope.
type FinspoSourceList struct {
	Count   int            `json:"count"`
	Sources []FinspoSource `json:"sources"`
}

func NewFinspoClient(baseURL, apiKey, apiKeyHeader string, httpClient *http.Client) *FinspoClient {
	return &FinspoClient{
		baseURL:      normalizeBaseURL(baseURL),
		apiKey:       apiKey,
		apiKeyHeader: normalizeHeader(apiKeyHeader, "X-API-Key"),
		httpClient:   withDefaultHTTPClient(httpClient),
	}
}

func NewFinspoClientFromConfig(cfg config.Config, httpClient *http.Client) *FinspoClient {
	return NewFinspoClient(cfg.FinspoCoreURL, cfg.FinspoCoreAPIKey, cfg.FinspoCoreAPIKeyHeader, httpClient)
}

func (c *FinspoClient) EnsureSource(ctx context.Context, orgID, userID string, input FinspoSourceRequest) (FinspoSource, error) {
	req, err := c.request(ctx, http.MethodPost, c.baseURL+"/api/v1/sources", orgID, userID, input)
	if err != nil {
		return FinspoSource{}, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return FinspoSource{}, fmt.Errorf("finspo-core source request failed: %w", err)
	}
	return decodeEnvelope[FinspoSource]("finspo-core", resp)
}

// ListSources returns every SharePoint source already registered for the
// organization in finspo-core. Used by the finspo worker as the fallback
// sync target set when a claimed sync job carries no site_id/drive_id of
// its own (the generic per-connection "sync now" path).
func (c *FinspoClient) ListSources(ctx context.Context, orgID, userID string) ([]FinspoSource, error) {
	req, err := c.request(ctx, http.MethodGet, c.baseURL+"/api/v1/sources", orgID, userID, nil)
	if err != nil {
		return nil, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("finspo-core list sources request failed: %w", err)
	}
	list, err := decodeEnvelope[FinspoSourceList]("finspo-core", resp)
	if err != nil {
		return nil, err
	}
	return list.Sources, nil
}

func (c *FinspoClient) SyncSource(ctx context.Context, orgID, userID, sourceID string) (FinspoSyncResult, error) {
	req, err := c.request(ctx, http.MethodPost, pathJoin(c.baseURL+"/api/v1/sources", sourceID, "sync"), orgID, userID, nil)
	if err != nil {
		return FinspoSyncResult{}, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return FinspoSyncResult{}, fmt.Errorf("finspo-core sync request failed: %w", err)
	}
	return decodeEnvelope[FinspoSyncResult]("finspo-core", resp)
}

func (c *FinspoClient) request(ctx context.Context, method, endpoint, orgID, userID string, body any) (*http.Request, error) {
	if c == nil || c.baseURL == "" || c.apiKey == "" {
		return nil, ErrNotConfigured
	}
	req, err := jsonRequest(ctx, method, endpoint, body)
	if err != nil {
		return nil, err
	}
	req.Header.Set(c.apiKeyHeader, c.apiKey)
	req.Header.Set("X-Org-ID", orgID)
	if userID != "" {
		req.Header.Set("X-User-ID", userID)
	}
	return req, nil
}
