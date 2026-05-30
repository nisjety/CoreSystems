package ai

import "context"

// AIClient defines the contract for all AI backends.
// The RESTClient targets Model Plane v2 ai-core (HTTP only; no gRPC).
type AIClient interface {
	PlanCrawl(ctx context.Context, req *PlanRequest) (*PlanResponse, error)
	ExtractData(ctx context.Context, req *ExtractRequest) (*ExtractResponse, error)
	ClassifyContent(ctx context.Context, req *ClassifyRequest) (*ClassifyResponse, error)
	SummarizeContent(ctx context.Context, req *SummarizeRequest) (*SummarizeResponse, error)
	AgentNavigate(ctx context.Context, req *AgentNavigateRequest) (*AgentNavigateResponse, error)
	EmbedText(ctx context.Context, texts []string) ([][]float32, error)
	GenerateSearchQueries(ctx context.Context, topic, findingsSummary string, seenQueries []string, maxQueries int) ([]string, error)
	ScoreScrapeOutcome(ctx context.Context, domain, engine string, success bool, latencyMs int, qualityScore float32)
	AnalyzeDiff(ctx context.Context, diffText, schema, url, orgID string) (*DiffAIAnalysis, error)
	Close() error
}

// Compile-time interface check.
var (
	_ AIClient = (*RESTClient)(nil)
)
