package jobs

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

// §17.3.3 — named NATS subjects. See infra/nats/SUBJECTS.md.
const (
	SubjectDocCreated  = "dataplane.documents.created"
	SubjectDocsIndexed = "dataplane.documents.indexed"
)

type Executor struct {
	store     JobStore
	publisher EventPublisher
	auditor   JobAuditor
	available bool
}

var ErrExecutionUnavailable = errors.New("durable signed job execution is unavailable")

func NewExecutor(pool *pgxpool.Pool, nc *nats.Conn, allowUnverifiedLegacyEvents bool) *Executor {
	var publisher EventPublisher = disabledEventPublisher{}
	if allowUnverifiedLegacyEvents && nc != nil {
		publisher = nc
	}
	return &Executor{
		store:     NewPostgresJobStore(pool),
		publisher: publisher,
		auditor:   &PostgresJobAuditor{pool: pool},
		available: allowUnverifiedLegacyEvents && nc != nil,
	}
}

type disabledEventPublisher struct{}

func (disabledEventPublisher) Publish(string, []byte) error {
	return errors.New("unsigned asynchronous job events are disabled")
}

type EventPublisher interface {
	Publish(string, []byte) error
}

type JobAuditor interface {
	RecordCreated(context.Context, model.Job) error
}

type PostgresJobAuditor struct {
	pool *pgxpool.Pool
}

// RecordCreated writes the job-creation audit row.
//
// Phase 1 RLS: a single-org write on a request path — job.OrgID originates in
// the HTTP handler's verified caller claims. Its caller (CreateJob) invokes
// this AFTER store.Create has committed and closed its own scope, so the two
// scopes are sequential rather than nested.
func (a *PostgresJobAuditor) RecordCreated(ctx context.Context, job model.Job) error {
	details, err := json.Marshal(map[string]any{
		"document_count": len(job.DocumentIDs),
		"job_type":       job.JobType,
	})
	if err != nil {
		return err
	}
	return orgscope.WithOrgScope(ctx, a.pool, job.OrgID, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
			INSERT INTO data_plane_audit_log
				(user_id, org_id, action, resource_type, resource_id, details)
			VALUES ('system', $1, 'job_created', 'job', $2, $3::jsonb)
		`, job.OrgID, job.JobID, details)
		return err
	})
}

func NewExecutorWithDependencies(store JobStore, publisher EventPublisher) *Executor {
	_, disabled := publisher.(disabledEventPublisher)
	return &Executor{store: store, publisher: publisher, available: !disabled}
}

func (e *Executor) CreateJob(ctx context.Context, input model.CreateJobInput, idempotencyKey string) (*model.Job, bool, error) {
	if e == nil || !e.available {
		return nil, false, ErrExecutionUnavailable
	}
	if input.OrgID == "" || idempotencyKey == "" {
		return nil, false, errors.New("organization and idempotency key are required")
	}
	switch input.JobType {
	case model.JobReindex, model.JobGraphBuild, model.JobWikiRefresh:
	default:
		return nil, false, fmt.Errorf("unsupported job type: %s", input.JobType)
	}
	now := time.Now().UTC()
	job := model.Job{
		JobID:          uuid.NewString(),
		OrgID:          input.OrgID,
		JobType:        input.JobType,
		Status:         model.StatusPending,
		DocumentIDs:    append([]string(nil), input.DocumentIDs...),
		Progress:       0,
		Total:          len(input.DocumentIDs),
		IdempotencyKey: idempotencyKey,
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	persisted, created, err := e.store.Create(ctx, job)
	if err != nil {
		return nil, false, err
	}
	if created && e.auditor != nil {
		if err := e.auditor.RecordCreated(ctx, *persisted); err != nil {
			log.Warn().Err(err).Str("job_id", persisted.JobID).Msg("audit log insert failed")
		}
	}
	return persisted, created, nil
}

func (e *Executor) GetJob(ctx context.Context, orgID, jobID string) (*model.Job, error) {
	return e.store.Get(ctx, orgID, jobID)
}

func (e *Executor) Run(ctx context.Context, job model.Job) error {
	running, err := e.store.Start(ctx, job.OrgID, job.JobID)
	if err != nil {
		return err
	}

	var result json.RawMessage
	switch running.JobType {
	case model.JobReindex:
		result, err = e.publishDocuments(ctx, *running, SubjectDocCreated)
	case model.JobGraphBuild:
		result, err = e.publishDocuments(ctx, *running, SubjectDocsIndexed)
	case model.JobWikiRefresh:
		result, err = e.refreshWiki(ctx, *running)
	default:
		err = fmt.Errorf("unsupported persisted job type: %s", running.JobType)
	}
	if err != nil {
		if _, persistErr := e.store.Fail(ctx, running.OrgID, running.JobID, err.Error()); persistErr != nil {
			return errors.Join(err, fmt.Errorf("persist job failure: %w", persistErr))
		}
		return err
	}
	if _, err := e.store.Complete(ctx, running.OrgID, running.JobID, result); err != nil {
		return err
	}
	return nil
}

func (e *Executor) publishDocuments(ctx context.Context, job model.Job, subject string) (json.RawMessage, error) {
	for i, documentID := range job.DocumentIDs {
		event, err := json.Marshal(map[string]string{
			"document_id": documentID,
			"org_id":      job.OrgID,
			"job_id":      job.JobID,
		})
		if err != nil {
			return nil, fmt.Errorf("encode job event: %w", err)
		}
		if err := e.publisher.Publish(subject, event); err != nil {
			return nil, fmt.Errorf("publish job event: %w", err)
		}
		if _, err := e.store.SetProgress(ctx, job.OrgID, job.JobID, i+1); err != nil {
			return nil, err
		}
	}
	result, err := json.Marshal(map[string]any{"published": len(job.DocumentIDs)})
	if err != nil {
		return nil, err
	}
	return result, nil
}

func (e *Executor) refreshWiki(ctx context.Context, job model.Job) (json.RawMessage, error) {
	dataQualityURL := getenvDefault("DATA_QUALITY_URL", "http://data-quality:8013")
	wikiURL := getenvDefault("WIKI_STORE_URL", "http://wiki-store:8011")
	items, err := fetchLintItems(ctx, dataQualityURL, job.OrgID)
	if err != nil {
		return nil, fmt.Errorf("fetch lint: %w", err)
	}

	accepted, rejected := 0, 0
	if len(items) > 0 {
		accepted, rejected, err = postSweep(ctx, wikiURL, job.OrgID, items)
		if err != nil {
			return nil, fmt.Errorf("post sweep: %w", err)
		}
	}
	result, err := json.Marshal(map[string]int{
		"accepted": accepted,
		"rejected": rejected,
	})
	if err != nil {
		return nil, err
	}
	return result, nil
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
	if err := applyVerifiedIdentity(ctx, req, orgID); err != nil {
		return nil, err
	}

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
	//
	// "contradiction" is deliberately NOT here. It used to be, for a kind the
	// linter never emitted — dead wiring. Now that data-quality does emit it
	// (plan P1-9, once graph-index started populating
	// `graph_claims.contradicted_by_claim_ids` in P1-4), forwarding it would be
	// actively wrong: below we map `Issue.ID` onto `sweepItem.PageID`, but
	// data-quality's contradiction findings are **claim-scoped**, so the ID is a
	// claim_id. `/v1/wiki/maintenance/sweep` does not verify the page exists, so
	// each one would be written into `wiki_maintenance_logs` as a maintenance
	// record against a page that never existed.
	//
	// wiki-store still accepts "contradiction" in its own allowed-kinds set, and
	// that stays correct: a *wiki-page-level* contradiction from a Model Plane
	// wiki-maintenance agent is a legitimate producer for that endpoint. This
	// linter is simply not that producer.
	wikiKinds := map[string]bool{
		"orphan_wiki":   true,
		"stale_wiki":    true,
		"weak_citation": true,
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
	if err := applyVerifiedIdentity(ctx, req, orgID); err != nil {
		return 0, 0, err
	}

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

// applyVerifiedIdentity forwards the already-verified, same-audience bearer
// for immediate internal callbacks. The requested org must equal the signed
// claim; the credential never enters job state, logs, storage, or events.
func applyVerifiedIdentity(ctx context.Context, req *http.Request, orgID string) error {
	claims, ok := authctx.FromContext(ctx)
	if !ok {
		return errors.New("verified callback identity missing")
	}
	if strings.TrimSpace(orgID) == "" || orgID != claims.OrgID {
		return errors.New("callback tenant does not match verified identity")
	}
	authorization, ok := authctx.AuthorizationHeader(ctx)
	if !ok {
		return errors.New("verified callback bearer missing")
	}
	req.Header.Set("Authorization", authorization)
	req.Header.Set("X-Org-ID", claims.OrgID)
	return nil
}

// 30s upper bound covers a 1000-item sweep on a slow link; the underlying
// wiki-store endpoint is bounded to 1000 items per call anyway.
var httpClient = &http.Client{Timeout: 30 * time.Second}
