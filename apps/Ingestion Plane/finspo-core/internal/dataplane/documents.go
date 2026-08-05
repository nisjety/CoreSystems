package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// DocumentsClient forwards content-bearing SharePoint files into Data Plane v2's
// durable document store (POST /v1/documents). It is the content-carrying
// counterpart to SourceObjectClient, which only ships metadata to the Quickwit
// source-object index. Both use a short-lived, org-constrained Control-issued
// service JWT. Data Plane verifies and pins the tenant from that credential;
// caller-selected identity headers are never sent.
//
// Data Plane v2 owns documents; Ingestion persists durable knowledge only
// through this contract. This client never touches Data Plane's database
// directly — it always goes through the documents-api HTTP boundary, which
// enforces validation, ownership, and idempotency.
type DocumentsClient struct {
	baseURL    string
	tokens     OrgTokenProvider
	httpClient *http.Client
}

func NewDocumentsClient(baseURL string, tokens OrgTokenProvider) *DocumentsClient {
	return &DocumentsClient{
		baseURL: strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		tokens:  tokens,
		httpClient: &http.Client{
			Timeout:       30 * time.Second,
			CheckRedirect: noBearerRedirect,
		},
	}
}

func noBearerRedirect(_ *http.Request, _ []*http.Request) error {
	return http.ErrUseLastResponse
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
	// ModifiedAt is the source item's own last-modified time (SharePoint's
	// lastModifiedDateTime), forwarded as document_date. Distinct from
	// Data Plane's own created_at/updated_at bookkeeping. Nil is a valid,
	// common value -- most non-SharePoint content has no such field.
	ModifiedAt *time.Time `json:"document_date,omitempty"`
}

// Configured reports whether the client can talk to Data Plane. Mirrors the
// nil-safe optional-integration gating used across the ingestion plane: an
// unconfigured client is a no-op, never an error.
func (c *DocumentsClient) Configured() bool {
	return c != nil && c.baseURL != "" && c.tokens != nil && c.tokens.Configured()
}

// CreateDocument POSTs one document to Data Plane v2. When the client is
// unconfigured it is a silent no-op (returns nil) so callers can wire it
// unconditionally and let configuration decide whether forwarding happens.
func (c *DocumentsClient) CreateDocument(ctx context.Context, orgID string, input CreateDocumentInput) error {
	if !c.Configured() {
		return nil
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
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
	if input.ModifiedAt != nil {
		payload["document_date"] = input.ModifiedAt
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal document payload: %w", err)
	}

	for attempt := 0; attempt < 2; attempt++ {
		token, err := c.tokens.Token(ctx, orgID)
		if err != nil {
			return err
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/documents", bytes.NewReader(body))
		if err != nil {
			return fmt.Errorf("build data plane document request: %w", err)
		}
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer "+token)
		resp, err := c.httpClient.Do(req)
		if err != nil {
			return fmt.Errorf("data plane document request: %w", err)
		}
		if resp.StatusCode == http.StatusUnauthorized && attempt == 0 {
			_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
			_ = resp.Body.Close()
			c.tokens.Invalidate(orgID, token)
			continue
		}
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 64<<10))
		_ = resp.Body.Close()
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return fmt.Errorf("data plane document request returned %s", resp.Status)
		}
		return nil
	}
	return fmt.Errorf("data plane document authentication failed after refresh")
}
