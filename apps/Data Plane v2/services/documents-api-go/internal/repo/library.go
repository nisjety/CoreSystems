package repo

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

// ListLibrary pages one org's library: the live documents whose metadata origin
// is one of input.Origins and that belong to no Space. The viewer predicate is
// the one List and Get apply, so the library never lists a document the viewer
// could not open. The page never selects the text; octet_length is the only
// read of content.
func (r *DocumentRepo) ListLibrary(ctx context.Context, input model.ListLibraryInput) (*model.ListLibraryResult, error) {
	limit := input.Limit
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	origins := normalizeGranted(input.Origins)
	granted := normalizeGranted(input.GrantedIDs)

	return orgscope.InOrgScope(ctx, r.pool, input.OrgID,
		func(ctx context.Context, tx pgx.Tx) (*model.ListLibraryResult, error) {
			var total int
			err := tx.QueryRow(ctx, `
				SELECT COUNT(*) FROM documents
				WHERE org_id = $1 AND deleted_at IS NULL AND space_ref IS NULL
				  AND metadata->>'origin' = ANY($2::text[])
				  AND ($3 = '' OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4::text[]))
			`, input.OrgID, origins, input.ViewerID, granted).Scan(&total)
			if err != nil {
				return nil, fmt.Errorf("count library documents: %w", err)
			}

			rows, err := tx.Query(ctx, `
				SELECT document_id, source, type, title, status, metadata, created_by,
				       owner_id, visibility, octet_length(content)::bigint, created_at, updated_at
				FROM documents
				WHERE org_id = $1 AND deleted_at IS NULL AND space_ref IS NULL
				  AND metadata->>'origin' = ANY($2::text[])
				  AND ($3 = '' OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4::text[]))
				ORDER BY updated_at DESC, document_id DESC
				LIMIT $5 OFFSET $6
			`, input.OrgID, origins, input.ViewerID, granted, limit, input.Offset)
			if err != nil {
				return nil, fmt.Errorf("list library documents: %w", err)
			}
			defer rows.Close()

			docs := []model.LibraryItem{}
			for rows.Next() {
				var d model.LibraryItem
				if err := rows.Scan(
					&d.DocumentID, &d.Source, &d.Type, &d.Title, &d.Status, &d.Metadata, &d.CreatedBy,
					&d.OwnerID, &d.Visibility, &d.ContentBytes, &d.CreatedAt, &d.UpdatedAt,
				); err != nil {
					return nil, fmt.Errorf("scan library document: %w", err)
				}
				docs = append(docs, d)
			}
			if err := rows.Err(); err != nil {
				return nil, fmt.Errorf("read library documents: %w", err)
			}
			return &model.ListLibraryResult{Documents: docs, Total: total}, nil
		})
}
