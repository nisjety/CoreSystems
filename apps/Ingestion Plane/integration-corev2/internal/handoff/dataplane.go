package handoff

import (
	"context"
	"fmt"
	"net/http"

	"github.com/triodelab/integration-corev2/internal/config"
)

type DataPlaneDocumentsClient struct {
	baseURL        string
	internalAPIKey string
	apiKeyHeader   string
	httpClient     httpClient
}

type DataPlaneDocumentRequest struct {
	OrgID     string         `json:"org_id"`
	Source    string         `json:"source"`
	Type      string         `json:"type"`
	Title     string         `json:"title"`
	Content   string         `json:"content,omitempty"`
	Metadata  map[string]any `json:"metadata,omitempty"`
	CreatedBy string         `json:"created_by,omitempty"`
}

type DataPlaneDocument struct {
	ID         string         `json:"id,omitempty"`
	DocumentID string         `json:"document_id,omitempty"`
	OrgID      string         `json:"org_id,omitempty"`
	Source     string         `json:"source,omitempty"`
	Type       string         `json:"type,omitempty"`
	Title      string         `json:"title,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
}

func NewDataPlaneDocumentsClient(baseURL, internalAPIKey, apiKeyHeader string, httpClient *http.Client) *DataPlaneDocumentsClient {
	return &DataPlaneDocumentsClient{
		baseURL:        normalizeBaseURL(baseURL),
		internalAPIKey: internalAPIKey,
		apiKeyHeader:   normalizeHeader(apiKeyHeader, "X-Internal-Api-Key"),
		httpClient:     withDefaultHTTPClient(httpClient),
	}
}

func NewDataPlaneDocumentsClientFromConfig(cfg config.Config, httpClient *http.Client) *DataPlaneDocumentsClient {
	return NewDataPlaneDocumentsClient(cfg.DataPlaneDocumentsURL, cfg.DataPlaneInternalAPIKey, cfg.DataPlaneInternalAPIHeader, httpClient)
}

func (c *DataPlaneDocumentsClient) CreateDocument(ctx context.Context, input DataPlaneDocumentRequest) (DataPlaneDocument, error) {
	if c == nil || c.baseURL == "" || c.internalAPIKey == "" {
		return DataPlaneDocument{}, ErrNotConfigured
	}
	req, err := jsonRequest(ctx, http.MethodPost, c.baseURL+"/internal/v1/documents", input)
	if err != nil {
		return DataPlaneDocument{}, err
	}
	req.Header.Set(c.apiKeyHeader, c.internalAPIKey)
	req.Header.Set("X-Org-Id", input.OrgID)
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return DataPlaneDocument{}, fmt.Errorf("data-plane documents request failed: %w", err)
	}
	return decodeJSONOrEnvelope[DataPlaneDocument]("data-plane documents", resp)
}
