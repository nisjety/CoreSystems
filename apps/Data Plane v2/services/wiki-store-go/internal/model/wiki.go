package model

import (
	"encoding/json"
	"time"
)

type WikiPage struct {
	PageID           string          `json:"page_id"`
	OrgID            string          `json:"org_id"`
	WorkspaceID      string          `json:"workspace_id"`
	Title            string          `json:"title"`
	Path             string          `json:"path"`
	CurrentVersionID *string         `json:"current_version_id,omitempty"`
	Status           string          `json:"status"`
	Backlinks        json.RawMessage `json:"backlinks,omitempty"`
	Metadata         json.RawMessage `json:"metadata,omitempty"`
	CreatedAt        time.Time       `json:"created_at"`
	UpdatedAt        time.Time       `json:"updated_at"`
}

type WikiPageVersion struct {
	VersionID string  `json:"version_id"`
	PageID    string  `json:"page_id"`
	Content   *string `json:"content,omitempty"`
	// §16.5.4 — `SafeHTML` is set by the handler after server-side scrub
	// (bluemonday UGC policy). Consumers SHOULD render `SafeHTML` for
	// trusted-render contexts (Verevon) and treat `Content` as raw markdown.
	// `true` only if the scrub passed without flagging suspicious markup.
	SafeHTML        *string         `json:"safe_html,omitempty"`
	SafeHTMLOK      bool            `json:"safe_html_ok"`
	SourceRefs      json.RawMessage `json:"source_refs,omitempty"`
	ProposedByAgent *string         `json:"proposed_by_agent,omitempty"`
	ProposedByUser  *string         `json:"proposed_by_user,omitempty"`
	ApprovedBy      *string         `json:"approved_by,omitempty"`
	EditReason      *string         `json:"edit_reason,omitempty"`
	VersionStatus   string          `json:"version_status"`
	Metadata        json.RawMessage `json:"metadata,omitempty"`
	CreatedAt       time.Time       `json:"created_at"`
	PublishedAt     *time.Time      `json:"published_at,omitempty"`
}

type WikiProposal struct {
	ProposalID      string          `json:"proposal_id"`
	PageID          string          `json:"page_id"`
	OrgID           string          `json:"org_id"`
	ProposedContent string          `json:"proposed_content"`
	EditReason      *string         `json:"edit_reason,omitempty"`
	ProposedByAgent *string         `json:"proposed_by_agent,omitempty"`
	SourceRefs      json.RawMessage `json:"source_refs,omitempty"`
	ProposalStatus  string          `json:"proposal_status"`
	ReviewedBy      *string         `json:"reviewed_by,omitempty"`
	Metadata        json.RawMessage `json:"metadata,omitempty"`
	CreatedAt       time.Time       `json:"created_at"`
}

type CreatePageInput struct {
	OrgID          string `json:"org_id"`
	WorkspaceID    string `json:"workspace_id"`
	Title          string `json:"title"`
	Path           string `json:"path"`
	InitialContent string `json:"initial_content"`
}

type UpdateVersionInput struct {
	PageID     string  `json:"page_id"`
	OrgID      string  `json:"org_id"`
	NewContent string  `json:"new_content"`
	EditReason string  `json:"edit_reason"`
	ProposedBy *string `json:"proposed_by_user,omitempty"`
}

type SubmitProposalInput struct {
	PageID          string   `json:"page_id"`
	OrgID           string   `json:"org_id"`
	ProposedContent string   `json:"proposed_content"`
	EditReason      string   `json:"edit_reason"`
	ProposedByAgent string   `json:"proposed_by_agent"`
	SourceRefs      []string `json:"source_refs,omitempty"`
}

type ReviewProposalInput struct {
	ProposalID string `json:"proposal_id"`
	OrgID      string `json:"org_id"`
	Decision   string `json:"decision"`
	ReviewedBy string `json:"reviewed_by"`
}

type SourceLog struct {
	LogID      string          `json:"log_id"`
	OrgID      string          `json:"org_id"`
	PageID     string          `json:"page_id"`
	SourceType string          `json:"source_type"`
	SourceRef  string          `json:"source_ref"`
	SyncStatus string          `json:"sync_status"`
	Details    json.RawMessage `json:"details,omitempty"`
	CreatedAt  time.Time       `json:"created_at"`
}

type MaintenanceLog struct {
	LogID     string          `json:"log_id"`
	OrgID     string          `json:"org_id"`
	PageID    string          `json:"page_id"`
	Action    string          `json:"action"`
	Actor     string          `json:"actor"`
	Details   json.RawMessage `json:"details,omitempty"`
	CreatedAt time.Time       `json:"created_at"`
}

type CreateSourceLogInput struct {
	OrgID      string          `json:"org_id"`
	PageID     string          `json:"page_id"`
	SourceType string          `json:"source_type"`
	SourceRef  string          `json:"source_ref"`
	SyncStatus string          `json:"sync_status"`
	Details    json.RawMessage `json:"details,omitempty"`
}

type CreateMaintenanceLogInput struct {
	OrgID   string          `json:"org_id"`
	PageID  string          `json:"page_id"`
	Action  string          `json:"action"`
	Actor   string          `json:"actor"`
	Details json.RawMessage `json:"details,omitempty"`
}

type OperatingMap struct {
	MapID            string          `json:"operating_map_id"`
	OrgID            string          `json:"org_id"`
	Status           string          `json:"status"`
	CurrentVersionID *string         `json:"current_version_id,omitempty"`
	GeneratedFrom    json.RawMessage `json:"generated_from,omitempty"`
	CreatedAt        time.Time       `json:"created_at"`
	UpdatedAt        time.Time       `json:"updated_at"`
}

type OperatingMapVersion struct {
	VersionID       string          `json:"version_id"`
	MapID           string          `json:"operating_map_id"`
	OrgID           string          `json:"org_id"`
	Departments     json.RawMessage `json:"departments"`
	Workflows       json.RawMessage `json:"workflows"`
	AgentBlueprints json.RawMessage `json:"agent_blueprints"`
	RolloutPhases   json.RawMessage `json:"rollout_phases"`
	RiskOverlays    json.RawMessage `json:"risk_overlays"`
	LearningModules json.RawMessage `json:"learning_modules"`
	ROINotes        json.RawMessage `json:"roi_notes"`
	EvidenceRefs    json.RawMessage `json:"evidence_refs"`
	Confidence      float64         `json:"confidence"`
	CreatedByRunID  *string         `json:"created_by_run_id,omitempty"`
	CreatedAt       time.Time       `json:"created_at"`
}

type OperatingMapProposal struct {
	ProposalID       string          `json:"proposal_id"`
	MapID            string          `json:"operating_map_id"`
	OrgID            string          `json:"org_id"`
	ProposedVersion  json.RawMessage `json:"proposed_version"`
	EvidenceRefs     json.RawMessage `json:"evidence_refs"`
	GeneratedByRunID *string         `json:"generated_by_run_id,omitempty"`
	ProposalStatus   string          `json:"proposal_status"`
	ReviewedBy       *string         `json:"reviewed_by,omitempty"`
	CreatedAt        time.Time       `json:"created_at"`
	ReviewedAt       *time.Time      `json:"reviewed_at,omitempty"`
}

type OperatingMapBlueprintSuggestion struct {
	SuggestionID     string          `json:"suggestion_id"`
	MapID            string          `json:"operating_map_id"`
	VersionID        string          `json:"version_id"`
	OrgID            string          `json:"org_id"`
	BlueprintID      string          `json:"blueprint_id"`
	Role             string          `json:"role"`
	SourceWorkflowID *string         `json:"source_workflow_id,omitempty"`
	Name             string          `json:"name"`
	SuggestionStatus string          `json:"suggestion_status"`
	RequestedBy      *string         `json:"requested_by,omitempty"`
	Payload          json.RawMessage `json:"payload,omitempty"`
	CreatedAt        time.Time       `json:"created_at"`
	UpdatedAt        time.Time       `json:"updated_at"`
}

type OperatingMapSnapshot struct {
	Map                  *OperatingMap                     `json:"map"`
	Version              *OperatingMapVersion              `json:"current_version,omitempty"`
	Proposals            []OperatingMapProposal            `json:"proposals"`
	BlueprintSuggestions []OperatingMapBlueprintSuggestion `json:"blueprint_suggestions"`
}

type SubmitOperatingMapProposalInput struct {
	OrgID            string          `json:"org_id"`
	GeneratedFrom    json.RawMessage `json:"generated_from,omitempty"`
	ProposedVersion  json.RawMessage `json:"proposed_version"`
	EvidenceRefs     []string        `json:"evidence_refs,omitempty"`
	GeneratedByRunID *string         `json:"generated_by_run_id,omitempty"`
}

type RefreshOperatingMapInput struct {
	OrgID         string          `json:"org_id"`
	GeneratedFrom json.RawMessage `json:"generated_from,omitempty"`
	RequestedBy   string          `json:"requested_by,omitempty"`
}

type ReviewOperatingMapProposalInput struct {
	ProposalID string `json:"proposal_id"`
	OrgID      string `json:"org_id"`
	Decision   string `json:"decision"`
	ReviewedBy string `json:"reviewed_by"`
}

type CreateOperatingMapBlueprintSuggestionInput struct {
	OrgID            string          `json:"org_id"`
	MapID            string          `json:"operating_map_id,omitempty"`
	VersionID        string          `json:"version_id"`
	BlueprintID      string          `json:"blueprint_id"`
	Role             string          `json:"role"`
	SourceWorkflowID string          `json:"source_workflow_id,omitempty"`
	Name             string          `json:"name"`
	RequestedBy      string          `json:"requested_by,omitempty"`
	Payload          json.RawMessage `json:"payload,omitempty"`
}
