// Package quarrycontracts — cross-plane Go contract types.
//
// These mirror the Rust types in `crates/quarry-core/src/contracts.rs` so
// the Go orchestrator and control plane can build/validate the same
// payload shapes Quarry's Rust hot path emits.
//
// JSON tags must stay in sync with Rust serde naming. Tests in this package
// roundtrip representative payloads through both languages to catch drift.

package quarrycontracts

import (
	"encoding/json"
	"time"
)

// ZdrMode mirrors `quarry_core::zdr::ZdrMode`.
type ZdrMode string

const (
	ZdrOff ZdrMode = "off"
	ZdrOn  ZdrMode = "on"
)

// IndexStatus mirrors `quarry_core::contracts::IndexStatus`.
type IndexStatus string

const (
	IndexPending IndexStatus = "pending"
	IndexIndexed IndexStatus = "indexed"
	IndexFailed  IndexStatus = "failed"
	IndexSkipped IndexStatus = "skipped"
)

// EmbeddingStatus mirrors `quarry_core::contracts::EmbeddingStatus`.
type EmbeddingStatus string

const (
	EmbeddingPending  EmbeddingStatus = "pending"
	EmbeddingEmbedded EmbeddingStatus = "embedded"
	EmbeddingFailed   EmbeddingStatus = "failed"
	EmbeddingSkipped  EmbeddingStatus = "skipped"
)

// ChunkRef points to a chunked artifact slice produced by the index engine.
type ChunkRef struct {
	ChunkID  string          `json:"chunk_id"`
	Index    int             `json:"index"`
	Bytes    int             `json:"bytes"`
	Metadata json.RawMessage `json:"metadata,omitempty"`
}

// FieldTrace records the source span an extracted field came from.
type FieldTrace struct {
	Selector    string    `json:"selector,omitempty"`
	ExtractedAt time.Time `json:"extracted_at"`
	XPath       string    `json:"xpath,omitempty"`
}

// SourceTrace bundles per-field provenance for an extracted document.
type SourceTrace struct {
	Fields      map[string]FieldTrace `json:"fields"`
	SourceURL   string                `json:"source_url,omitempty"`
	FetchedAt   *time.Time            `json:"fetched_at,omitempty"`
	Fingerprint string                `json:"fingerprint,omitempty"`
}

// DataPlaneIngestRequest mirrors the Rust contract Quarry posts to Data Plane.
type DataPlaneIngestRequest struct {
	RunID           ID              `json:"run_id"`
	OrgID           string          `json:"org_id"`
	SourceURL       string          `json:"source_url"`
	Title           *string         `json:"title,omitempty"`
	Markdown        *string         `json:"markdown,omitempty"`
	HTMLRef         *string         `json:"html_ref,omitempty"`
	RawRef          *string         `json:"raw_ref,omitempty"`
	Chunks          []ChunkRef      `json:"chunks,omitempty"`
	Metadata        json.RawMessage `json:"metadata,omitempty"`
	Fingerprint     string          `json:"fingerprint"`
	Zdr             ZdrMode         `json:"zdr"`
	RetentionPolicy *string         `json:"retention_policy,omitempty"`
	SourceTrace     *SourceTrace    `json:"source_trace,omitempty"`
}

// DataPlaneIngestResponse mirrors the Rust contract.
type DataPlaneIngestResponse struct {
	DocumentID         string          `json:"document_id"`
	IndexStatus        IndexStatus     `json:"index_status"`
	KnowledgeUnitCount uint32          `json:"knowledge_unit_count"`
	EmbeddingStatus    EmbeddingStatus `json:"embedding_status"`
	RetrievableAfter   *time.Time      `json:"retrievable_after,omitempty"`
	TraceID            string          `json:"trace_id"`
}

// AgentAction is a tagged enum mirroring `quarry_core::contracts::AgentAction`.
// The serde tag is "type" with snake_case content names — when constructing
// from Go, set `Type` to the variant name (snake_case) and populate the
// matching content field.
type AgentAction struct {
	Type    string          `json:"type"`
	Payload json.RawMessage `json:"payload,omitempty"`
}

// AgentConstraints mirrors the Rust contract.
type AgentConstraints struct {
	MaxSteps       *uint32  `json:"max_steps,omitempty"`
	MaxRuntimeS    *uint32  `json:"max_runtime_s,omitempty"`
	MaxCostUSD     *float64 `json:"max_cost_usd,omitempty"`
	AllowedDomains []string `json:"allowed_domains,omitempty"`
}

// AgentActionRequest is the wire shape Model Plane sends to Quarry to
// request a browser action.
type AgentActionRequest struct {
	RunID       ID                `json:"run_id"`
	LeaseID     string            `json:"lease_id"`
	Action      AgentAction       `json:"action"`
	Instruction string            `json:"instruction,omitempty"`
	Constraints *AgentConstraints `json:"constraints,omitempty"`
	Zdr         ZdrMode           `json:"zdr"`
}

// StructuredExtractRequest mirrors the Rust contract.
type StructuredExtractRequest struct {
	RunID                  ID              `json:"run_id"`
	OrgID                  string          `json:"org_id"`
	SourceArtifactRef      *string         `json:"source_artifact_ref,omitempty"`
	Markdown               *string         `json:"markdown,omitempty"`
	StructuredOutputSchema json.RawMessage `json:"structured_output_schema"`
	SourceTraceRequired    bool            `json:"source_trace_required,omitempty"`
	MaxCostUSD             *float64        `json:"max_cost_usd,omitempty"`
	MaxTokens              *uint32         `json:"max_tokens,omitempty"`
	Zdr                    ZdrMode         `json:"zdr"`
}

// ExtractionUsage mirrors the Rust contract.
type ExtractionUsage struct {
	InputTokens  uint32  `json:"input_tokens"`
	OutputTokens uint32  `json:"output_tokens"`
	CostUSD      float64 `json:"cost_usd"`
}

// StructuredExtractResponse mirrors the Rust contract.
type StructuredExtractResponse struct {
	Data        json.RawMessage `json:"data"`
	SchemaValid bool            `json:"schema_valid"`
	Usage       ExtractionUsage `json:"usage"`
	SourceTrace *SourceTrace    `json:"source_trace,omitempty"`
	Model       string          `json:"model"`
	Provider    string          `json:"provider"`
}

// IsZeroRetention returns true when the request opts into ZDR enforcement.
func (r *DataPlaneIngestRequest) IsZeroRetention() bool {
	return r != nil && r.Zdr == ZdrOn
}

// HasContent returns true when the ingest request carries persistable
// content. Used by ZDR-enforcing receivers to reject content+ZDR combinations.
func (r *DataPlaneIngestRequest) HasContent() bool {
	if r == nil {
		return false
	}
	return r.Markdown != nil || r.HTMLRef != nil || r.RawRef != nil
}
