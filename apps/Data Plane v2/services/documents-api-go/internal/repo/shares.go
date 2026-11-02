package repo

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// WS14-S4: the "shared with me" listing. The ids come from Control's grant
// store (resource_grants); this only resolves them to document headers inside
// the viewer's org, excluding the viewer's own documents and deleted rows.
// Content is never selected.

// SharedDocumentSummary is a document header shown in the shared-with-me list.
type SharedDocumentSummary struct {
	DocumentID string    `json:"document_id"`
	Title      string    `json:"title"`
	Type       string    `json:"type"`
	Source     string    `json:"source"`
	OwnerID    string    `json:"owner_id"`
	SpaceRef   *string   `json:"space_ref,omitempty"`
	CreatedAt  time.Time `json:"created_at"`
	UpdatedAt  time.Time `json:"updated_at"`
}

// ListSharedWith resolves grantedIDs to headers of live documents in orgID that
// viewerID does not own. limit is clamped to 1..200.
func (r *DocumentRepo) ListSharedWith(ctx context.Context, orgID, viewerID string, grantedIDs []string, limit int) ([]SharedDocumentSummary, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	granted := normalizeGranted(grantedIDs)
	return orgscope.InOrgScope(ctx, r.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) ([]SharedDocumentSummary, error) {
			rows, err := tx.Query(ctx, `
				SELECT document_id, title, type, source, owner_id, space_ref, created_at, updated_at
				FROM documents
				WHERE org_id = $1 AND deleted_at IS NULL
				  AND document_id = ANY($2) AND owner_id <> $3
				ORDER BY updated_at DESC
				LIMIT $4
			`, orgID, granted, viewerID, limit)
			if err != nil {
				return nil, fmt.Errorf("list shared documents: %w", err)
			}
			defer rows.Close()
			out := []SharedDocumentSummary{}
			for rows.Next() {
				var d SharedDocumentSummary
				if err := rows.Scan(&d.DocumentID, &d.Title, &d.Type, &d.Source, &d.OwnerID, &d.SpaceRef, &d.CreatedAt, &d.UpdatedAt); err != nil {
					return nil, fmt.Errorf("scan shared document: %w", err)
				}
				out = append(out, d)
			}
			return out, rows.Err()
		})
}
