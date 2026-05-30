package jobs

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

// §17.3.3 — named NATS subjects. See infra/nats/SUBJECTS.md.
const (
	SubjectDocCreated  = "dataplane.documents.created"
	SubjectDocsIndexed = "dataplane.documents.indexed"
)

type Executor struct {
	pool *pgxpool.Pool
	nc   *nats.Conn
}

func NewExecutor(pool *pgxpool.Pool, nc *nats.Conn) *Executor {
	return &Executor{pool: pool, nc: nc}
}

func (e *Executor) CreateJob(ctx context.Context, input model.CreateJobInput) (*model.Job, error) {
	id := uuid.New().String()
	now := time.Now()

	docIDs, _ := json.Marshal(input.DocumentIDs)

	_, err := e.pool.Exec(ctx, `
		INSERT INTO data_plane_audit_log (user_id, org_id, action, resource_type, resource_id, details)
		VALUES ('system', $1, 'job_created', 'job', $2, $3)
	`, input.OrgID, id, string(docIDs))
	if err != nil {
		log.Warn().Err(err).Msg("audit log insert failed")
	}

	job := &model.Job{
		JobID:       id,
		OrgID:       input.OrgID,
		JobType:     input.JobType,
		Status:      model.StatusPending,
		DocumentIDs: input.DocumentIDs,
		Progress:    0,
		Total:       len(input.DocumentIDs),
		CreatedAt:   now,
	}

	return job, nil
}

func (e *Executor) ExecuteReindex(ctx context.Context, job *model.Job) error {
	now := time.Now()
	job.Status = model.StatusRunning
	job.StartedAt = &now

	for i, docID := range job.DocumentIDs {
		evt := map[string]string{
			"document_id": docID,
			"org_id":      job.OrgID,
			"job_id":      job.JobID,
		}
		data, _ := json.Marshal(evt)
		if err := e.nc.Publish(SubjectDocCreated, data); err != nil {
			return fmt.Errorf("publish reindex event for %s: %w", docID, err)
		}
		job.Progress = i + 1
	}

	done := time.Now()
	job.Status = model.StatusCompleted
	job.CompletedAt = &done

	log.Info().
		Str("job_id", job.JobID).
		Str("org_id", job.OrgID).
		Int("documents", len(job.DocumentIDs)).
		Msg("reindex job completed")

	return nil
}

func (e *Executor) ExecuteGraphBuild(ctx context.Context, job *model.Job) error {
	now := time.Now()
	job.Status = model.StatusRunning
	job.StartedAt = &now

	for i, docID := range job.DocumentIDs {
		evt := map[string]string{
			"document_id": docID,
			"org_id":      job.OrgID,
		}
		data, _ := json.Marshal(evt)
		if err := e.nc.Publish(SubjectDocsIndexed, data); err != nil {
			return fmt.Errorf("publish graph build event for %s: %w", docID, err)
		}
		job.Progress = i + 1
	}

	done := time.Now()
	job.Status = model.StatusCompleted
	job.CompletedAt = &done

	log.Info().
		Str("job_id", job.JobID).
		Int("documents", len(job.DocumentIDs)).
		Msg("graph build job completed")

	return nil
}

func (e *Executor) ExecuteWikiRefresh(ctx context.Context, job *model.Job) error {
	now := time.Now()
	job.Status = model.StatusRunning
	job.StartedAt = &now

	log.Info().
		Str("job_id", job.JobID).
		Str("org_id", job.OrgID).
		Msg("wiki refresh — calling lint → maintenance/sweep pipeline")

	// Wave-2 D5-6 close: read lint findings from data-quality-go and POST
	// them to wiki-store's /v1/wiki/maintenance/sweep. Both URLs are
	// container-internal DNS; configurable via env for cross-cluster runs.
	dqURL := getenvDefault("DATA_QUALITY_URL", "http://data-quality:8013")
	wikiURL := getenvDefault("WIKI_STORE_URL", "http://wiki-store:8011")

	items, err := fetchLintItems(ctx, dqURL, job.OrgID)
	if err != nil {
		failJob(job, fmt.Errorf("fetch lint: %w", err))
		return err
	}
	if len(items) == 0 {
		log.Info().Str("job_id", job.JobID).Msg("no lint findings; sweep skipped")
		done := time.Now()
		job.Status = model.StatusCompleted
		job.CompletedAt = &done
		return nil
	}

	accepted, rejected, err := postSweep(ctx, wikiURL, job.OrgID, items)
	if err != nil {
		failJob(job, fmt.Errorf("post sweep: %w", err))
		return err
	}

	log.Info().
		Str("job_id", job.JobID).
		Int("accepted", accepted).
		Int("rejected", rejected).
		Msg("wiki maintenance sweep complete")

	done := time.Now()
	job.Status = model.StatusCompleted
	job.CompletedAt = &done
	return nil
}

// failJob marks a job as failed with the error message, preserving its
// original StartedAt timestamp.
func failJob(job *model.Job, err error) {
	now := time.Now()
	msg := err.Error()
	job.Status = model.StatusFailed
	job.ErrorMessage = &msg
	job.CompletedAt = &now
}

func getenvDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// lintItem mirrors the relevant fields from data-quality-go's /v1/quality/lint
// response so we can shovel them straight into the wiki sweep endpoint.
type lintItem struct {
	Kind  string `json:"kind"`
	ID    string `json:"id"`
	Title string `json:"title,omitempty"`
}

type sweepItem struct {
	PageID  string          `json:"page_id"`
	Kind    string          `json:"kind"`
	Actor   string          `json:"actor"`
	Details json.RawMessage `json:"details,omitempty"`
}

func fetchLintItems(ctx context.Context, baseURL, orgID string) ([]sweepItem, error) {
	url := strings.TrimRight(baseURL, "/") + "/v1/quality/lint"
	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("X-Org-ID", orgID)

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("lint endpoint returned %d: %s", resp.StatusCode, string(body))
	}

	var lintResp struct {
		Issues []lintItem `json:"issues"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&lintResp); err != nil {
		return nil, fmt.Errorf("decode lint response: %w", err)
	}

	// Only wiki-relevant kinds are forwarded to the sweep endpoint; doc-side
	// findings (orphan_doc, stale_doc) belong to the documents-api lifecycle.
	wikiKinds := map[string]bool{
		"orphan_wiki":   true,
		"stale_wiki":    true,
		"weak_citation": true,
		"contradiction": true,
	}
	items := make([]sweepItem, 0, len(lintResp.Issues))
	for _, it := range lintResp.Issues {
		if !wikiKinds[it.Kind] {
			continue
		}
		items = append(items, sweepItem{
			PageID: it.ID,
			Kind:   it.Kind,
			Actor:  "data-orchestrator-lint-sweep",
		})
	}
	return items, nil
}

func postSweep(ctx context.Context, baseURL, orgID string, items []sweepItem) (int, int, error) {
	url := strings.TrimRight(baseURL, "/") + "/v1/wiki/maintenance/sweep"
	body, err := json.Marshal(map[string]any{"items": items})
	if err != nil {
		return 0, 0, err
	}
	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(body))
	if err != nil {
		return 0, 0, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Org-ID", orgID)

	resp, err := httpClient.Do(req)
	if err != nil {
		return 0, 0, err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		b, _ := io.ReadAll(resp.Body)
		return 0, 0, fmt.Errorf("sweep returned %d: %s", resp.StatusCode, string(b))
	}

	var sweepResp struct {
		Accepted int `json:"accepted"`
		Rejected int `json:"rejected"`
	}
	_ = json.NewDecoder(resp.Body).Decode(&sweepResp)
	return sweepResp.Accepted, sweepResp.Rejected, nil
}

// 30s upper bound covers a 1000-item sweep on a slow link; the underlying
// wiki-store endpoint is bounded to 1000 items per call anyway.
var httpClient = &http.Client{Timeout: 30 * time.Second}

func (e *Executor) Run(ctx context.Context, job *model.Job) error {
	switch job.JobType {
	case model.JobReindex:
		return e.ExecuteReindex(ctx, job)
	case model.JobGraphBuild:
		return e.ExecuteGraphBuild(ctx, job)
	case model.JobWikiRefresh:
		return e.ExecuteWikiRefresh(ctx, job)
	default:
		return fmt.Errorf("unknown job type: %s", job.JobType)
	}
}
