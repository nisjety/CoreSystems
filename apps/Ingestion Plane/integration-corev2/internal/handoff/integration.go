package handoff

import (
	"context"
	"fmt"
	"net/http"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

type IntegrationClient struct {
	baseURL        string
	internalAPIKey string
	apiKeyHeader   string
	httpClient     httpClient
}

type SyncClaimRequest struct {
	Consumer       string         `json:"consumer"`
	Target         string         `json:"target,omitempty"`
	OrganizationID string         `json:"organizationId,omitempty"`
	ProviderKey    string         `json:"providerKey,omitempty"`
	Checkpoint     map[string]any `json:"checkpoint,omitempty"`
	Metadata       map[string]any `json:"metadata,omitempty"`
}

type SyncProgressRequest struct {
	Consumer   string          `json:"consumer"`
	Status     string          `json:"status"`
	Message    string          `json:"message,omitempty"`
	Checkpoint map[string]any  `json:"checkpoint,omitempty"`
	Metadata   map[string]any  `json:"metadata,omitempty"`
	Sources    []SyncSourceRef `json:"sources,omitempty"`
}

type SyncSourceRef struct {
	Provider   string `json:"provider,omitempty"`
	Type       string `json:"type,omitempty"`
	SourceID   string `json:"sourceId,omitempty"`
	ExternalID string `json:"externalId,omitempty"`
	Status     string `json:"status,omitempty"`
	Title      string `json:"title,omitempty"`
	URL        string `json:"url,omitempty"`
}

type syncJobEnvelope struct {
	SyncJob store.SyncJob `json:"syncJob"`
}

func NewIntegrationClient(baseURL, internalAPIKey, apiKeyHeader string, httpClient *http.Client) *IntegrationClient {
	return &IntegrationClient{
		baseURL:        normalizeBaseURL(baseURL),
		internalAPIKey: internalAPIKey,
		apiKeyHeader:   normalizeHeader(apiKeyHeader, "X-Internal-API-Key"),
		httpClient:     withDefaultHTTPClient(httpClient),
	}
}

func NewIntegrationClientFromConfig(cfg config.Config, httpClient *http.Client) *IntegrationClient {
	return NewIntegrationClient(cfg.IntegrationCoreURL, cfg.InternalAPIKey, cfg.InternalAPIKeyHeader, httpClient)
}

func (c *IntegrationClient) ClaimSyncJob(ctx context.Context, input SyncClaimRequest) (store.SyncJob, error) {
	req, err := c.request(ctx, http.MethodPost, c.baseURL+"/internal/sync-jobs/claim", input)
	if err != nil {
		return store.SyncJob{}, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return store.SyncJob{}, fmt.Errorf("integration-core sync claim failed: %w", err)
	}
	result, err := decodeEnvelope[syncJobEnvelope]("integration-core", resp)
	if err != nil {
		return store.SyncJob{}, err
	}
	return result.SyncJob, nil
}

func (c *IntegrationClient) UpdateSyncProgress(ctx context.Context, jobID string, input SyncProgressRequest) (store.SyncJob, error) {
	req, err := c.request(ctx, http.MethodPatch, pathJoin(c.baseURL+"/internal/sync-jobs", jobID, "progress"), input)
	if err != nil {
		return store.SyncJob{}, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return store.SyncJob{}, fmt.Errorf("integration-core sync progress failed: %w", err)
	}
	result, err := decodeEnvelope[syncJobEnvelope]("integration-core", resp)
	if err != nil {
		return store.SyncJob{}, err
	}
	return result.SyncJob, nil
}

func (c *IntegrationClient) request(ctx context.Context, method, endpoint string, body any) (*http.Request, error) {
	if c == nil || c.baseURL == "" || c.internalAPIKey == "" {
		return nil, ErrNotConfigured
	}
	req, err := jsonRequest(ctx, method, endpoint, body)
	if err != nil {
		return nil, err
	}
	req.Header.Set(c.apiKeyHeader, c.internalAPIKey)
	return req, nil
}
