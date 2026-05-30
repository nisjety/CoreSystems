package models

import "time"

// ChangeTrackingRequest represents options for change tracking
type ChangeTrackingRequest struct {
	Enabled bool                   `json:"enabled"`
	Modes   []string               `json:"modes,omitempty"`  // "git-diff", "json"
	Schema  map[string]interface{} `json:"schema,omitempty"` // Schema for JSON mode comparison
	Prompt  string                 `json:"prompt,omitempty"` // Custom extraction prompt for JSON mode
	Tag     string                 `json:"tag,omitempty"`    // Tag for separate tracking histories
	DryRun  bool                   `json:"dryRun,omitempty"` // Compare without persisting a new snapshot
}

// ChangeTrackingResult represents the result of change tracking
type ChangeTrackingResult struct {
	PreviousScrapeAt *time.Time      `json:"previousScrapeAt"` // When the previous scrape happened
	ChangeStatus     string          `json:"changeStatus"`     // "new", "same", "changed", "removed"
	Visibility       string          `json:"visibility"`       // "visible", "hidden"
	Diff             *DiffResult     `json:"diff,omitempty"`   // Git-diff mode results
	JSON             map[string]any  `json:"json,omitempty"`   // JSON mode field comparison
	AIAnalysis       *DiffAIAnalysis `json:"aiAnalysis,omitempty"` // AI-generated diff interpretation
}

// DiffAIAnalysis holds the AI-generated analysis of a detected diff.
type DiffAIAnalysis struct {
	Summary      string           `json:"summary"`
	FieldChanges []DiffFieldChange `json:"fieldChanges,omitempty"`
}

// DiffFieldChange describes a single field-level change detected by the AI.
type DiffFieldChange struct {
	FieldPath  string `json:"fieldPath"`
	ChangeType string `json:"changeType"` // "added", "removed", "modified"
	OldValue   string `json:"oldValue,omitempty"`
	NewValue   string `json:"newValue,omitempty"`
}

// DiffResult represents git-style diff output
type DiffResult struct {
	Text string          `json:"text"` // Human-readable diff text
	JSON *StructuredDiff `json:"json"` // Structured diff data
}

// StructuredDiff represents structured diff information
type StructuredDiff struct {
	Files []DiffFile `json:"files"`
}

// DiffFile represents changes in a single file/content
type DiffFile struct {
	From   *string     `json:"from"`   // Previous version identifier
	To     *string     `json:"to"`     // Current version identifier
	Chunks []DiffChunk `json:"chunks"` // Change chunks
}

// DiffChunk represents a section of changes
type DiffChunk struct {
	Content string       `json:"content"` // Chunk header
	Changes []DiffChange `json:"changes"` // Individual line changes
}

// DiffChange represents a single line change
type DiffChange struct {
	Type    string `json:"type"`             // "add", "delete", "normal"
	Normal  bool   `json:"normal,omitempty"` // True if unchanged line
	Ln      *int   `json:"ln,omitempty"`     // Line number (for normal lines)
	Ln1     *int   `json:"ln1,omitempty"`    // Line number in old version
	Ln2     *int   `json:"ln2,omitempty"`    // Line number in new version
	Content string `json:"content"`          // Line content
}

// FieldComparison represents before/after values for a field
type FieldComparison struct {
	Previous any `json:"previous"`
	Current  any `json:"current"`
}

// StoredScrape represents a previously scraped page stored for comparison
type StoredScrape struct {
	URL       string    `json:"url"`
	Content   string    `json:"content"` // Markdown content
	Timestamp time.Time `json:"timestamp"`
	Tag       string    `json:"tag"`  // Optional tag for separate histories
	Hash      string    `json:"hash"` // Content hash for quick comparison
}
