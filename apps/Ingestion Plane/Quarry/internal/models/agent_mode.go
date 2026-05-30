package models

import "github.com/triodelab/quarry/internal/security"

type AgentModeAPIRequest struct {
	UserID                string                 `json:"userId,omitempty"`
	Tier                  string                 `json:"tier,omitempty"`
	Objective             string                 `json:"objective"`
	URL                   string                 `json:"url,omitempty"`
	URLs                  []string               `json:"urls,omitempty"`
	Schema                interface{}            `json:"schema,omitempty"`
	Model                 string                 `json:"model,omitempty"`
	Module                string                 `json:"module,omitempty"`
	Collection            string                 `json:"collection,omitempty"`
	MaxSteps              int                    `json:"maxSteps,omitempty"`
	MaxCredits            int                    `json:"maxCredits,omitempty"`
	StrictConstrainToURLs bool                   `json:"strictConstrainToUrls,omitempty"`
	EnableWebSearch       bool                   `json:"enableWebSearch,omitempty"`
	AllowExternalLinks    bool                   `json:"allowExternalLinks,omitempty"`
	Agent                 *AgentOptions          `json:"agent,omitempty"`
	Context               map[string]interface{} `json:"context,omitempty"`
	ChangeTrack           *ChangeTrackingRequest `json:"changeTracking,omitempty"`
	Webhook               *WebhookConfig         `json:"webhook,omitempty"`
	TimeoutSec            int                    `json:"timeout,omitempty"`
}

// AgentOptions allows per-request override of agent behaviour.
type AgentOptions struct {
	Model              string `json:"model,omitempty"`
	MaxCredits         int    `json:"maxCredits,omitempty"`
	EnableWebSearch    bool   `json:"enableWebSearch,omitempty"`
	AllowExternalLinks bool   `json:"allowExternalLinks,omitempty"`
}

// ResolvedURLs returns the union of URL and URLs as a single deduplicated slice.
func (r *AgentModeAPIRequest) ResolvedURLs() []string {
	seen := make(map[string]struct{})
	var out []string
	if r.URL != "" {
		seen[r.URL] = struct{}{}
		out = append(out, r.URL)
	}
	for _, u := range r.URLs {
		if _, exists := seen[u]; !exists {
			seen[u] = struct{}{}
			out = append(out, u)
		}
	}
	return out
}

// ResolvedEnableWebSearch returns true if enableWebSearch is set at the top
// level or inside the nested agent options block.
func (r *AgentModeAPIRequest) ResolvedEnableWebSearch() bool {
	if r.EnableWebSearch {
		return true
	}
	if r.Agent != nil && r.Agent.EnableWebSearch {
		return true
	}
	return false
}

// ResolvedAllowExternalLinks returns true if allowExternalLinks is set at the
// top level or inside the nested agent options block.
func (r *AgentModeAPIRequest) ResolvedAllowExternalLinks() bool {
	if r.AllowExternalLinks {
		return true
	}
	if r.Agent != nil && r.Agent.AllowExternalLinks {
		return true
	}
	return false
}

type AgentModeResult struct {
	Content      string      `json:"content"`
	ModelUsed    string      `json:"modelUsed"`
	Intent       string      `json:"intent"`
	Confidence   float64     `json:"confidence"`
	RequestID    string      `json:"requestId"`
	TotalCostUSD float64     `json:"totalCostUsd"`
	DurationMs   float64     `json:"durationMs"`
	Structured   interface{} `json:"structured,omitempty"`
}

type AgentModeAPIResponse struct {
	Success        bool                  `json:"success"`
	Data           *AgentModeResult      `json:"data,omitempty"`
	ChangeTracking *ChangeTrackingResult `json:"changeTracking,omitempty"`
	Security       *security.Assessment  `json:"security,omitempty"`
	Error          string                `json:"error,omitempty"`
}

type AgentStatusEnvelope struct {
	Success     bool             `json:"success"`
	ID          string           `json:"id"`
	Resource    string           `json:"resource"`
	Status      string           `json:"status"`
	CreatedAt   string           `json:"createdAt,omitempty"`
	ExpiresAt   string           `json:"expiresAt,omitempty"`
	Objective   string           `json:"objective,omitempty"`
	URLsScraped int              `json:"urlsScraped"`
	Data        *AgentModeResult `json:"data,omitempty"`
	Error       string           `json:"error,omitempty"`
}
