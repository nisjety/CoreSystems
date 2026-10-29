package model

import (
	"encoding/json"
	"time"
)

type JobType string

const (
	JobReindex     JobType = "reindex"
	JobGraphBuild  JobType = "graph_build"
	JobWikiRefresh JobType = "wiki_refresh"
)

type JobStatus string

const (
	StatusPending   JobStatus = "pending"
	StatusRunning   JobStatus = "running"
	StatusCompleted JobStatus = "completed"
	StatusFailed    JobStatus = "failed"
)

type Job struct {
	JobID          string          `json:"job_id"`
	OrgID          string          `json:"org_id"`
	JobType        JobType         `json:"job_type"`
	Status         JobStatus       `json:"status"`
	DocumentIDs    []string        `json:"document_ids,omitempty"`
	Result         json.RawMessage `json:"result,omitempty"`
	ErrorMessage   *string         `json:"error_message,omitempty"`
	Progress       int             `json:"progress"`
	Total          int             `json:"total"`
	IdempotencyKey string          `json:"-"`
	CreatedAt      time.Time       `json:"created_at"`
	StartedAt      *time.Time      `json:"started_at,omitempty"`
	CompletedAt    *time.Time      `json:"completed_at,omitempty"`
	UpdatedAt      time.Time       `json:"updated_at"`
}

type CreateJobInput struct {
	OrgID       string   `json:"org_id"`
	JobType     JobType  `json:"job_type"`
	DocumentIDs []string `json:"document_ids,omitempty"`
}
