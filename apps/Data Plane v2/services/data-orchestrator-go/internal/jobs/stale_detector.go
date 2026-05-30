package jobs

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog/log"
)

type StaleEmbeddingReport struct {
	OrgID             string             `json:"org_id"`
	StaleCount        int                `json:"stale_count"`
	StuckPendingCount int                `json:"stuck_pending_count"`
	FailedCount       int                `json:"failed_count"`
	StaleDocumentIDs  []string           `json:"stale_document_ids,omitempty"`
	StuckDocumentIDs  []string           `json:"stuck_document_ids,omitempty"`
	FailedDocumentIDs []string           `json:"failed_document_ids,omitempty"`
	CheckedAt         time.Time          `json:"checked_at"`
	Details           []StaleEmbedDetail `json:"details,omitempty"`
}

type StaleEmbedDetail struct {
	DocumentID      string    `json:"document_id"`
	KnowledgeID     string    `json:"knowledge_id"`
	EmbeddingStatus string    `json:"embedding_status"`
	ContentUpdatedAt time.Time `json:"content_updated_at"`
	EmbeddedAt      *time.Time `json:"embedded_at,omitempty"`
	Reason          string    `json:"reason"`
}

type StaleDetector struct {
	pool *pgxpool.Pool
}

func NewStaleDetector(pool *pgxpool.Pool) *StaleDetector {
	return &StaleDetector{pool: pool}
}

func (d *StaleDetector) Detect(ctx context.Context, orgID string, stuckThreshold time.Duration) (*StaleEmbeddingReport, error) {
	if stuckThreshold == 0 {
		stuckThreshold = 30 * time.Minute
	}

	report := &StaleEmbeddingReport{
		OrgID:     orgID,
		CheckedAt: time.Now(),
	}

	staleRows, err := d.pool.Query(ctx, `
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
		return nil, fmt.Errorf("detect stale embeddings: %w", err)
	}
	defer staleRows.Close()

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
	for docID := range staleDocSet {
		report.StaleDocumentIDs = append(report.StaleDocumentIDs, docID)
	}
	report.StaleCount = len(report.StaleDocumentIDs)

	cutoff := time.Now().Add(-stuckThreshold)
	stuckRows, err := d.pool.Query(ctx, `
		SELECT DISTINCT ku.document_id, ku.knowledge_id, ku.embedding_status, ku.updated_at
		FROM knowledge_units ku
		JOIN documents d ON d.document_id = ku.document_id
		WHERE ku.org_id = $1
		  AND ku.embedding_status = 'pending'
		  AND ku.created_at < $2
		  AND d.deleted_at IS NULL
		ORDER BY ku.updated_at DESC
		LIMIT 500
	`, orgID, cutoff)
	if err != nil {
		return nil, fmt.Errorf("detect stuck pending: %w", err)
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
	for docID := range stuckDocSet {
		report.StuckDocumentIDs = append(report.StuckDocumentIDs, docID)
	}
	report.StuckPendingCount = len(report.StuckDocumentIDs)

	var failedCount int
	err = d.pool.QueryRow(ctx, `
		SELECT COUNT(DISTINCT document_id) FROM knowledge_units
		WHERE org_id = $1 AND embedding_status = 'failed'
	`, orgID).Scan(&failedCount)
	if err != nil {
		return nil, fmt.Errorf("count failed embeddings: %w", err)
	}
	report.FailedCount = failedCount

	if failedCount > 0 {
		failedRows, err := d.pool.Query(ctx, `
			SELECT DISTINCT document_id FROM knowledge_units
			WHERE org_id = $1 AND embedding_status = 'failed'
			LIMIT 200
		`, orgID)
		if err == nil {
			defer failedRows.Close()
			for failedRows.Next() {
				var docID string
				if failedRows.Scan(&docID) == nil {
					report.FailedDocumentIDs = append(report.FailedDocumentIDs, docID)
				}
			}
		}
	}

	log.Info().
		Str("org_id", orgID).
		Int("stale", report.StaleCount).
		Int("stuck_pending", report.StuckPendingCount).
		Int("failed", report.FailedCount).
		Msg("stale embedding detection complete")

	return report, nil
}
