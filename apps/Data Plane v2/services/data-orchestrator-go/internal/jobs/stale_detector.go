package jobs

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// defaultReconcileMaxAttempts mirrors index-engine-rs's
// reconcile::ReconcileConfig default. D19 put the actual re-drive there (it is
// the only service holding the `index-events-v1` signing key that
// embedding-engine's issuer-pinned verifier accepts for
// `dataplane.knowledge.units.created`); this service reports on it, so the two
// have to agree on where the ceiling is or the report lies about which units
// are still going to be retried.
const defaultReconcileMaxAttempts = 5

func reconcileMaxAttempts() int {
	raw := os.Getenv("EMBEDDING_RECONCILE_MAX_ATTEMPTS")
	if raw == "" {
		return defaultReconcileMaxAttempts
	}
	parsed, err := strconv.Atoi(raw)
	if err != nil || parsed < 0 {
		return defaultReconcileMaxAttempts
	}
	return parsed
}

type StaleEmbeddingReport struct {
	OrgID             string `json:"org_id"`
	StaleCount        int    `json:"stale_count"`
	StuckPendingCount int    `json:"stuck_pending_count"`
	FailedCount       int    `json:"failed_count"`
	// D19: `failed` is no longer one undifferentiated bucket. A unit under the
	// reconciler's ceiling will be re-driven automatically and needs nobody;
	// a unit past it never will and is the only kind worth paging a human
	// about. Reporting one number for both was part of why 51 chunks sat
	// `failed` on a healthy pipeline with nothing happening.
	RetryableFailedCount int `json:"retryable_failed_count"`
	ExhaustedFailedCount int `json:"exhausted_failed_count"`

	StaleDocumentIDs           []string           `json:"stale_document_ids,omitempty"`
	StuckDocumentIDs           []string           `json:"stuck_document_ids,omitempty"`
	FailedDocumentIDs          []string           `json:"failed_document_ids,omitempty"`
	ExhaustedFailedDocumentIDs []string           `json:"exhausted_failed_document_ids,omitempty"`
	CheckedAt                  time.Time          `json:"checked_at"`
	Details                    []StaleEmbedDetail `json:"details,omitempty"`
}

type StaleEmbedDetail struct {
	DocumentID       string     `json:"document_id"`
	KnowledgeID      string     `json:"knowledge_id"`
	EmbeddingStatus  string     `json:"embedding_status"`
	ContentUpdatedAt time.Time  `json:"content_updated_at"`
	EmbeddedAt       *time.Time `json:"embedded_at,omitempty"`
	Reason           string     `json:"reason"`
}

type StaleDetector struct {
	pool *pgxpool.Pool
}

func NewStaleDetector(pool *pgxpool.Pool) *StaleDetector {
	return &StaleDetector{pool: pool}
}

// collectTolerated runs a best-effort id lookup on a SAVEPOINT.
//
// The two lookups that use it deliberately swallow their errors — the report
// still carries its counts without them. On the unscoped pool that was free;
// inside one scoped transaction a failure would abort the tx and take every
// later statement, including the COMMIT, with it, so the whole report would 500
// instead of degrading. A SAVEPOINT (pgx models one as a nested Begin) keeps
// the original tolerance, and returning whatever was collected before a
// mid-iteration error matches the previous behaviour exactly.
func collectTolerated(ctx context.Context, tx pgx.Tx, sql string, args ...any) []string {
	sp, err := tx.Begin(ctx)
	if err != nil {
		return nil
	}
	rows, err := sp.Query(ctx, sql, args...)
	if err != nil {
		_ = sp.Rollback(ctx)
		return nil
	}
	var ids []string
	for rows.Next() {
		var id string
		if rows.Scan(&id) == nil {
			ids = append(ids, id)
		}
	}
	rows.Close()
	if rows.Err() != nil {
		// Roll the savepoint back so the OUTER transaction stays usable.
		_ = sp.Rollback(ctx)
		return ids
	}
	_ = sp.Commit(ctx)
	return ids
}

// Detect reports embedding staleness for exactly one organization.
//
// Phase 1 RLS: despite living next to the cross-org job queue, this is NOT a
// sweep — its only caller is the GET /v1/orchestrator/stale-embeddings handler,
// which takes orgID from verified caller claims (authctx), and every query
// already filters `org_id = $1`. All five queries share one scoped transaction
// so the counts, the id lists and the details cannot disagree with each other
// under a concurrent write, and so a five-query report costs one pool checkout.
func (d *StaleDetector) Detect(ctx context.Context, orgID string, stuckThreshold time.Duration) (*StaleEmbeddingReport, error) {
	if stuckThreshold == 0 {
		stuckThreshold = 30 * time.Minute
	}

	report := &StaleEmbeddingReport{
		OrgID:     orgID,
		CheckedAt: time.Now(),
	}

	if err := orgscope.WithOrgScope(ctx, d.pool, orgID, func(tx pgx.Tx) error {
		return d.detectInScope(ctx, tx, orgID, stuckThreshold, report)
	}); err != nil {
		return nil, err
	}

	log.Info().
		Str("org_id", orgID).
		Int("stale", report.StaleCount).
		Int("stuck_pending", report.StuckPendingCount).
		Int("failed", report.FailedCount).
		Int("retryable_failed", report.RetryableFailedCount).
		Int("exhausted_failed", report.ExhaustedFailedCount).
		Msg("stale embedding detection complete")

	return report, nil
}

// detectInScope holds every query in Detect. It takes the tx rather than
// reading d.pool so it cannot be called outside a scope — a compile error is a
// stronger guarantee than a comment for a function this long.
func (d *StaleDetector) detectInScope(
	ctx context.Context,
	tx pgx.Tx,
	orgID string,
	stuckThreshold time.Duration,
	report *StaleEmbeddingReport,
) error {
	staleRows, err := tx.Query(ctx, `
		SELECT DISTINCT ku.document_id, ku.knowledge_id, ku.embedding_status, ku.updated_at, ku.embedded_at
		FROM knowledge_units ku
		JOIN documents d ON d.document_id = ku.document_id
		WHERE ku.org_id = $1
		  AND ku.embedded_at IS NOT NULL
		  AND ku.updated_at > ku.embedded_at
		  AND d.deleted_at IS NULL
		ORDER BY ku.updated_at DESC
		LIMIT 500
	`, orgID)
	if err != nil {
		return fmt.Errorf("detect stale embeddings: %w", err)
	}
	defer staleRows.Close()

	// Drained before the next query: the scope requires every pgx.Rows to be
	// consumed inside the callback, and pgx forbids a second query on the same
	// transaction while these rows are still open.
	staleDocSet := make(map[string]bool)
	for staleRows.Next() {
		var detail StaleEmbedDetail
		if err := staleRows.Scan(&detail.DocumentID, &detail.KnowledgeID, &detail.EmbeddingStatus, &detail.ContentUpdatedAt, &detail.EmbeddedAt); err != nil {
			continue
		}
		detail.Reason = "content_updated_after_embedding"
		report.Details = append(report.Details, detail)
		staleDocSet[detail.DocumentID] = true
	}
	staleRows.Close()
	for docID := range staleDocSet {
		report.StaleDocumentIDs = append(report.StaleDocumentIDs, docID)
	}
	report.StaleCount = len(report.StaleDocumentIDs)

	cutoff := time.Now().Add(-stuckThreshold)
	stuckRows, err := tx.Query(ctx, `
		SELECT DISTINCT ku.document_id, ku.knowledge_id, ku.embedding_status, ku.updated_at
		FROM knowledge_units ku
		JOIN documents d ON d.document_id = ku.document_id
		WHERE ku.org_id = $1
		  AND ku.embedding_status = 'pending'
		  AND ku.created_at < $2
		  -- D19: a unit the reconciler just re-drove is 'pending' again with an
		  -- OLD created_at, so without this clause every re-drive would
		  -- immediately masquerade as "stuck for 30 minutes". Judge a re-driven
		  -- unit from when it was re-driven, not from when it was first chunked.
		  AND (ku.embedding_retry_at IS NULL OR ku.embedding_retry_at < $2)
		  AND d.deleted_at IS NULL
		ORDER BY ku.updated_at DESC
		LIMIT 500
	`, orgID, cutoff)
	if err != nil {
		return fmt.Errorf("detect stuck pending: %w", err)
	}
	defer stuckRows.Close()

	stuckDocSet := make(map[string]bool)
	for stuckRows.Next() {
		var detail StaleEmbedDetail
		if err := stuckRows.Scan(&detail.DocumentID, &detail.KnowledgeID, &detail.EmbeddingStatus, &detail.ContentUpdatedAt); err != nil {
			continue
		}
		detail.Reason = "stuck_pending"
		report.Details = append(report.Details, detail)
		stuckDocSet[detail.DocumentID] = true
	}
	stuckRows.Close()
	for docID := range stuckDocSet {
		report.StuckDocumentIDs = append(report.StuckDocumentIDs, docID)
	}
	report.StuckPendingCount = len(report.StuckDocumentIDs)

	maxAttempts := reconcileMaxAttempts()
	var failedCount, retryableFailed, exhaustedFailed int
	err = tx.QueryRow(ctx, `
		SELECT
			COUNT(DISTINCT document_id),
			COUNT(DISTINCT document_id) FILTER (WHERE embedding_retry_count < $2),
			COUNT(DISTINCT document_id) FILTER (WHERE embedding_retry_count >= $2)
		FROM knowledge_units
		WHERE org_id = $1 AND embedding_status = 'failed'
	`, orgID, maxAttempts).Scan(&failedCount, &retryableFailed, &exhaustedFailed)
	if err != nil {
		return fmt.Errorf("count failed embeddings: %w", err)
	}
	report.FailedCount = failedCount
	// A document with a mix of retryable and exhausted units counts in both.
	// That is deliberate: it still needs the human, and it is still healing.
	report.RetryableFailedCount = retryableFailed
	report.ExhaustedFailedCount = exhaustedFailed

	if failedCount > 0 {
		report.FailedDocumentIDs = append(report.FailedDocumentIDs, collectTolerated(ctx, tx, `
			SELECT DISTINCT document_id FROM knowledge_units
			WHERE org_id = $1 AND embedding_status = 'failed'
			LIMIT 200
		`, orgID)...)
	}

	if exhaustedFailed > 0 {
		report.ExhaustedFailedDocumentIDs = append(report.ExhaustedFailedDocumentIDs, collectTolerated(ctx, tx, `
			SELECT DISTINCT document_id FROM knowledge_units
			WHERE org_id = $1
			  AND embedding_status = 'failed'
			  AND embedding_retry_count >= $2
			LIMIT 200
		`, orgID, maxAttempts)...)
	}

	return nil
}
