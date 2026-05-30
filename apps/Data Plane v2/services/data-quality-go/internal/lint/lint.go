// Package lint detects knowledge-quality issues across documents and wiki:
//   - orphan documents: indexed but never retrieved
//   - stale documents: older than threshold and not updated
//   - orphan wiki pages: no backlinks, never linked from sources
//   - stale wiki pages: not updated within threshold
//   - weak citations: wiki versions with empty source_refs
package lint

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

type Linter struct {
	pool *pgxpool.Pool
}

func NewLinter(pool *pgxpool.Pool) *Linter {
	return &Linter{pool: pool}
}

type Issue struct {
	Kind        string `json:"kind"`         // "orphan_doc" | "stale_doc" | "orphan_wiki" | "stale_wiki" | "weak_citation"
	Severity    string `json:"severity"`     // "info" | "warning" | "error"
	ID          string `json:"id"`           // document_id or page_id
	Title       string `json:"title,omitempty"`
	Description string `json:"description"`
	DetectedAt  time.Time `json:"detected_at"`
}

type Report struct {
	OrgID    string  `json:"org_id"`
	Issues   []Issue `json:"issues"`
	Counts   map[string]int `json:"counts"`
	RanAt    time.Time `json:"ran_at"`
}

// Run executes all linters and returns a consolidated report.
// staleAfterDays defaults to 90 if zero.
func (l *Linter) Run(ctx context.Context, orgID string, staleAfterDays int) (*Report, error) {
	if staleAfterDays <= 0 {
		staleAfterDays = 90
	}

	report := &Report{
		OrgID:  orgID,
		Issues: []Issue{},
		Counts: map[string]int{},
		RanAt:  time.Now().UTC(),
	}

	checks := []struct {
		name string
		fn   func(context.Context, string, int) ([]Issue, error)
	}{
		{"orphan_docs", l.findOrphanDocs},
		{"stale_docs", l.findStaleDocs},
		{"orphan_wiki", l.findOrphanWikiPages},
		{"stale_wiki", l.findStaleWikiPages},
		{"weak_citations", l.findWeakCitations},
	}

	for _, c := range checks {
		issues, err := c.fn(ctx, orgID, staleAfterDays)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", c.name, err)
		}
		report.Issues = append(report.Issues, issues...)
		report.Counts[c.name] = len(issues)
	}
	return report, nil
}

func (l *Linter) findOrphanDocs(ctx context.Context, orgID string, _ int) ([]Issue, error) {
	rows, err := l.pool.Query(ctx, `
		SELECT d.document_id, d.title
		FROM documents d
		LEFT JOIN retrieval_candidates rc ON rc.document_id = d.document_id
		WHERE d.org_id = $1
		  AND d.deleted_at IS NULL
		  AND d.status = 'indexed'
		  AND rc.document_id IS NULL
		LIMIT 200
	`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	now := time.Now().UTC()
	var issues []Issue
	for rows.Next() {
		var id, title string
		if err := rows.Scan(&id, &title); err != nil {
			return nil, err
		}
		issues = append(issues, Issue{
			Kind:        "orphan_doc",
			Severity:    "info",
			ID:          id,
			Title:       title,
			Description: "Indexed document has never appeared in a retrieval candidate set",
			DetectedAt:  now,
		})
	}
	return issues, rows.Err()
}

func (l *Linter) findStaleDocs(ctx context.Context, orgID string, days int) ([]Issue, error) {
	rows, err := l.pool.Query(ctx, `
		SELECT document_id, title
		FROM documents
		WHERE org_id = $1
		  AND deleted_at IS NULL
		  AND updated_at < NOW() - ($2 || ' days')::INTERVAL
		ORDER BY updated_at ASC
		LIMIT 200
	`, orgID, fmt.Sprintf("%d", days))
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	now := time.Now().UTC()
	var issues []Issue
	for rows.Next() {
		var id, title string
		if err := rows.Scan(&id, &title); err != nil {
			return nil, err
		}
		issues = append(issues, Issue{
			Kind:        "stale_doc",
			Severity:    "warning",
			ID:          id,
			Title:       title,
			Description: fmt.Sprintf("Document not updated in over %d days", days),
			DetectedAt:  now,
		})
	}
	return issues, rows.Err()
}

func (l *Linter) findOrphanWikiPages(ctx context.Context, orgID string, _ int) ([]Issue, error) {
	rows, err := l.pool.Query(ctx, `
		SELECT p.page_id, p.title
		FROM wiki_pages p
		WHERE p.org_id = $1
		  AND p.deleted_at IS NULL
		  AND NOT EXISTS (
		      SELECT 1 FROM wiki_page_versions v
		      WHERE v.page_id <> p.page_id
		        AND v.org_id = $1
		        AND v.source_refs @> jsonb_build_array(jsonb_build_object('type', 'wiki', 'page_id', p.page_id))
		  )
		LIMIT 200
	`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	now := time.Now().UTC()
	var issues []Issue
	for rows.Next() {
		var id, title string
		if err := rows.Scan(&id, &title); err != nil {
			return nil, err
		}
		issues = append(issues, Issue{
			Kind:        "orphan_wiki",
			Severity:    "info",
			ID:          id,
			Title:       title,
			Description: "Wiki page has no incoming backlinks from other pages",
			DetectedAt:  now,
		})
	}
	return issues, rows.Err()
}

func (l *Linter) findStaleWikiPages(ctx context.Context, orgID string, days int) ([]Issue, error) {
	rows, err := l.pool.Query(ctx, `
		SELECT p.page_id, p.title
		FROM wiki_pages p
		WHERE p.org_id = $1
		  AND p.deleted_at IS NULL
		  AND p.updated_at < NOW() - ($2 || ' days')::INTERVAL
		ORDER BY p.updated_at ASC
		LIMIT 200
	`, orgID, fmt.Sprintf("%d", days))
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	now := time.Now().UTC()
	var issues []Issue
	for rows.Next() {
		var id, title string
		if err := rows.Scan(&id, &title); err != nil {
			return nil, err
		}
		issues = append(issues, Issue{
			Kind:        "stale_wiki",
			Severity:    "warning",
			ID:          id,
			Title:       title,
			Description: fmt.Sprintf("Wiki page not updated in over %d days", days),
			DetectedAt:  now,
		})
	}
	return issues, rows.Err()
}

func (l *Linter) findWeakCitations(ctx context.Context, orgID string, _ int) ([]Issue, error) {
	rows, err := l.pool.Query(ctx, `
		SELECT v.version_id, p.title
		FROM wiki_page_versions v
		JOIN wiki_pages p ON p.page_id = v.page_id
		WHERE v.org_id = $1
		  AND p.deleted_at IS NULL
		  AND p.current_version_id = v.version_id
		  AND (v.source_refs IS NULL OR jsonb_array_length(v.source_refs) = 0)
		LIMIT 200
	`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	now := time.Now().UTC()
	var issues []Issue
	for rows.Next() {
		var id, title string
		if err := rows.Scan(&id, &title); err != nil {
			return nil, err
		}
		issues = append(issues, Issue{
			Kind:        "weak_citation",
			Severity:    "warning",
			ID:          id,
			Title:       title,
			Description: "Wiki page current version has no source_refs",
			DetectedAt:  now,
		})
	}
	return issues, rows.Err()
}
