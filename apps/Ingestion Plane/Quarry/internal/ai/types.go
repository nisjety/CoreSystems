package ai

// Request and response types shared by all AIClient implementations.
// Previously defined alongside the gRPC Client; extracted here so they
// remain available after the gRPC client was removed.

// PlanRequest is the input for PlanCrawl.
type PlanRequest struct {
	URL             string
	MaxDepth        int32
	Prompt          string
	IncludePatterns []string
	ExcludePatterns []string
	OrgID           string
}

// PlanResponse is the output from PlanCrawl.
type PlanResponse struct {
	Plan string
}

// ExtractRequest is the input for ExtractData.
type ExtractRequest struct {
	HTML         string
	URL          string
	Schema       string
	Prompt       string
	SystemPrompt string
	ModelHint    ModelTier
	OrgID        string
}

// ExtractResponse is the output from ExtractData.
type ExtractResponse struct {
	Data       string
	TokensUsed int32
	Model      string
	Confidence float32
}

// ClassifyRequest is the input for ClassifyContent.
type ClassifyRequest struct {
	URL   string
	HTML  string
	OrgID string
}

// ClassifyResponse is the output from ClassifyContent.
type ClassifyResponse struct {
	ContentType string
	Confidence  float32
	Scores      map[string]float32
}

// SummarizeRequest is the input for SummarizeContent.
type SummarizeRequest struct {
	URL       string
	HTML      string
	MaxLength int
	OrgID     string
}

// SummarizeResponse is the output from SummarizeContent.
type SummarizeResponse struct {
	Summary    string
	TokensUsed int32
}

// AgentNavigateRequest is the input for AgentNavigate.
type AgentNavigateRequest struct {
	Goal         string
	StepNumber   int
	MaxSteps     int
	CurrentURL   string
	VisitedURLs  []string
	PageSnapshot string
	Schema       string
	OrgID        string
}

// NavigationAction describes a single browser action.
type NavigationAction struct {
	Type     string
	Selector string
	Value    string
	WaitMs   int32
}

// AgentNavigateResponse is the output from AgentNavigate.
type AgentNavigateResponse struct {
	Action        NavigationAction
	IsComplete    bool
	ExtractedData string
	Reasoning     string
}

// DiffFieldChange describes a single field-level change between two snapshots.
type DiffFieldChange struct {
	FieldPath  string
	ChangeType string
	OldValue   string
	NewValue   string
}

// DiffAIAnalysis is the output from AnalyzeDiff.
type DiffAIAnalysis struct {
	Summary      string
	FieldChanges []DiffFieldChange
}

// composeExtractPrompt merges an optional system prompt with an optional user
// prompt into a single combined prompt string.
func composeExtractPrompt(systemPrompt, prompt string) string {
	switch {
	case systemPrompt != "" && prompt != "":
		return "System instructions:\n" + systemPrompt + "\n\nUser request:\n" + prompt
	case systemPrompt != "":
		return "System instructions:\n" + systemPrompt
	case prompt != "":
		return prompt
	default:
		return ""
	}
}
