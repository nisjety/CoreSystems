package models

import "github.com/triodelab/quarry/internal/security"

type ScrapeAPIResponse struct {
	Success        bool                    `json:"success"`
	Data           *ScrapeResult           `json:"data,omitempty"`
	Outputs        map[string]interface{}  `json:"outputs,omitempty"`
	Actions        []ActionExecutionResult `json:"actions,omitempty"`
	Security       *security.Assessment    `json:"security,omitempty"`
	ChangeTracking *ChangeTrackingResult   `json:"changeTracking,omitempty"`
	Error          string                  `json:"error,omitempty"`
}

type ActionExecutionResult struct {
	Type       string      `json:"type"`
	Success    bool        `json:"success"`
	DurationMs int64       `json:"durationMs"`
	Error      string      `json:"error,omitempty"`
	Output     interface{} `json:"output,omitempty"`
}

type CrawlDispatch struct {
	Mode       string `json:"mode"`
	WorkflowID string `json:"workflowId,omitempty"`
	RunID      string `json:"runId,omitempty"`
}

type CrawlAPIResponse struct {
	Success  bool           `json:"success"`
	Job      *JobSummary    `json:"job,omitempty"`
	Dispatch *CrawlDispatch `json:"dispatch,omitempty"`
	Data     *ScrapeResult  `json:"data,omitempty"`
	Error    string         `json:"error,omitempty"`
}

type JobStatusResponse struct {
	Success bool        `json:"success"`
	Job     *JobSummary `json:"job,omitempty"`
	Result  any         `json:"result,omitempty"`
	Error   string      `json:"error,omitempty"`
}
