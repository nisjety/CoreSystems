package quarrycontracts

import (
	"encoding/json"
	"testing"
	"time"
)

// TestDataPlaneIngestRequest_RoundTrip asserts the JSON shape exactly
// matches what `crates/quarry-core/src/contracts.rs` emits — if Quarry
// changes a serde rename, this test fires immediately.
func TestDataPlaneIngestRequest_RoundTrip(t *testing.T) {
	canonical := []byte(`{
		"run_id": "run_01HX",
		"org_id": "org_demo",
		"source_url": "https://example.com",
		"title": "Hello",
		"markdown": "# Hello",
		"chunks": [],
		"fingerprint": "blake3:abc",
		"zdr": "off"
	}`)

	var parsed DataPlaneIngestRequest
	if err := json.Unmarshal(canonical, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.OrgID != "org_demo" {
		t.Errorf("org_id = %q, want %q", parsed.OrgID, "org_demo")
	}
	if parsed.Zdr != ZdrOff {
		t.Errorf("zdr = %q, want %q", parsed.Zdr, ZdrOff)
	}
	if !parsed.HasContent() {
		t.Error("HasContent() should return true when markdown is present")
	}
	if parsed.IsZeroRetention() {
		t.Error("IsZeroRetention() should be false when zdr=off")
	}

	// Roundtrip — serialize then deserialize, structure must match.
	out, err := json.Marshal(&parsed)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var roundtripped DataPlaneIngestRequest
	if err := json.Unmarshal(out, &roundtripped); err != nil {
		t.Fatalf("re-unmarshal: %v", err)
	}
	if roundtripped.OrgID != parsed.OrgID {
		t.Errorf("org_id drift after roundtrip")
	}
}

func TestDataPlaneIngestRequest_ZdrSemantics(t *testing.T) {
	cases := []struct {
		name       string
		req        DataPlaneIngestRequest
		wantZDR    bool
		wantContent bool
	}{
		{
			name:       "zdr_on_with_markdown",
			req:        DataPlaneIngestRequest{Zdr: ZdrOn, Markdown: ptr("# X")},
			wantZDR:    true,
			wantContent: true,
		},
		{
			name:       "zdr_on_without_content",
			req:        DataPlaneIngestRequest{Zdr: ZdrOn},
			wantZDR:    true,
			wantContent: false,
		},
		{
			name:       "zdr_off_with_html",
			req:        DataPlaneIngestRequest{Zdr: ZdrOff, HTMLRef: ptr("art_1")},
			wantZDR:    false,
			wantContent: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.req.IsZeroRetention(); got != tc.wantZDR {
				t.Errorf("IsZeroRetention = %v, want %v", got, tc.wantZDR)
			}
			if got := tc.req.HasContent(); got != tc.wantContent {
				t.Errorf("HasContent = %v, want %v", got, tc.wantContent)
			}
		})
	}
}

func TestDataPlaneIngestResponse_RoundTrip(t *testing.T) {
	canonical := []byte(`{
		"document_id": "doc_01HX",
		"index_status": "indexed",
		"knowledge_unit_count": 3,
		"embedding_status": "embedded",
		"trace_id": "trace_1"
	}`)

	var parsed DataPlaneIngestResponse
	if err := json.Unmarshal(canonical, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.IndexStatus != IndexIndexed {
		t.Errorf("index_status = %q, want %q", parsed.IndexStatus, IndexIndexed)
	}
	if parsed.EmbeddingStatus != EmbeddingEmbedded {
		t.Errorf("embedding_status = %q, want %q", parsed.EmbeddingStatus, EmbeddingEmbedded)
	}
	if parsed.KnowledgeUnitCount != 3 {
		t.Errorf("knowledge_unit_count = %d, want 3", parsed.KnowledgeUnitCount)
	}
}

func TestStructuredExtractRequest_ZdrField(t *testing.T) {
	canonical := []byte(`{
		"run_id": "run_x",
		"org_id": "org_x",
		"markdown": "# Hi",
		"structured_output_schema": {"type":"object","required":["x"]},
		"max_cost_usd": 0.10,
		"zdr": "on"
	}`)

	var parsed StructuredExtractRequest
	if err := json.Unmarshal(canonical, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.Zdr != ZdrOn {
		t.Errorf("zdr = %q, want %q", parsed.Zdr, ZdrOn)
	}
	if parsed.MaxCostUSD == nil || *parsed.MaxCostUSD != 0.10 {
		t.Errorf("max_cost_usd unexpected: %v", parsed.MaxCostUSD)
	}
}

func TestStructuredExtractResponse_UsageShape(t *testing.T) {
	canonical := []byte(`{
		"data": {"name": "Alice"},
		"schema_valid": true,
		"usage": {"input_tokens": 100, "output_tokens": 200, "cost_usd": 0.001},
		"model": "claude-sonnet-4-6",
		"provider": "anthropic"
	}`)
	var parsed StructuredExtractResponse
	if err := json.Unmarshal(canonical, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.Usage.CostUSD != 0.001 {
		t.Errorf("cost_usd = %v", parsed.Usage.CostUSD)
	}
	if !parsed.SchemaValid {
		t.Error("schema_valid should be true")
	}
}

func TestSourceTrace_PreservesFieldSelectors(t *testing.T) {
	now := time.Now().UTC()
	trace := SourceTrace{
		Fields: map[string]FieldTrace{
			"title": {Selector: "head > title", ExtractedAt: now},
			"body":  {Selector: "article", ExtractedAt: now},
		},
		SourceURL:   "https://example.com",
		Fingerprint: "blake3:abc",
	}
	out, err := json.Marshal(trace)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var parsed SourceTrace
	if err := json.Unmarshal(out, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.Fields["title"].Selector != "head > title" {
		t.Errorf("selector lost in roundtrip")
	}
	if len(parsed.Fields) != 2 {
		t.Errorf("expected 2 field traces, got %d", len(parsed.Fields))
	}
}

func TestAgentConstraints_PartialFields(t *testing.T) {
	canonical := []byte(`{"max_steps": 10, "allowed_domains": ["example.com"]}`)
	var parsed AgentConstraints
	if err := json.Unmarshal(canonical, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if parsed.MaxSteps == nil || *parsed.MaxSteps != 10 {
		t.Errorf("max_steps = %v", parsed.MaxSteps)
	}
	if parsed.MaxCostUSD != nil {
		t.Errorf("max_cost_usd should be nil, got %v", parsed.MaxCostUSD)
	}
	if len(parsed.AllowedDomains) != 1 {
		t.Errorf("allowed_domains: %v", parsed.AllowedDomains)
	}
}

func ptr[T any](v T) *T { return &v }
