package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// DocumentsClient forwards content-bearing SharePoint files into Data Plane v2's
// durable document store (POST /v1/documents). It is the content-carrying
// counterpart to SourceObjectClient, which only ships metadata to the Quickwit
// source-object index. Both share the same base URL + internal API key and the
// same X-Org-ID / X-Internal-Api-Key auth headers.
//
// Data Plane v2 owns documents; Ingestion persists durable knowledge only
// through this contract. This client never touches Data Plane's database
// directly — it always goes through the documents-api HTTP boundary, which
// enforces validation, ownership, and idempotency.
type DocumentsClient struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

func NewDocumentsClient(baseURL, apiKey string) *DocumentsClient {
	return &DocumentsClient{
		baseURL: strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:  strings.TrimSpace(apiKey),
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

// CreateDocumentInput mirrors documents-api-go model.CreateDocumentInput for the
// fields finspo-core populates. Content MUST be non-empty — documents-api
// rejects empty bodies with 400 "content is required".
type CreateDocumentInput struct {
	Source            string         `json:"source"`
	Type              string         `json:"type"`
	Title             string         `json:"title"`
	Content           string         `json:"content"`
	ZDRClassification string         `json:"zdr_classification,omitempty"`
	Metadata          map[string]any `json:"metadata,omitempty"`
	IdempotencyKey    string         `json:"idempotency_key,omitempty"`
	CreatedBy         string         `json:"created_by,omitempty"`
}

// Configured reports whether the client can talk to Data Plane. Mirrors the
// nil-safe optional-integration gating used across the ingestion plane: an
// unconfigured client is a no-op, never an error.
func (c *DocumentsClient) Configured() bool {
	return c != nil && c.baseURL != "" && c.apiKey != ""
}

// CreateDocument POSTs one document to Data Plane v2. When the client is
// unconfigured it is a silent no-op (returns nil) so callers can wire it
// unconditionally and let configuration decide whether forwarding happens.
func (c *DocumentsClient) CreateDocument(ctx context.Context, orgID string, input CreateDocumentInput) error {
	if !c.Configured() {
		return nil
	}
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("orgID is required")
	}
	if strings.TrimSpace(input.Content) == "" {
		// Fail loudly rather than let documents-api 400: an empty body means the
		// extractor produced nothing and the caller should have skipped.
		return fmt.Errorf("document content is empty")
	}

	payload := map[string]any{
		"org_id":  orgID,
		"source":  input.Source,
		"type":    input.Type,
		"title":   input.Title,
		"content": input.Content,
	}
	if input.ZDRClassification != "" {
		payload["zdr_classification"] = input.ZDRClassification
	}
	if len(input.Metadata) > 0 {
		payload["metadata"] = input.Metadata
	}
	if input.IdempotencyKey != "" {
		payload["idempotency_key"] = input.IdempotencyKey
	}
	if input.CreatedBy != "" {
		payload["created_by"] = input.CreatedBy
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal document payload: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/documents", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build data plane document request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Org-ID", orgID)
	req.Header.Set("X-Internal-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("data plane document request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("data plane document request returned %s", resp.Status)
	}
	return nil
}
