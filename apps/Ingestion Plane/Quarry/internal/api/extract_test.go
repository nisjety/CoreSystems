package api

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/batch"
	"github.com/triodelab/quarry/internal/dataplane"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/tracker"
)

func TestNormalizeSchema(t *testing.T) {
	t.Parallel()

	handler := &Handler{schemaCache: NewSchemaCache(time.Minute)}
	t.Cleanup(func() {
		if err := handler.schemaCache.Close(); err != nil {
			t.Fatalf("SchemaCache.Close() error = %v", err)
		}
	})

	tests := []struct {
		name    string
		input   json.RawMessage
		want    string
		wantErr bool
	}{
		{
			name:  "object schema",
			input: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"}}}`),
			want:  `{"properties":{"name":{"type":"string"}},"type":"object"}`,
		},
		{
			name:  "json encoded string schema",
			input: json.RawMessage(`"{\"type\":\"object\",\"required\":[\"name\"]}"`),
			want:  `{"required":["name"],"type":"object"}`,
		},
		{
			name:    "invalid schema string",
			input:   json.RawMessage(`"not-json"`),
			wantErr: true,
		},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got, err := handler.normalizeSchema(tt.input)
			if tt.wantErr {
				if err == nil {
					t.Fatal("normalizeSchema() error = nil, want non-nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("normalizeSchema() error = %v", err)
			}
			if got != tt.want {
				t.Fatalf("normalizeSchema() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestExtractLifecycleAndIngest(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		schemaCache:        NewSchemaCache(time.Minute),
		extractStructuredFn: func(ctx context.Context, targetURL string, opts *scraper.StructuredExtractOptions) (map[string]interface{}, error) {
			return map[string]interface{}{
				"name":       "Example",
				"source_url": targetURL,
				"schema":     opts.Schema,
			}, nil
		},
		ingestExtractionFn: func(ctx context.Context, orgID string, req *dataplane.DocumentCreateRequest) (*dataplane.DocumentResponse, error) {
			if orgID != "org-1" {
				t.Fatalf("orgID = %q, want org-1", orgID)
			}
			if req.Source != "quarry" {
				t.Fatalf("source = %q, want quarry", req.Source)
			}
			if req.Type != "extraction" {
				t.Fatalf("type = %q, want extraction", req.Type)
			}
			if req.Metadata["url"] != "https://example.com/item" {
				t.Fatalf("metadata.url = %v, want https://example.com/item", req.Metadata["url"])
			}
			return &dataplane.DocumentResponse{
				DocumentID: "doc-123",
				OrgID:      orgID,
				Source:     req.Source,
				Type:       req.Type,
				Title:      req.Title,
				Status:     "created",
				Metadata:   req.Metadata,
			}, nil
		},
	}
	t.Cleanup(func() {
		if err := handler.Close(); err != nil {
			t.Fatalf("Handler.Close() error = %v", err)
		}
	})

	app := fiber.New()
	app.Post("/v1/extract", handler.handleExtract)
	app.Get("/v1/extract/:id", handler.handleExtractStatus)
	app.Post("/v1/extract/:id/ingest", handler.handleExtractIngest)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"url":    "https://example.com/item",
		"schema": map[string]interface{}{"type": "object", "properties": map[string]interface{}{"name": map[string]interface{}{"type": "string"}}},
	})
	if createResp.StatusCode != http.StatusAccepted {
		t.Fatalf("POST /v1/extract status = %d, want %d", createResp.StatusCode, http.StatusAccepted)
	}

	var queued ExtractResponse
	decodeJSONResponse(t, createResp, &queued)
	if queued.JobID == "" {
		t.Fatal("jobId should not be empty")
	}

	status := waitForExtractionStatus(t, app, queued.JobID, jobs.ExtractionCompleted)
	if status.Result["name"] != "Example" {
		t.Fatalf("result.name = %v, want Example", status.Result["name"])
	}
	if status.ExpiresAt == "" {
		t.Fatal("expires_at should be populated")
	}

	ingestResp := performJSONRequest(t, app, http.MethodPost, "/v1/extract/"+queued.JobID+"/ingest", map[string]interface{}{
		"org_id": "org-1",
	})
	if ingestResp.StatusCode != http.StatusOK {
		t.Fatalf("POST /v1/extract/:id/ingest status = %d, want %d", ingestResp.StatusCode, http.StatusOK)
	}

	var ingested IngestResponse
	decodeJSONResponse(t, ingestResp, &ingested)
	if ingested.DocumentID != "doc-123" {
		t.Fatalf("documentId = %q, want doc-123", ingested.DocumentID)
	}
}

func TestExtractReturnsBadRequestForInvalidSchema(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		schemaCache:        NewSchemaCache(time.Minute),
	}
	t.Cleanup(func() {
		if err := handler.Close(); err != nil {
			t.Fatalf("Handler.Close() error = %v", err)
		}
	})

	app := fiber.New()
	app.Post("/v1/extract", handler.handleExtract)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"url":    "https://example.com/item",
		"schema": "not-json",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}

	var payload ErrorEnvelope
	decodeJSONResponse(t, resp, &payload)
	if payload.Error != "schema must be valid JSON or a JSON-encoded string" {
		t.Fatalf("error = %q", payload.Error)
	}
}

func TestExtractFailsWhenStructuredExtractorIsUnavailable(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		schemaCache:        NewSchemaCache(time.Minute),
	}
	t.Cleanup(func() {
		if err := handler.Close(); err != nil {
			t.Fatalf("Handler.Close() error = %v", err)
		}
	})

	app := fiber.New()
	app.Post("/v1/extract", handler.handleExtract)
	app.Get("/v1/extract/:id", handler.handleExtractStatus)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"url":    "https://example.com/item",
		"prompt": "extract the product name",
	})
	if resp.StatusCode != http.StatusAccepted {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusAccepted)
	}

	var queued ExtractResponse
	decodeJSONResponse(t, resp, &queued)

	status := waitForExtractionStatus(t, app, queued.JobID, jobs.ExtractionFailed)
	if !strings.Contains(status.Error, "scraper is not initialized") {
		t.Fatalf("error = %q, want substring %q", status.Error, "scraper is not initialized")
	}
}

func TestExtractStatusNotFound(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
	}
	t.Cleanup(func() {
		if err := handler.Close(); err != nil {
			t.Fatalf("Handler.Close() error = %v", err)
		}
	})

	app := fiber.New()
	app.Get("/v1/extract/:id", handler.handleExtractStatus)

	resp := performJSONRequest(t, app, http.MethodGet, "/v1/extract/missing", nil)
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusNotFound)
	}
}

func TestHandlerCloseIsIdempotent(t *testing.T) {
	t.Setenv("JOB_STORE_BACKEND", "")
	t.Setenv("CHANGE_TRACKING_BACKEND", "")

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		schemaCache:        NewSchemaCache(time.Minute),
		changeTracker:      tracker.NewChangeTracker(),
		batchManager:       batch.NewManager(nil, 1, time.Minute, ""),
	}

	if err := handler.Close(); err != nil {
		t.Fatalf("Close() first call error = %v", err)
	}
	if err := handler.Close(); err != nil {
		t.Fatalf("Close() second call error = %v", err)
	}
}

func waitForExtractionStatus(t *testing.T, app *fiber.App, jobID string, want jobs.ExtractionStatus) ExtractStatusResponse {
	t.Helper()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/extract/"+jobID, nil)
		if resp.StatusCode != http.StatusOK {
			var errPayload ErrorEnvelope
			decodeJSONResponse(t, resp, &errPayload)
			t.Fatalf("unexpected status poll response: %+v", errPayload)
		}

		var status ExtractStatusResponse
		decodeJSONResponse(t, resp, &status)
		if jobs.ExtractionStatus(status.Status) == want {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}

	t.Fatalf("timed out waiting for extraction status %q", want)
	return ExtractStatusResponse{}
}
