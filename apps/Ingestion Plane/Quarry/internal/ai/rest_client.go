package ai

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// RESTClient implements AIClient by calling Model Plane v2's ai-core HTTP
// endpoints and optionally publishing fire-and-forget signals over NATS.
type RESTClient struct {
	baseURL        string // e.g. "http://ai-core:8001"
	httpClient     *http.Client
	natsConn       *nats.Conn // nil when NATS is unavailable
	internalAPIKey string     // x-internal-api-key header value; empty = omit
}

// NewRESTClient creates a REST-backed AI client. natsURL/natsToken may be
// empty if NATS is not available — fire-and-forget methods become no-ops.
// internalAPIKey is the x-internal-api-key header value sent to ai-core;
// pass an empty string to omit the header.
func NewRESTClient(baseURL, natsURL, natsToken, internalAPIKey string) (*RESTClient, error) {
	if baseURL == "" {
		return nil, fmt.Errorf("ai-core base URL is required")
	}
	baseURL = strings.TrimRight(baseURL, "/")

	c := &RESTClient{
		baseURL:        baseURL,
		internalAPIKey: internalAPIKey,
		httpClient: &http.Client{
			Timeout: 60 * time.Second,
		},
	}

	if natsURL != "" {
		opts := []nats.Option{nats.Name("quarry-ai-rest")}
		if natsToken != "" {
			opts = append(opts, nats.Token(natsToken))
		}
		nc, err := nats.Connect(natsURL, opts...)
		if err != nil {
			log.Warn().Err(err).Msg("ai rest client: NATS unavailable, fire-and-forget signals disabled")
		} else {
			c.natsConn = nc
		}
	}

	return c, nil
}

// Close releases HTTP and NATS resources.
func (c *RESTClient) Close() error {
	if c.natsConn != nil {
		c.natsConn.Close()
	}
	c.httpClient.CloseIdleConnections()
	return nil
}

// ──────────────────────────────────────────────────────────────────────────────
// AI method implementations — each maps to POST /api/v1/chat or /api/v1/complete
// with appropriate system prompts and structured output instructions.
// ──────────────────────────────────────────────────────────────────────────────

func (c *RESTClient) PlanCrawl(ctx context.Context, req *PlanRequest) (*PlanResponse, error) {
	if req == nil {
		return nil, fmt.Errorf("plan request is nil")
	}

	systemPrompt := `You are a crawl planner. Given a URL, max depth, and patterns, produce a JSON crawl plan with fields: urls (list of seed URLs), depth (int), strategy (string: "bfs"|"sitemap"|"hybrid"), include_patterns, exclude_patterns. Respond ONLY with valid JSON.`
	userMsg := fmt.Sprintf("Plan a crawl for URL: %s\nMax depth: %d\nPrompt: %s\nInclude: %v\nExclude: %v",
		req.URL, req.MaxDepth, req.Prompt, req.IncludePatterns, req.ExcludePatterns)

	resp, err := c.chat(ctx, systemPrompt, userMsg, req.OrgID, "")
	if err != nil {
		return nil, fmt.Errorf("plan_crawl rest failed: %w", err)
	}
	return &PlanResponse{Plan: resp.Content}, nil
}

func (c *RESTClient) ExtractData(ctx context.Context, req *ExtractRequest) (*ExtractResponse, error) {
	if req == nil {
		return nil, fmt.Errorf("extract request is nil")
	}

	systemPrompt := composeExtractPrompt(req.SystemPrompt, "")
	if systemPrompt == "" {
		systemPrompt = "You are a structured data extractor. Extract data from the HTML according to the provided JSON schema. Return ONLY valid JSON matching the schema."
	}

	userMsg := fmt.Sprintf("Extract data from this page.\n\nSchema:\n%s\n\nURL: %s\n\nHTML:\n%s",
		req.Schema, req.URL, truncateHTML(req.HTML, 60000))
	if req.Prompt != "" {
		userMsg = fmt.Sprintf("%s\n\nAdditional instructions: %s", userMsg, req.Prompt)
	}

	model := ""
	if req.ModelHint != "" {
		model = mapModelTier(req.ModelHint)
	}

	resp, err := c.chat(ctx, systemPrompt, userMsg, req.OrgID, model)
	if err != nil {
		return nil, fmt.Errorf("extract_data rest failed: %w", err)
	}
	return &ExtractResponse{
		Data:       resp.Content,
		TokensUsed: int32(resp.TokensIn + resp.TokensOut),
		Model:      resp.ModelUsed,
		Confidence: 0.85, // Default confidence for REST-based extraction
	}, nil
}

func (c *RESTClient) ClassifyContent(ctx context.Context, req *ClassifyRequest) (*ClassifyResponse, error) {
	if req == nil {
		return nil, fmt.Errorf("classify request is nil")
	}

	systemPrompt := `You are a web page classifier. Classify the page into ONE of these types: product, article, blog, listing, category, search, login, error, homepage, other. Respond with JSON: {"content_type": "...", "confidence": 0.0-1.0, "scores": {"type": score, ...}}`
	userMsg := fmt.Sprintf("URL: %s\n\nHTML:\n%s", req.URL, truncateHTML(req.HTML, 30000))

	resp, err := c.chat(ctx, systemPrompt, userMsg, req.OrgID, "")
	if err != nil {
		return nil, fmt.Errorf("classify_content rest failed: %w", err)
	}

	var result struct {
		ContentType string             `json:"content_type"`
		Confidence  float32            `json:"confidence"`
		Scores      map[string]float32 `json:"scores"`
	}
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &result); err != nil {
		return &ClassifyResponse{ContentType: "other", Confidence: 0.3}, nil
	}
	return &ClassifyResponse{
		ContentType: result.ContentType,
		Confidence:  result.Confidence,
		Scores:      result.Scores,
	}, nil
}

func (c *RESTClient) SummarizeContent(ctx context.Context, req *SummarizeRequest) (*SummarizeResponse, error) {
	if req == nil {
		return nil, fmt.Errorf("summarize request is nil")
	}

	maxLen := req.MaxLength
	if maxLen <= 0 {
		maxLen = 300
	}
	systemPrompt := fmt.Sprintf("Summarize the following web page content in %d words or fewer. Be concise and factual.", maxLen)
	userMsg := fmt.Sprintf("URL: %s\n\nHTML:\n%s", req.URL, truncateHTML(req.HTML, 40000))

	resp, err := c.chat(ctx, systemPrompt, userMsg, req.OrgID, "")
	if err != nil {
		return nil, fmt.Errorf("summarize_content rest failed: %w", err)
	}
	return &SummarizeResponse{
		Summary:    resp.Content,
		TokensUsed: int32(resp.TokensIn + resp.TokensOut),
	}, nil
}

func (c *RESTClient) AgentNavigate(ctx context.Context, req *AgentNavigateRequest) (*AgentNavigateResponse, error) {
	if req == nil {
		return nil, fmt.Errorf("agent navigate request is nil")
	}

	systemPrompt := `You are a browser navigation agent. Given a page snapshot, goal, and history, decide the next action. Respond with JSON:
{"action": {"type": "click|type|scroll|wait|navigate|extract", "selector": "css", "value": "", "wait_ms": 0}, "is_complete": false, "extracted_data": "", "reasoning": "..."}`

	userMsg := fmt.Sprintf("Goal: %s\nStep %d/%d\nCurrent URL: %s\nVisited: %v\n\nPage snapshot:\n%s",
		req.Goal, req.StepNumber, req.MaxSteps, req.CurrentURL,
		req.VisitedURLs, truncateHTML(req.PageSnapshot, 40000))
	if req.Schema != "" {
		userMsg += fmt.Sprintf("\n\nExtraction schema:\n%s", req.Schema)
	}

	resp, err := c.chat(ctx, systemPrompt, userMsg, req.OrgID, "")
	if err != nil {
		return nil, fmt.Errorf("agent_navigate rest failed: %w", err)
	}

	var result struct {
		Action struct {
			Type     string `json:"type"`
			Selector string `json:"selector"`
			Value    string `json:"value"`
			WaitMs   int32  `json:"wait_ms"`
		} `json:"action"`
		IsComplete    bool   `json:"is_complete"`
		ExtractedData string `json:"extracted_data"`
		Reasoning     string `json:"reasoning"`
	}
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &result); err != nil {
		return nil, fmt.Errorf("parse navigate response: %w", err)
	}
	return &AgentNavigateResponse{
		Action: NavigationAction{
			Type:     result.Action.Type,
			Selector: result.Action.Selector,
			Value:    result.Action.Value,
			WaitMs:   result.Action.WaitMs,
		},
		IsComplete:    result.IsComplete,
		ExtractedData: result.ExtractedData,
		Reasoning:     result.Reasoning,
	}, nil
}

func (c *RESTClient) EmbedText(ctx context.Context, texts []string) ([][]float32, error) {
	if len(texts) == 0 {
		return nil, nil
	}

	// Model Plane v2's ai-core uses the chat endpoint for all tasks.
	// For embeddings we use a dedicated prompt that returns JSON arrays.
	systemPrompt := `You are an embedding proxy. For each input text, produce a 384-dimensional dense float32 vector. Return a JSON array of arrays: [[0.1, -0.2, ...], ...]. This is a placeholder — production should use a dedicated embedding model.`
	userMsg := fmt.Sprintf("Generate embeddings for %d texts:\n%s",
		len(texts), strings.Join(texts, "\n---\n"))

	// If NATS is available, prefer the complete endpoint for lower latency.
	if c.natsConn != nil {
		return c.embedViaNATS(ctx, texts)
	}

	resp, err := c.chat(ctx, systemPrompt, userMsg, "", "")
	if err != nil {
		return nil, fmt.Errorf("embed_text rest failed: %w", err)
	}

	var embeddings [][]float32
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &embeddings); err != nil {
		return nil, fmt.Errorf("parse embeddings: %w", err)
	}
	return embeddings, nil
}

func (c *RESTClient) GenerateSearchQueries(ctx context.Context, topic, findingsSummary string, seenQueries []string, maxQueries int) ([]string, error) {
	systemPrompt := fmt.Sprintf(`Generate %d diverse search queries to research a topic further. Queries should explore different aspects not yet covered. Return a JSON array of strings: ["query1", "query2", ...]`, maxQueries)
	userMsg := fmt.Sprintf("Topic: %s\n\nFindings so far:\n%s\n\nQueries already tried:\n%s",
		topic, findingsSummary, strings.Join(seenQueries, "\n"))

	resp, err := c.chat(ctx, systemPrompt, userMsg, "", "")
	if err != nil {
		return nil, fmt.Errorf("generate_search_queries rest failed: %w", err)
	}

	var queries []string
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &queries); err != nil {
		return nil, fmt.Errorf("parse queries: %w", err)
	}
	return queries, nil
}

func (c *RESTClient) ScoreScrapeOutcome(ctx context.Context, domain, engine string, success bool, latencyMs int, qualityScore float32) {
	// Fire-and-forget via NATS if available.
	if c.natsConn != nil {
		payload, _ := json.Marshal(map[string]interface{}{
			"domain":        domain,
			"engine":        engine,
			"success":       success,
			"latency_ms":    latencyMs,
			"quality_score": qualityScore,
			"timestamp":     time.Now().UTC().Format(time.RFC3339),
		})
		if err := c.natsConn.Publish("velion.ingestion.scrape.outcome", payload); err != nil {
			log.Debug().Err(err).Msg("failed to publish scrape outcome via NATS")
		}
		return
	}

	// Fallback: POST to ai-core (best-effort, no error propagation).
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	_, _ = c.chat(ctx,
		"Record this scrape outcome for learning. No response needed.",
		fmt.Sprintf("domain=%s engine=%s success=%v latency=%dms quality=%.2f", domain, engine, success, latencyMs, qualityScore),
		"", "")
}

func (c *RESTClient) AnalyzeDiff(ctx context.Context, diffText, schema, url, orgID string) (*DiffAIAnalysis, error) {
	systemPrompt := `Analyze the following unified diff of scraped data. Provide:
1. A brief summary of what changed.
2. A list of field-level changes with field_path, change_type (added/removed/modified), old_value, new_value.
Return JSON: {"summary": "...", "field_changes": [{"field_path": "...", "change_type": "...", "old_value": "...", "new_value": "..."}]}`

	userMsg := fmt.Sprintf("URL: %s\nSchema: %s\n\nDiff:\n%s", url, schema, diffText)

	resp, err := c.chat(ctx, systemPrompt, userMsg, orgID, "")
	if err != nil {
		return nil, fmt.Errorf("analyze_diff rest failed: %w", err)
	}

	var result struct {
		Summary      string `json:"summary"`
		FieldChanges []struct {
			FieldPath  string `json:"field_path"`
			ChangeType string `json:"change_type"`
			OldValue   string `json:"old_value"`
			NewValue   string `json:"new_value"`
		} `json:"field_changes"`
	}
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &result); err != nil {
		return &DiffAIAnalysis{Summary: resp.Content}, nil
	}

	analysis := &DiffAIAnalysis{Summary: result.Summary}
	for _, fc := range result.FieldChanges {
		analysis.FieldChanges = append(analysis.FieldChanges, DiffFieldChange{
			FieldPath:  fc.FieldPath,
			ChangeType: fc.ChangeType,
			OldValue:   fc.OldValue,
			NewValue:   fc.NewValue,
		})
	}
	return analysis, nil
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────────

// chatResponse mirrors Model Plane v2 ai-core ChatResponse.
type chatResponse struct {
	RequestID    string `json:"request_id"`
	Content      string `json:"content"`
	ModelUsed    string `json:"model_used"`
	Provider     string `json:"provider"`
	TokensIn     int    `json:"tokens_in"`
	TokensOut    int    `json:"tokens_out"`
	LatencyMs    int    `json:"latency_ms"`
	Intent       string `json:"intent"`
	FinishReason string `json:"finish_reason"`
}

// chat sends a request to POST /api/v1/chat on ai-core.
func (c *RESTClient) chat(ctx context.Context, systemPrompt, userMessage, orgID, model string) (*chatResponse, error) {
	body := map[string]interface{}{
		"message":    systemPrompt + "\n\n" + userMessage,
		"org_id":     orgID,
		"session_id": fmt.Sprintf("quarry-%d", time.Now().UnixNano()),
	}
	if model != "" {
		body["model"] = model
	}

	payload, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("marshal chat request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/v1/chat", bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("create http request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if c.internalAPIKey != "" {
		httpReq.Header.Set("x-internal-api-key", c.internalAPIKey)
	}

	httpResp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("http request to ai-core: %w", err)
	}
	defer httpResp.Body.Close()

	respBody, err := io.ReadAll(httpResp.Body)
	if err != nil {
		return nil, fmt.Errorf("read ai-core response: %w", err)
	}

	if httpResp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("ai-core returned %d: %s", httpResp.StatusCode, string(respBody))
	}

	var result chatResponse
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("unmarshal ai-core response: %w", err)
	}
	return &result, nil
}

// embedViaNATS sends embedding requests via NATS request/reply.
func (c *RESTClient) embedViaNATS(ctx context.Context, texts []string) ([][]float32, error) {
	reqPayload, err := json.Marshal(map[string]interface{}{
		"request_id": fmt.Sprintf("embed-%d", time.Now().UnixNano()),
		"org_id":     "",
		"model_id":   "text-embedding-3-small",
		"provider":   "openai",
		"messages": []map[string]string{
			{"role": "user", "content": strings.Join(texts, "\n---\n")},
		},
		"temperature": 0,
		"max_tokens":  1,
	})
	if err != nil {
		return nil, fmt.Errorf("marshal nats embed request: %w", err)
	}

	timeout := 10 * time.Second
	if deadline, ok := ctx.Deadline(); ok {
		timeout = time.Until(deadline)
	}

	msg, err := c.natsConn.Request("velion.ai.complete.request", reqPayload, timeout)
	if err != nil {
		return nil, fmt.Errorf("nats embed request: %w", err)
	}

	var resp struct {
		Content string `json:"content"`
	}
	if err := json.Unmarshal(msg.Data, &resp); err != nil {
		return nil, fmt.Errorf("parse nats embed response: %w", err)
	}

	var embeddings [][]float32
	if err := json.Unmarshal([]byte(cleanJSON(resp.Content)), &embeddings); err != nil {
		return nil, fmt.Errorf("parse embeddings from nats: %w", err)
	}
	return embeddings, nil
}

// mapModelTier converts a Quarry ModelTier to a Model Plane v2 model name.
func mapModelTier(tier ModelTier) string {
	switch tier {
	case TierLight:
		return "gpt-4o-mini"
	case TierStandard:
		return "gpt-4o"
	case TierHeavy:
		return "gpt-4.1"
	default:
		return ""
	}
}

// truncateHTML caps HTML size to avoid exceeding LLM context windows.
func truncateHTML(html string, maxLen int) string {
	if len(html) <= maxLen {
		return html
	}
	return html[:maxLen] + "\n... [truncated]"
}

// cleanJSON strips markdown code fences that LLMs sometimes wrap around JSON.
func cleanJSON(s string) string {
	s = strings.TrimSpace(s)
	if strings.HasPrefix(s, "```json") {
		s = strings.TrimPrefix(s, "```json")
	} else if strings.HasPrefix(s, "```") {
		s = strings.TrimPrefix(s, "```")
	}
	s = strings.TrimSuffix(s, "```")
	return strings.TrimSpace(s)
}
