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

// CreateDocument POSTs to Data Plane v2's real route, POST /v1/documents
// (documents-api-go internal/handler/documents.go DocumentHandler.Create,
// mounted at cmd/main.go's r.Route("/v1/documents", ...) — there is no
// "/internal" prefix on this route; that prefix only ever existed in this
// dead client, never in documents-api-go). The route requires:
//   - X-Internal-Api-Key (or X-Internal-Key / X-Api-Key): internalAuthMiddleware
//   - X-Org-ID (capitalized "ID", not "Id"): handler.OrgIDMiddleware reads
//     r.Header.Get("X-Org-ID") exactly; Go's http.Header canonicalizes casing
//     on both Set and Get so this was not a live bug, but the header is
//     written here in the exact casing the receiver documents to avoid
//     relying on that canonicalization implicitly.
//   - A non-empty Content field: validate.CreateDocument rejects
//     content == "" with 400 "content is required". Callers must not invoke
//     this with a synthetic/fabricated Content value — see finspo.go's
//     worker, which does NOT call this method today because no source in
//     this codebase extracts real document text yet.
func (c *DataPlaneDocumentsClient) CreateDocument(ctx context.Context, input DataPlaneDocumentRequest) (DataPlaneDocument, error) {
	if c == nil || c.baseURL == "" || c.internalAPIKey == "" {
		return DataPlaneDocument{}, ErrNotConfigured
	}
	req, err := jsonRequest(ctx, http.MethodPost, c.baseURL+"/v1/documents", input)
	if err != nil {
		return DataPlaneDocument{}, err
	}
	req.Header.Set(c.apiKeyHeader, c.internalAPIKey)
	req.Header.Set("X-Org-ID", input.OrgID)
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return DataPlaneDocument{}, fmt.Errorf("data-plane documents request failed: %w", err)
	}
	return decodeJSONOrEnvelope[DataPlaneDocument]("data-plane documents", resp)
}

// Configured reports whether the client has enough configuration to make
// requests. Callers (e.g. finspo-worker) use this to skip optional
// Data Plane forwarding silently instead of crashing when
// DATA_PLANE_DOCUMENTS_URL / DATA_PLANE_INTERNAL_API_KEY are unset, mirroring
// the nil-safe optional-integration gating used elsewhere in this codebase
// (e.g. ServerConfig.Actions/Billing/Audit/Events in internal/api/server.go).
func (c *DataPlaneDocumentsClient) Configured() bool {
	return c != nil && c.baseURL != "" && c.internalAPIKey != ""
}
