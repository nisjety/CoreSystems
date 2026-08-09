package repo

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/events"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
)

// Phase 1 RLS — how this repository is scoped.
//
// Every exported method here serves exactly one organization: the org_id
// arrives from verified caller claims (authctx) through the HTTP handler or
// the gRPC server, never from an unauthenticated request field. So each one
// runs its queries inside orgscope.WithOrgScope, which sets
// `app.current_org` and drops to the NOBYPASSRLS `dataplane_app` role for the
// duration of one transaction. The SQL below still binds org_id itself —
// the database policy is a backstop against that filter being dropped or
// mis-edited later, not a replacement for it.
//
// Two paths in this package deliberately stay on the unscoped pool; both are
// commented where they live:
//
//   - internal/events/outbox.go's postgresOutboxStore — a cross-org
//     background drain. A scoped transaction would see one org's rows and
//     silently stop publishing everyone else's events.
//   - HardPurgeByOrg (org_purge.go) — the GDPR erasure path.
//
// The unexported helpers below take a pgx.Tx rather than the pool, so the
// type signature makes it impossible to call one outside a scope by mistake.

type WikiRepo struct {
	pool *pgxpool.Pool
}

func NewWikiRepo(pool *pgxpool.Pool) *WikiRepo {
	return &WikiRepo{pool: pool}
}

func verifiedPrincipalID(ctx context.Context) string {
	claims, ok := authctx.FromContext(ctx)
	if !ok {
		return ""
	}
	return claims.PrincipalID()
}

func enqueueWikiPublished(ctx context.Context, tx pgx.Tx, event events.WikiVersionPublishedEvent) error {
	payload, err := json.Marshal(event)
	if err != nil {
		return fmt.Errorf("marshal wiki outbox event: %w", err)
	}
	var userID any
	if event.UserID != "" {
		userID = event.UserID
	}
	_, err = tx.Exec(ctx, `
		INSERT INTO wiki_event_outbox
			(org_id, user_id, event_type, payload, idempotency_key)
		VALUES ($1, $2, $3, $4::jsonb, $5)
		ON CONFLICT (idempotency_key) DO NOTHING
	`, event.OrgID, userID, events.SubjectWikiPublished, string(payload), "wiki.published:"+event.VersionID)
	if err != nil {
		return fmt.Errorf("enqueue wiki published event: %w", err)
	}
	return nil
}

// ListPages — Wave 3.1 / Wave 11.C-b close: paginated wiki page enumeration
// for the verevon sidebar (which previously had to fall back to localStorage
// bookmarks). Filter is optional on workspace_id + page_status; both empty
// means "every non-deleted page in the org".
func (r *WikiRepo) ListPages(
	ctx context.Context,
	orgID, workspaceID, status string,
	limit, offset int,
) ([]model.WikiPage, int, error) {
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	if offset < 0 {
		offset = 0
	}

	var total int
	var pages []model.WikiPage

	// Phase 1 RLS: one org's page list. The COUNT and the page query share a
	// single scoped transaction so they cannot disagree about which rows
	// exist under a concurrent write.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		// Four static query shapes — keeps tenant-isolation static check happy
		// and avoids any %s-formatted SQL. The branching is verbose but each
		// shape is unambiguous and audit-friendly.
		var rows pgx.Rows
		var err error

		switch {
		case workspaceID != "" && status != "":
			err = tx.QueryRow(ctx,
				`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND workspace_id = $2 AND page_status = $3
               AND deleted_at IS NULL AND LOWER(COALESCE(page_status, '')) <> 'deleted'`,
				orgID, workspaceID, status,
			).Scan(&total)
			if err != nil {
				return fmt.Errorf("count wiki pages: %w", err)
			}
			rows, err = tx.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND workspace_id = $2 AND page_status = $3
              AND deleted_at IS NULL AND LOWER(COALESCE(page_status, '')) <> 'deleted'
            ORDER BY updated_at DESC LIMIT $4 OFFSET $5
        `, orgID, workspaceID, status, limit, offset)
		case workspaceID != "":
			err = tx.QueryRow(ctx,
				`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND workspace_id = $2 AND deleted_at IS NULL
               AND LOWER(COALESCE(page_status, '')) <> 'deleted'`,
				orgID, workspaceID,
			).Scan(&total)
			if err != nil {
				return fmt.Errorf("count wiki pages: %w", err)
			}
			rows, err = tx.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND workspace_id = $2 AND deleted_at IS NULL
              AND LOWER(COALESCE(page_status, '')) <> 'deleted'
            ORDER BY updated_at DESC LIMIT $3 OFFSET $4
        `, orgID, workspaceID, limit, offset)
		case status != "":
			err = tx.QueryRow(ctx,
				`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND page_status = $2 AND deleted_at IS NULL
               AND LOWER(COALESCE(page_status, '')) <> 'deleted'`,
				orgID, status,
			).Scan(&total)
			if err != nil {
				return fmt.Errorf("count wiki pages: %w", err)
			}
			rows, err = tx.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND page_status = $2 AND deleted_at IS NULL
              AND LOWER(COALESCE(page_status, '')) <> 'deleted'
            ORDER BY updated_at DESC LIMIT $3 OFFSET $4
        `, orgID, status, limit, offset)
		default:
			err = tx.QueryRow(ctx,
				`SELECT COUNT(*) FROM wiki_pages WHERE org_id = $1 AND deleted_at IS NULL
             AND LOWER(COALESCE(page_status, '')) <> 'deleted'`,
				orgID,
			).Scan(&total)
			if err != nil {
				return fmt.Errorf("count wiki pages: %w", err)
			}
			rows, err = tx.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND deleted_at IS NULL
              AND LOWER(COALESCE(page_status, '')) <> 'deleted'
            ORDER BY updated_at DESC LIMIT $2 OFFSET $3
        `, orgID, limit, offset)
		}
		if err != nil {
			return fmt.Errorf("list wiki pages: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var p model.WikiPage
			if err := rows.Scan(
				&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path,
				&p.CurrentVersionID, &p.Status, &p.Backlinks, &p.Metadata,
				&p.CreatedAt, &p.UpdatedAt,
			); err != nil {
				return fmt.Errorf("scan wiki page: %w", err)
			}
			pages = append(pages, p)
		}
		return nil
	})
	if err != nil {
		return nil, 0, err
	}
	return pages, total, nil
}

func (r *WikiRepo) GetPage(ctx context.Context, orgID, pageID string) (*model.WikiPage, error) {
	var p model.WikiPage
	// Phase 1 RLS: single-org page read.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
			SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
			       backlinks, metadata, created_at, updated_at
			FROM wiki_pages WHERE page_id = $1 AND org_id = $2 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, pageID, orgID).Scan(
			&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
			&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
		)
	})
	if err != nil {
		return nil, fmt.Errorf("get page: %w", err)
	}
	return &p, nil
}

func (r *WikiRepo) GetPageByPath(ctx context.Context, orgID, path string) (*model.WikiPage, error) {
	var p model.WikiPage
	// Phase 1 RLS: single-org page read.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
			SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
			       backlinks, metadata, created_at, updated_at
			FROM wiki_pages WHERE org_id = $1 AND path = $2 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, orgID, path).Scan(
			&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
			&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
		)
	})
	if err != nil {
		return nil, fmt.Errorf("get page by path: %w", err)
	}
	return &p, nil
}

func (r *WikiRepo) GetVersionForPage(ctx context.Context, orgID, pageID, versionID string) (*model.WikiPageVersion, error) {
	var v model.WikiPageVersion
	// Phase 1 RLS: single-org version read. wiki_page_versions carries no
	// org_id of its own — the child policy from
	// 20260809180000_org_rls_child_tables.sql derives it from the wiki_pages
	// parent this query already joins.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
			SELECT v.version_id, v.page_id, v.content, v.source_refs, v.proposed_by_agent, v.proposed_by_user,
			       v.approved_by, v.edit_reason, v.version_status, v.metadata, v.created_at, v.published_at
			FROM wiki_page_versions AS v
			JOIN wiki_pages AS p ON p.page_id = v.page_id
			WHERE v.version_id = $1 AND v.page_id = $2 AND p.org_id = $3 AND p.deleted_at IS NULL
			  AND LOWER(COALESCE(p.page_status, '')) <> 'deleted'
		`, versionID, pageID, orgID).Scan(
			&v.VersionID, &v.PageID, &v.Content, &v.SourceRefs, &v.ProposedByAgent,
			&v.ProposedByUser, &v.ApprovedBy, &v.EditReason, &v.VersionStatus,
			&v.Metadata, &v.CreatedAt, &v.PublishedAt,
		)
	})
	if err != nil {
		return nil, fmt.Errorf("get version for page: %w", err)
	}
	return &v, nil
}

func (r *WikiRepo) ListVersions(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.WikiPageVersion, int, error) {
	var total int
	var versions []model.WikiPageVersion

	// Phase 1 RLS: one org's version history — count and page share one
	// scoped transaction.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx,
			"SELECT COUNT(*) FROM wiki_page_versions v JOIN wiki_pages p ON v.page_id = p.page_id WHERE v.page_id = $1 AND p.org_id = $2 AND p.deleted_at IS NULL AND LOWER(COALESCE(p.page_status, '')) <> 'deleted'",
			pageID, orgID).Scan(&total); err != nil {
			return fmt.Errorf("count versions: %w", err)
		}

		rows, err := tx.Query(ctx, `
			SELECT v.version_id, v.page_id, v.content, v.source_refs, v.proposed_by_agent,
			       v.proposed_by_user, v.approved_by, v.edit_reason, v.version_status,
			       v.metadata, v.created_at, v.published_at
			FROM wiki_page_versions v JOIN wiki_pages p ON v.page_id = p.page_id
			WHERE v.page_id = $1 AND p.org_id = $2 AND p.deleted_at IS NULL
			  AND LOWER(COALESCE(p.page_status, '')) <> 'deleted'
			ORDER BY v.created_at DESC LIMIT $3 OFFSET $4
		`, pageID, orgID, limit, offset)
		if err != nil {
			return fmt.Errorf("list versions: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var v model.WikiPageVersion
			if err := rows.Scan(
				&v.VersionID, &v.PageID, &v.Content, &v.SourceRefs, &v.ProposedByAgent,
				&v.ProposedByUser, &v.ApprovedBy, &v.EditReason, &v.VersionStatus,
				&v.Metadata, &v.CreatedAt, &v.PublishedAt,
			); err != nil {
				return fmt.Errorf("scan version: %w", err)
			}
			versions = append(versions, v)
		}
		return nil
	})
	if err != nil {
		return nil, 0, err
	}
	return versions, total, nil
}

func (r *WikiRepo) CreatePage(ctx context.Context, input model.CreatePageInput) (*model.WikiPage, *model.WikiPageVersion, error) {
	pageID := uuid.New().String()
	versionID := uuid.New().String()
	now := time.Now()

	// Phase 1 RLS: this function already opened its own transaction, so the
	// scope replaces that Begin rather than nesting a second transaction
	// inside it. Ordering matters for RLS as well as for the FK: the
	// wiki_pages row is inserted first, so the wiki_page_versions WITH CHECK
	// — which derives the org from its parent page — can see it.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `
			INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path, current_version_id, page_status)
			VALUES ($1, $2, $3, $4, $5, $6, 'published')
		`, pageID, input.OrgID, input.WorkspaceID, input.Title, input.Path, versionID); err != nil {
			return fmt.Errorf("insert page: %w", err)
		}

		if _, err := tx.Exec(ctx, `
			INSERT INTO wiki_page_versions (version_id, page_id, content, version_status, created_at, published_at)
			VALUES ($1, $2, $3, 'published', $4, $4)
		`, versionID, pageID, input.InitialContent, now); err != nil {
			return fmt.Errorf("insert version: %w", err)
		}

		return enqueueWikiPublished(ctx, tx, events.WikiVersionPublishedEvent{
			PageID: pageID, VersionID: versionID, OrgID: input.OrgID,
			WorkspaceID: input.WorkspaceID, Title: input.Title, Path: input.Path,
			Content: input.InitialContent, UserID: verifiedPrincipalID(ctx), ZDR: false,
		})
	})
	if err != nil {
		return nil, nil, err
	}

	page, _ := r.GetPage(ctx, input.OrgID, pageID)
	version, _ := r.GetVersionForPage(ctx, input.OrgID, pageID, versionID)

	return page, version, nil
}

func (r *WikiRepo) CreateVersion(ctx context.Context, input model.UpdateVersionInput) (*model.WikiPageVersion, error) {
	versionID := uuid.New().String()
	now := time.Now()

	// Phase 1 RLS: replaces this function's own Begin — the page lookup, the
	// version insert, the current_version_id update and the outbox intent all
	// belong to input.OrgID and stay in one transaction, exactly as before.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		var workspaceID, title, path string
		if err := tx.QueryRow(ctx, `
			SELECT workspace_id, title, path FROM wiki_pages
			WHERE page_id = $1 AND org_id = $2 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
			FOR UPDATE
		`, input.PageID, input.OrgID).Scan(&workspaceID, &title, &path); err != nil {
			return fmt.Errorf("load page for version: %w", err)
		}

		if _, err := tx.Exec(ctx, `
			INSERT INTO wiki_page_versions (version_id, page_id, content, edit_reason, proposed_by_user, version_status, created_at, published_at)
			VALUES ($1, $2, $3, $4, $5, 'published', $6, $6)
		`, versionID, input.PageID, input.NewContent, input.EditReason, input.ProposedBy, now); err != nil {
			return fmt.Errorf("insert version: %w", err)
		}

		if _, err := tx.Exec(ctx,
			"UPDATE wiki_pages SET current_version_id = $1 WHERE page_id = $2 AND org_id = $3",
			versionID, input.PageID, input.OrgID); err != nil {
			return fmt.Errorf("update page version: %w", err)
		}

		return enqueueWikiPublished(ctx, tx, events.WikiVersionPublishedEvent{
			PageID: input.PageID, VersionID: versionID, OrgID: input.OrgID,
			WorkspaceID: workspaceID, Title: title, Path: path,
			Content: input.NewContent, UserID: verifiedPrincipalID(ctx), ZDR: false,
		})
	})
	if err != nil {
		return nil, err
	}

	return r.GetVersionForPage(ctx, input.OrgID, input.PageID, versionID)
}

func (r *WikiRepo) SubmitProposal(ctx context.Context, input model.SubmitProposalInput) (*model.WikiProposal, error) {
	id := uuid.New().String()
	refs, _ := json.Marshal(input.SourceRefs)

	// Phase 1 RLS: the INSERT ... SELECT sources its org_id from wiki_pages,
	// which the policy has already narrowed to this org — so the row written
	// can only ever belong to the scoped tenant.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		result, err := tx.Exec(ctx, `
			INSERT INTO wiki_proposals (proposal_id, page_id, org_id, proposed_content, edit_reason, proposed_by_agent, source_refs, proposal_status)
			SELECT $1, page_id, org_id, $4, $5, $6, $7, 'pending'
			FROM wiki_pages
			WHERE page_id = $2 AND org_id = $3 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, id, input.PageID, input.OrgID, input.ProposedContent, input.EditReason, input.ProposedByAgent, refs)
		if err != nil {
			return fmt.Errorf("insert proposal: %w", err)
		}
		if result.RowsAffected() != 1 {
			return fmt.Errorf("insert proposal: %w", pgx.ErrNoRows)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	return r.GetProposal(ctx, input.OrgID, id)
}

func (r *WikiRepo) GetProposal(ctx context.Context, orgID, proposalID string) (*model.WikiProposal, error) {
	var p model.WikiProposal
	// Phase 1 RLS: single-org proposal read.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
			SELECT proposal.proposal_id, proposal.page_id, proposal.org_id, proposal.proposed_content,
			       proposal.edit_reason, proposal.proposed_by_agent, proposal.source_refs,
			       proposal.proposal_status, proposal.reviewed_by, proposal.metadata, proposal.created_at
			FROM wiki_proposals AS proposal
			JOIN wiki_pages AS page ON page.page_id = proposal.page_id
			WHERE proposal.proposal_id = $1 AND proposal.org_id = $2
			  AND page.org_id = proposal.org_id AND page.deleted_at IS NULL
			  AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'
		`, proposalID, orgID).Scan(
			&p.ProposalID, &p.PageID, &p.OrgID, &p.ProposedContent, &p.EditReason,
			&p.ProposedByAgent, &p.SourceRefs, &p.ProposalStatus, &p.ReviewedBy,
			&p.Metadata, &p.CreatedAt,
		)
	})
	if err != nil {
		return nil, fmt.Errorf("get proposal: %w", err)
	}
	return &p, nil
}

func (r *WikiRepo) ReviewProposal(ctx context.Context, input model.ReviewProposalInput) (*model.WikiProposal, *model.WikiPageVersion, error) {
	proposal, err := r.GetProposal(ctx, input.OrgID, input.ProposalID)
	if err != nil {
		return nil, nil, err
	}

	status := "rejected"
	if input.Decision == "accept" {
		status = "accepted"
	}

	// Phase 1 RLS: the status update is its own scope. Deliberately NOT
	// merged with the GetProposal above or the CreateVersion below — those
	// were already three independent, separately-committed statements, and
	// folding them into one transaction would change this method's failure
	// semantics rather than just its isolation.
	if err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx,
			"UPDATE wiki_proposals SET proposal_status = $1, reviewed_by = $2 WHERE proposal_id = $3 AND org_id = $4",
			status, input.ReviewedBy, input.ProposalID, input.OrgID)
		if err != nil {
			return fmt.Errorf("update proposal status: %w", err)
		}
		return nil
	}); err != nil {
		return nil, nil, err
	}

	proposal.ProposalStatus = status
	rb := input.ReviewedBy
	proposal.ReviewedBy = &rb

	if input.Decision == "accept" {
		version, err := r.CreateVersion(ctx, model.UpdateVersionInput{
			PageID:     proposal.PageID,
			OrgID:      proposal.OrgID,
			NewContent: proposal.ProposedContent,
			EditReason: "accepted proposal " + input.ProposalID,
		})
		if err != nil {
			return proposal, nil, err
		}
		return proposal, version, nil
	}

	return proposal, nil, nil
}

func (r *WikiRepo) CreateSourceLog(ctx context.Context, input model.CreateSourceLogInput) (*model.SourceLog, error) {
	id := uuid.New().String()
	details := input.Details
	if details == nil {
		details = json.RawMessage(`{}`)
	}

	var sl model.SourceLog
	// Phase 1 RLS: insert and read-back are the same org, so they share one
	// scoped transaction — the read-back now sees its own write instead of
	// racing a concurrent delete on a second connection.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		result, err := tx.Exec(ctx, `
			INSERT INTO wiki_source_logs (log_id, org_id, page_id, source_type, source_ref, sync_status, details)
			SELECT $1, org_id, page_id, $4, $5, $6, $7
			FROM wiki_pages
			WHERE org_id = $2 AND page_id = $3 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, id, input.OrgID, input.PageID, input.SourceType, input.SourceRef, input.SyncStatus, details)
		if err != nil {
			return fmt.Errorf("insert source log: %w", err)
		}
		if result.RowsAffected() != 1 {
			return fmt.Errorf("insert source log: %w", pgx.ErrNoRows)
		}

		if err := tx.QueryRow(ctx,
			"SELECT log_id, org_id, page_id, source_type, source_ref, sync_status, details, created_at FROM wiki_source_logs WHERE log_id = $1 AND org_id = $2", id, input.OrgID,
		).Scan(&sl.LogID, &sl.OrgID, &sl.PageID, &sl.SourceType, &sl.SourceRef, &sl.SyncStatus, &sl.Details, &sl.CreatedAt); err != nil {
			return fmt.Errorf("read source log: %w", err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &sl, nil
}

func (r *WikiRepo) ListSourceLogs(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.SourceLog, int, error) {
	var total int
	var logs []model.SourceLog

	// Phase 1 RLS: one org's source logs — count and page share one scope.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_source_logs AS log
			 JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
			 WHERE log.org_id = $1 AND log.page_id = $2 AND page.deleted_at IS NULL
			   AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'`,
			orgID, pageID).Scan(&total); err != nil {
			return fmt.Errorf("count source logs: %w", err)
		}

		rows, err := tx.Query(ctx, `
			SELECT log.log_id, log.org_id, log.page_id, log.source_type, log.source_ref,
			       log.sync_status, log.details, log.created_at
			FROM wiki_source_logs AS log
			JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
			WHERE log.org_id = $1 AND log.page_id = $2 AND page.deleted_at IS NULL
			  AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'
			ORDER BY log.created_at DESC LIMIT $3 OFFSET $4
		`, orgID, pageID, limit, offset)
		if err != nil {
			return fmt.Errorf("list source logs: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var sl model.SourceLog
			if err := rows.Scan(&sl.LogID, &sl.OrgID, &sl.PageID, &sl.SourceType, &sl.SourceRef, &sl.SyncStatus, &sl.Details, &sl.CreatedAt); err != nil {
				return fmt.Errorf("scan source log: %w", err)
			}
			logs = append(logs, sl)
		}
		return nil
	})
	if err != nil {
		return nil, 0, err
	}
	return logs, total, nil
}

func (r *WikiRepo) CreateMaintenanceLog(ctx context.Context, input model.CreateMaintenanceLogInput) (*model.MaintenanceLog, error) {
	id := uuid.New().String()
	details := input.Details
	if details == nil {
		details = json.RawMessage(`{}`)
	}

	var ml model.MaintenanceLog
	// Phase 1 RLS: insert and read-back share one scoped transaction. This
	// is also the per-item write behind MaintenanceSweep, which loops over a
	// batch — every item in a sweep carries the same verified org, so each
	// iteration re-enters the same scope rather than crossing tenants.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		result, err := tx.Exec(ctx, `
			INSERT INTO wiki_maintenance_logs (log_id, org_id, page_id, action, actor, details)
			SELECT $1, org_id, page_id, $4, $5, $6
			FROM wiki_pages
			WHERE org_id = $2 AND page_id = $3 AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, id, input.OrgID, input.PageID, input.Action, input.Actor, details)
		if err != nil {
			return fmt.Errorf("insert maintenance log: %w", err)
		}
		if result.RowsAffected() != 1 {
			return fmt.Errorf("insert maintenance log: %w", pgx.ErrNoRows)
		}

		if err := tx.QueryRow(ctx,
			"SELECT log_id, org_id, page_id, action, actor, details, created_at FROM wiki_maintenance_logs WHERE log_id = $1 AND org_id = $2", id, input.OrgID,
		).Scan(&ml.LogID, &ml.OrgID, &ml.PageID, &ml.Action, &ml.Actor, &ml.Details, &ml.CreatedAt); err != nil {
			return fmt.Errorf("read maintenance log: %w", err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &ml, nil
}

func (r *WikiRepo) ListMaintenanceLogs(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.MaintenanceLog, int, error) {
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	if offset < 0 {
		offset = 0
	}

	var total int
	var logs []model.MaintenanceLog

	// Phase 1 RLS: one scope covers both shapes — the whole-org listing and
	// the single-page listing are the same tenant either way.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		if pageID == "" {
			var err error
			logs, total, err = listOrgMaintenanceLogs(ctx, tx, orgID, limit, offset)
			return err
		}

		if err := tx.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_maintenance_logs AS log
			 JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
			 WHERE log.org_id = $1 AND log.page_id = $2 AND page.deleted_at IS NULL
			   AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'`,
			orgID, pageID).Scan(&total); err != nil {
			return fmt.Errorf("count maintenance logs: %w", err)
		}

		rows, err := tx.Query(ctx, `
			SELECT log.log_id, log.org_id, log.page_id, log.action, log.actor, log.details, log.created_at
			FROM wiki_maintenance_logs AS log
			JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
			WHERE log.org_id = $1 AND log.page_id = $2 AND page.deleted_at IS NULL
			  AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'
			ORDER BY log.created_at DESC LIMIT $3 OFFSET $4
		`, orgID, pageID, limit, offset)
		if err != nil {
			return fmt.Errorf("list maintenance logs: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var ml model.MaintenanceLog
			if err := rows.Scan(&ml.LogID, &ml.OrgID, &ml.PageID, &ml.Action, &ml.Actor, &ml.Details, &ml.CreatedAt); err != nil {
				return fmt.Errorf("scan maintenance log: %w", err)
			}
			logs = append(logs, ml)
		}
		return nil
	})
	if err != nil {
		return nil, 0, err
	}
	return logs, total, nil
}

// listOrgMaintenanceLogs runs inside an already-scoped transaction — see
// ListMaintenanceLogs, its only caller. Taking a pgx.Tx rather than the pool
// is what keeps it from being reachable unscoped.
func listOrgMaintenanceLogs(ctx context.Context, tx pgx.Tx, orgID string, limit, offset int) ([]model.MaintenanceLog, int, error) {
	var total int
	if err := tx.QueryRow(ctx,
		`SELECT COUNT(*) FROM wiki_maintenance_logs AS log
		 JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
		 WHERE log.org_id = $1 AND page.deleted_at IS NULL
		   AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'`,
		orgID,
	).Scan(&total); err != nil {
		return nil, 0, fmt.Errorf("count org maintenance logs: %w", err)
	}

	rows, err := tx.Query(ctx, `
		SELECT log.log_id, log.org_id, log.page_id, log.action, log.actor, log.details, log.created_at
		FROM wiki_maintenance_logs AS log
		JOIN wiki_pages AS page ON page.page_id = log.page_id AND page.org_id = log.org_id
		WHERE log.org_id = $1 AND page.deleted_at IS NULL
		  AND LOWER(COALESCE(page.page_status, '')) <> 'deleted'
		ORDER BY log.created_at DESC LIMIT $2 OFFSET $3
	`, orgID, limit, offset)
	if err != nil {
		return nil, 0, fmt.Errorf("list org maintenance logs: %w", err)
	}
	defer rows.Close()

	logs := make([]model.MaintenanceLog, 0, total)
	for rows.Next() {
		var maintenanceLog model.MaintenanceLog
		if err := rows.Scan(
			&maintenanceLog.LogID, &maintenanceLog.OrgID, &maintenanceLog.PageID,
			&maintenanceLog.Action, &maintenanceLog.Actor, &maintenanceLog.Details,
			&maintenanceLog.CreatedAt,
		); err != nil {
			return nil, 0, fmt.Errorf("scan org maintenance log: %w", err)
		}
		logs = append(logs, maintenanceLog)
	}
	return logs, total, rows.Err()
}

func (r *WikiRepo) GetBacklinks(ctx context.Context, orgID, pageID string) ([]model.WikiPage, error) {
	var pages []model.WikiPage

	// Phase 1 RLS: single-org backlink scan.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
			SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
			       backlinks, metadata, created_at, updated_at
			FROM wiki_pages
			WHERE org_id = $1 AND backlinks @> $2::jsonb AND deleted_at IS NULL
			  AND LOWER(COALESCE(page_status, '')) <> 'deleted'
		`, orgID, fmt.Sprintf(`["%s"]`, pageID))
		if err != nil {
			return fmt.Errorf("get backlinks: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var p model.WikiPage
			if err := rows.Scan(
				&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
				&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
			); err != nil {
				return fmt.Errorf("scan backlink page: %w", err)
			}
			pages = append(pages, p)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return pages, nil
}

func (r *WikiRepo) GetOperatingMapSnapshot(ctx context.Context, orgID string) (*model.OperatingMapSnapshot, error) {
	snapshot := &model.OperatingMapSnapshot{
		Proposals:            []model.OperatingMapProposal{},
		BlueprintSuggestions: []model.OperatingMapBlueprintSuggestion{},
	}

	// Phase 1 RLS: the map, its current version, its proposals and its
	// blueprint suggestions are all one org's — four reads that previously
	// took four pool round trips now share a single scoped transaction and
	// therefore a single consistent snapshot.
	err := orgscope.WithOrgScope(ctx, r.pool, orgID, func(tx pgx.Tx) error {
		opMap, err := getOperatingMap(ctx, tx, orgID)
		if err == pgx.ErrNoRows {
			// No operating map for this org yet — an empty snapshot, not an
			// error. Preserved verbatim from the unscoped implementation.
			return nil
		}
		if err != nil {
			return err
		}

		if opMap.CurrentVersionID != nil {
			version, err := getOperatingMapVersion(ctx, tx, orgID, *opMap.CurrentVersionID)
			if err != nil {
				return err
			}
			snapshot.Version = version
		}

		proposals, err := listOperatingMapProposals(ctx, tx, orgID, opMap.MapID, 20)
		if err != nil {
			return err
		}

		suggestions, err := listOperatingMapBlueprintSuggestions(ctx, tx, orgID, opMap.MapID, 50)
		if err != nil {
			return err
		}

		snapshot.Map = opMap
		snapshot.Proposals = proposals
		snapshot.BlueprintSuggestions = suggestions
		return nil
	})
	if err != nil {
		return nil, err
	}
	return snapshot, nil
}

func (r *WikiRepo) SubmitOperatingMapProposal(ctx context.Context, input model.SubmitOperatingMapProposalInput) (*model.OperatingMapProposal, error) {
	id := uuid.New().String()
	var proposal *model.OperatingMapProposal

	// Phase 1 RLS: ensure-map, insert-proposal and read-back are one org's
	// work and now one transaction. Statement order is unchanged from the
	// unscoped version; the difference is that a validation failure after
	// ensureOperatingMap now rolls the implicit draft map back instead of
	// leaving it behind.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		opMap, err := ensureOperatingMap(ctx, tx, input.OrgID, input.GeneratedFrom)
		if err != nil {
			return err
		}

		proposedVersion := input.ProposedVersion
		if len(proposedVersion) == 0 {
			proposedVersion = defaultOperatingMapVersion(input.GeneratedByRunID, input.EvidenceRefs)
		}
		if !json.Valid(proposedVersion) {
			return fmt.Errorf("proposed_version must be valid JSON")
		}

		refs, err := json.Marshal(input.EvidenceRefs)
		if err != nil {
			return fmt.Errorf("marshal evidence refs: %w", err)
		}

		if _, err := tx.Exec(ctx, `
			INSERT INTO operating_map_proposals (
				proposal_id, operating_map_id, org_id, proposed_version, evidence_refs,
				generated_by_run_id, proposal_status
			)
			VALUES ($1, $2, $3, $4, $5, $6, 'pending')
		`, id, opMap.MapID, input.OrgID, proposedVersion, json.RawMessage(refs), input.GeneratedByRunID); err != nil {
			return fmt.Errorf("insert operating map proposal: %w", err)
		}

		proposal, err = getOperatingMapProposal(ctx, tx, input.OrgID, id)
		return err
	})
	if err != nil {
		return nil, err
	}
	return proposal, nil
}

func (r *WikiRepo) RefreshOperatingMap(ctx context.Context, input model.RefreshOperatingMapInput) (*model.OperatingMapProposal, error) {
	runID := fmt.Sprintf("operating_map_%s", uuid.New().String())
	generatedFrom := input.GeneratedFrom
	if len(generatedFrom) == 0 {
		generatedFrom = json.RawMessage(`{"source":"knowledge-workspace","mode":"deterministic-fallback"}`)
	}
	// Phase 1 RLS: no direct queries here — both callees open their own
	// scope for input.OrgID, sequentially, so nothing nests.
	evidenceRefs, err := r.operatingMapEvidenceRefs(ctx, input.OrgID, 8)
	if err != nil {
		return nil, fmt.Errorf("load operating map evidence refs: %w", err)
	}
	return r.SubmitOperatingMapProposal(ctx, model.SubmitOperatingMapProposalInput{
		OrgID:            input.OrgID,
		GeneratedFrom:    generatedFrom,
		ProposedVersion:  defaultOperatingMapVersion(&runID, evidenceRefs),
		EvidenceRefs:     evidenceRefs,
		GeneratedByRunID: &runID,
	})
}

func (r *WikiRepo) ReviewOperatingMapProposal(ctx context.Context, input model.ReviewOperatingMapProposalInput) (*model.OperatingMapProposal, *model.OperatingMapVersion, error) {
	if input.Decision != "accept" && input.Decision != "reject" {
		return nil, nil, fmt.Errorf("decision must be accept or reject")
	}

	// Phase 1 RLS: the review keeps its original three phases — read the
	// proposal, mutate, re-read — rather than collapsing into one
	// transaction, because the mutation block was already its own committed
	// unit and the trailing reads deliberately observe it post-commit. Each
	// phase gets its own scope for the same org.
	var proposal *model.OperatingMapProposal
	if err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		var err error
		proposal, err = getOperatingMapProposal(ctx, tx, input.OrgID, input.ProposalID)
		return err
	}); err != nil {
		return nil, nil, err
	}

	status := "rejected"
	if input.Decision == "accept" {
		status = "accepted"
	}

	var versionID string
	// Phase 1 RLS: replaces this method's own Begin — same statements, same
	// single transaction, now org-enforced by the database as well.
	if err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `
			UPDATE operating_map_proposals
			SET proposal_status = $1, reviewed_by = $2, reviewed_at = NOW()
			WHERE proposal_id = $3 AND org_id = $4
		`, status, input.ReviewedBy, input.ProposalID, input.OrgID); err != nil {
			return fmt.Errorf("update operating map proposal: %w", err)
		}

		if input.Decision != "accept" {
			return nil
		}

		payload, err := parseOperatingMapVersion(proposal.ProposedVersion, proposal.EvidenceRefs, proposal.GeneratedByRunID)
		if err != nil {
			return err
		}

		versionID = uuid.New().String()
		if _, err := tx.Exec(ctx, `
			INSERT INTO operating_map_versions (
				version_id, operating_map_id, org_id, departments, workflows,
				agent_blueprints, rollout_phases, risk_overlays, learning_modules,
				roi_notes, evidence_refs, confidence, created_by_run_id
			)
			VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
		`, versionID, proposal.MapID, input.OrgID, payload.Departments, payload.Workflows,
			payload.AgentBlueprints, payload.RolloutPhases, payload.RiskOverlays,
			payload.LearningModules, payload.ROINotes, payload.EvidenceRefs,
			payload.Confidence, proposal.GeneratedByRunID); err != nil {
			return fmt.Errorf("insert operating map version: %w", err)
		}

		if _, err := tx.Exec(ctx, `
			UPDATE operating_maps
			SET current_version_id = $1, map_status = 'published', updated_at = NOW()
			WHERE operating_map_id = $2 AND org_id = $3
		`, versionID, proposal.MapID, input.OrgID); err != nil {
			return fmt.Errorf("update operating map current version: %w", err)
		}
		return nil
	}); err != nil {
		return nil, nil, err
	}

	var reviewed *model.OperatingMapProposal
	var version *model.OperatingMapVersion
	if err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		var err error
		reviewed, err = getOperatingMapProposal(ctx, tx, input.OrgID, input.ProposalID)
		if err != nil {
			return err
		}
		if input.Decision != "accept" {
			return nil
		}
		version, err = getOperatingMapVersion(ctx, tx, input.OrgID, versionID)
		return err
	}); err != nil {
		return reviewed, version, err
	}

	if input.Decision != "accept" {
		return reviewed, nil, nil
	}
	if err := r.publishOperatingMapWikiPage(ctx, input.OrgID, proposal.ProposedVersion); err != nil {
		return reviewed, version, err
	}
	return reviewed, version, nil
}

func (r *WikiRepo) CreateOperatingMapBlueprintSuggestion(ctx context.Context, input model.CreateOperatingMapBlueprintSuggestionInput) (*model.OperatingMapBlueprintSuggestion, error) {
	input.BlueprintID = strings.TrimSpace(input.BlueprintID)
	input.Role = strings.TrimSpace(input.Role)
	input.Name = strings.TrimSpace(input.Name)
	input.VersionID = strings.TrimSpace(input.VersionID)
	if input.BlueprintID == "" || input.Role == "" || input.Name == "" {
		return nil, fmt.Errorf("blueprint_id, role, and name are required")
	}
	if !validOperatingMapBlueprintRole(input.Role) {
		return nil, fmt.Errorf("unsupported blueprint role: %s", input.Role)
	}

	id := uuid.New().String()
	var suggestion *model.OperatingMapBlueprintSuggestion

	// Phase 1 RLS: map lookup, version check, upsert and read-back all belong
	// to input.OrgID, so they share one scope. Everything before the upsert
	// is a read, so the early error returns roll back nothing.
	err := orgscope.WithOrgScope(ctx, r.pool, input.OrgID, func(tx pgx.Tx) error {
		opMap, err := getOperatingMap(ctx, tx, input.OrgID)
		if err != nil {
			return err
		}
		if input.MapID != "" && input.MapID != opMap.MapID {
			return fmt.Errorf("operating map does not belong to org")
		}
		if input.VersionID == "" {
			if opMap.CurrentVersionID == nil || *opMap.CurrentVersionID == "" {
				return fmt.Errorf("accepted operating map version required")
			}
			input.VersionID = *opMap.CurrentVersionID
		}
		if _, err := getOperatingMapVersion(ctx, tx, input.OrgID, input.VersionID); err != nil {
			return err
		}

		payload := input.Payload
		if len(payload) == 0 {
			payload = json.RawMessage(`{}`)
		}
		if !json.Valid(payload) {
			return fmt.Errorf("payload must be valid JSON")
		}

		if _, err := tx.Exec(ctx, `
			INSERT INTO operating_map_blueprint_suggestions (
				suggestion_id, operating_map_id, version_id, org_id, blueprint_id,
				role, source_workflow_id, name, suggestion_status, requested_by, payload
			)
			VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, ''), $8, 'suggested', NULLIF($9, ''), $10)
			ON CONFLICT (org_id, version_id, blueprint_id) DO UPDATE SET
				role = EXCLUDED.role,
				source_workflow_id = EXCLUDED.source_workflow_id,
				name = EXCLUDED.name,
				requested_by = EXCLUDED.requested_by,
				payload = EXCLUDED.payload,
				updated_at = NOW()
		`, id, opMap.MapID, input.VersionID, input.OrgID, input.BlueprintID,
			input.Role, input.SourceWorkflowID, input.Name, input.RequestedBy, payload); err != nil {
			return fmt.Errorf("insert operating map blueprint suggestion: %w", err)
		}

		suggestion, err = getOperatingMapBlueprintSuggestion(ctx, tx, input.OrgID, input.VersionID, input.BlueprintID)
		return err
	})
	if err != nil {
		return nil, err
	}
	return suggestion, nil
}

// ─────────────────────────────────────────────────────────────────────────
// Scoped-transaction helpers.
//
// Every function below takes a pgx.Tx supplied by an orgscope.WithOrgScope
// callback above. None of them can reach the pool, so none of them can run
// unscoped — that is the point of the signature.
// ─────────────────────────────────────────────────────────────────────────

func ensureOperatingMap(ctx context.Context, tx pgx.Tx, orgID string, generatedFrom json.RawMessage) (*model.OperatingMap, error) {
	opMap, err := getOperatingMap(ctx, tx, orgID)
	if err == nil {
		return opMap, nil
	}
	if err != pgx.ErrNoRows {
		return nil, err
	}

	if len(generatedFrom) == 0 {
		generatedFrom = json.RawMessage(`{}`)
	}
	id := uuid.New().String()
	if _, err := tx.Exec(ctx, `
		INSERT INTO operating_maps (operating_map_id, org_id, map_status, generated_from)
		VALUES ($1, $2, 'draft', $3)
		ON CONFLICT (org_id) DO NOTHING
	`, id, orgID, generatedFrom); err != nil {
		return nil, fmt.Errorf("insert operating map: %w", err)
	}
	return getOperatingMap(ctx, tx, orgID)
}

func getOperatingMap(ctx context.Context, tx pgx.Tx, orgID string) (*model.OperatingMap, error) {
	var opMap model.OperatingMap
	err := tx.QueryRow(ctx, `
		SELECT operating_map_id, org_id, map_status, current_version_id,
		       generated_from, created_at, updated_at
		FROM operating_maps
		WHERE org_id = $1
	`, orgID).Scan(&opMap.MapID, &opMap.OrgID, &opMap.Status, &opMap.CurrentVersionID,
		&opMap.GeneratedFrom, &opMap.CreatedAt, &opMap.UpdatedAt)
	if err != nil {
		return nil, err
	}
	return &opMap, nil
}

func getOperatingMapVersion(ctx context.Context, tx pgx.Tx, orgID, versionID string) (*model.OperatingMapVersion, error) {
	var version model.OperatingMapVersion
	err := tx.QueryRow(ctx, `
		SELECT version_id, operating_map_id, org_id, departments, workflows,
		       agent_blueprints, rollout_phases, risk_overlays, learning_modules,
		       roi_notes, evidence_refs, confidence, created_by_run_id, created_at
		FROM operating_map_versions
		WHERE version_id = $1 AND org_id = $2
	`, versionID, orgID).Scan(&version.VersionID, &version.MapID, &version.OrgID,
		&version.Departments, &version.Workflows, &version.AgentBlueprints,
		&version.RolloutPhases, &version.RiskOverlays, &version.LearningModules,
		&version.ROINotes, &version.EvidenceRefs, &version.Confidence,
		&version.CreatedByRunID, &version.CreatedAt)
	if err != nil {
		return nil, err
	}
	return &version, nil
}

func getOperatingMapProposal(ctx context.Context, tx pgx.Tx, orgID, proposalID string) (*model.OperatingMapProposal, error) {
	var proposal model.OperatingMapProposal
	err := tx.QueryRow(ctx, `
		SELECT proposal_id, operating_map_id, org_id, proposed_version,
		       evidence_refs, generated_by_run_id, proposal_status,
		       reviewed_by, created_at, reviewed_at
		FROM operating_map_proposals
		WHERE proposal_id = $1 AND org_id = $2
	`, proposalID, orgID).Scan(&proposal.ProposalID, &proposal.MapID, &proposal.OrgID,
		&proposal.ProposedVersion, &proposal.EvidenceRefs, &proposal.GeneratedByRunID,
		&proposal.ProposalStatus, &proposal.ReviewedBy, &proposal.CreatedAt,
		&proposal.ReviewedAt)
	if err != nil {
		return nil, fmt.Errorf("get operating map proposal: %w", err)
	}
	return &proposal, nil
}

func listOperatingMapProposals(ctx context.Context, tx pgx.Tx, orgID, mapID string, limit int) ([]model.OperatingMapProposal, error) {
	if limit <= 0 || limit > 50 {
		limit = 20
	}
	rows, err := tx.Query(ctx, `
		SELECT proposal_id, operating_map_id, org_id, proposed_version,
		       evidence_refs, generated_by_run_id, proposal_status,
		       reviewed_by, created_at, reviewed_at
		FROM operating_map_proposals
		WHERE org_id = $1 AND operating_map_id = $2
		ORDER BY created_at DESC
		LIMIT $3
	`, orgID, mapID, limit)
	if err != nil {
		return nil, fmt.Errorf("list operating map proposals: %w", err)
	}
	defer rows.Close()

	var proposals []model.OperatingMapProposal
	for rows.Next() {
		var proposal model.OperatingMapProposal
		if err := rows.Scan(&proposal.ProposalID, &proposal.MapID, &proposal.OrgID,
			&proposal.ProposedVersion, &proposal.EvidenceRefs, &proposal.GeneratedByRunID,
			&proposal.ProposalStatus, &proposal.ReviewedBy, &proposal.CreatedAt,
			&proposal.ReviewedAt); err != nil {
			return nil, fmt.Errorf("scan operating map proposal: %w", err)
		}
		proposals = append(proposals, proposal)
	}
	return proposals, nil
}

func getOperatingMapBlueprintSuggestion(ctx context.Context, tx pgx.Tx, orgID, versionID, blueprintID string) (*model.OperatingMapBlueprintSuggestion, error) {
	var suggestion model.OperatingMapBlueprintSuggestion
	err := tx.QueryRow(ctx, `
		SELECT suggestion_id, operating_map_id, version_id, org_id, blueprint_id,
		       role, source_workflow_id, name, suggestion_status, requested_by,
		       payload, created_at, updated_at
		FROM operating_map_blueprint_suggestions
		WHERE org_id = $1 AND version_id = $2 AND blueprint_id = $3
	`, orgID, versionID, blueprintID).Scan(&suggestion.SuggestionID,
		&suggestion.MapID, &suggestion.VersionID, &suggestion.OrgID,
		&suggestion.BlueprintID, &suggestion.Role, &suggestion.SourceWorkflowID,
		&suggestion.Name, &suggestion.SuggestionStatus, &suggestion.RequestedBy,
		&suggestion.Payload, &suggestion.CreatedAt, &suggestion.UpdatedAt)
	if err != nil {
		return nil, fmt.Errorf("get operating map blueprint suggestion: %w", err)
	}
	return &suggestion, nil
}

func listOperatingMapBlueprintSuggestions(ctx context.Context, tx pgx.Tx, orgID, mapID string, limit int) ([]model.OperatingMapBlueprintSuggestion, error) {
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	rows, err := tx.Query(ctx, `
		SELECT suggestion_id, operating_map_id, version_id, org_id, blueprint_id,
		       role, source_workflow_id, name, suggestion_status, requested_by,
		       payload, created_at, updated_at
		FROM operating_map_blueprint_suggestions
		WHERE org_id = $1 AND operating_map_id = $2
		ORDER BY created_at DESC
		LIMIT $3
	`, orgID, mapID, limit)
	if err != nil {
		return nil, fmt.Errorf("list operating map blueprint suggestions: %w", err)
	}
	defer rows.Close()

	var suggestions []model.OperatingMapBlueprintSuggestion
	for rows.Next() {
		var suggestion model.OperatingMapBlueprintSuggestion
		if err := rows.Scan(&suggestion.SuggestionID, &suggestion.MapID,
			&suggestion.VersionID, &suggestion.OrgID, &suggestion.BlueprintID,
			&suggestion.Role, &suggestion.SourceWorkflowID, &suggestion.Name,
			&suggestion.SuggestionStatus, &suggestion.RequestedBy,
			&suggestion.Payload, &suggestion.CreatedAt, &suggestion.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan operating map blueprint suggestion: %w", err)
		}
		suggestions = append(suggestions, suggestion)
	}
	return suggestions, rows.Err()
}

type operatingMapVersionPayload struct {
	Departments     json.RawMessage
	Workflows       json.RawMessage
	AgentBlueprints json.RawMessage
	RolloutPhases   json.RawMessage
	RiskOverlays    json.RawMessage
	LearningModules json.RawMessage
	ROINotes        json.RawMessage
	EvidenceRefs    json.RawMessage
	Confidence      float64
}

func parseOperatingMapVersion(raw json.RawMessage, proposalRefs json.RawMessage, runID *string) (operatingMapVersionPayload, error) {
	var decoded map[string]json.RawMessage
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return operatingMapVersionPayload{}, fmt.Errorf("decode operating map version: %w", err)
	}

	confidence := 0.5
	if rawConfidence, ok := decoded["confidence"]; ok {
		if err := json.Unmarshal(rawConfidence, &confidence); err != nil {
			return operatingMapVersionPayload{}, fmt.Errorf("decode operating map confidence: %w", err)
		}
	}
	if confidence < 0 {
		confidence = 0
	}
	if confidence > 1 {
		confidence = 1
	}

	evidenceRefs := jsonField(decoded, "evidence_refs", proposalRefs)
	if len(evidenceRefs) == 0 {
		evidenceRefs = json.RawMessage(`[]`)
	}
	if _, ok := decoded["created_by_run_id"]; !ok && runID != nil {
		decoded["created_by_run_id"], _ = json.Marshal(*runID)
	}

	return operatingMapVersionPayload{
		Departments:     jsonField(decoded, "departments", json.RawMessage(`[]`)),
		Workflows:       jsonField(decoded, "workflows", json.RawMessage(`[]`)),
		AgentBlueprints: jsonField(decoded, "agent_blueprints", json.RawMessage(`[]`)),
		RolloutPhases:   jsonField(decoded, "rollout_phases", json.RawMessage(`[]`)),
		RiskOverlays:    jsonField(decoded, "risk_overlays", json.RawMessage(`[]`)),
		LearningModules: jsonField(decoded, "learning_modules", json.RawMessage(`[]`)),
		ROINotes:        jsonField(decoded, "roi_notes", json.RawMessage(`[]`)),
		EvidenceRefs:    evidenceRefs,
		Confidence:      confidence,
	}, nil
}

func jsonField(fields map[string]json.RawMessage, key string, fallback json.RawMessage) json.RawMessage {
	if value, ok := fields[key]; ok && len(value) > 0 && json.Valid(value) {
		return value
	}
	return fallback
}

func validOperatingMapBlueprintRole(role string) bool {
	switch role {
	case "service", "sales", "ecommerce", "chatbot", "workflow":
		return true
	default:
		return false
	}
}

func (r *WikiRepo) operatingMapEvidenceRefs(ctx context.Context, orgID string, limit int) ([]string, error) {
	// Phase 1 RLS: no direct queries — ListPages opens its own scope for
	// orgID and this runs entirely outside it.
	pages, _, err := r.ListPages(ctx, orgID, "", "", limit+1, 0)
	if err != nil {
		return nil, err
	}
	refs := make([]string, 0, limit)
	for _, page := range pages {
		if page.Path == "/operating-map" {
			continue
		}
		refs = append(refs, page.PageID)
		if len(refs) >= limit {
			break
		}
	}
	return refs, nil
}

func defaultOperatingMapVersion(runID *string, evidenceRefs []string) json.RawMessage {
	refs := append([]string(nil), evidenceRefs...)
	refAt := func(index int) []string {
		if len(refs) == 0 {
			return []string{}
		}
		return []string{refs[index%len(refs)]}
	}
	confidence := 0.42
	if len(refs) > 0 {
		confidence = 0.58
	}
	payload := map[string]any{
		"summary": "Evidence-grounded AI Operating Map proposal for this Verevon knowledge space.",
		"departments": []map[string]any{
			{"id": "support", "name": "Support", "confidence": confidence, "evidence_refs": refAt(0)},
			{"id": "sales", "name": "Sales", "confidence": confidence, "evidence_refs": refAt(1)},
			{"id": "operations", "name": "Operations", "confidence": confidence, "evidence_refs": refAt(2)},
			{"id": "leadership", "name": "Leadership", "confidence": confidence, "evidence_refs": refAt(3)},
		},
		"workflows": []map[string]any{
			{"id": "support-triage", "department_id": "support", "name": "Support triage", "phase": "Assist", "risk": "medium", "evidence_refs": refAt(0)},
			{"id": "sales-follow-up", "department_id": "sales", "name": "Sales follow-up", "phase": "Ground", "risk": "medium", "evidence_refs": refAt(1)},
			{"id": "knowledge-refresh", "department_id": "operations", "name": "Knowledge refresh", "phase": "Act", "risk": "low", "evidence_refs": refAt(2)},
		},
		"agent_blueprints": []map[string]any{
			{"id": "service-agent", "role": "service", "name": "Service agent", "source_workflow_id": "support-triage", "requires_approval": true},
			{"id": "sales-agent", "role": "sales", "name": "Sales agent", "source_workflow_id": "sales-follow-up", "requires_approval": true},
			{"id": "workflow-agent", "role": "workflow", "name": "Knowledge refresh workflow", "source_workflow_id": "knowledge-refresh", "requires_approval": true},
		},
		"rollout_phases": []map[string]any{
			{"id": "assist", "name": "Assist", "description": "Human copilots and low-risk productivity support."},
			{"id": "ground", "name": "Ground", "description": "Shared knowledge, retrieval, workflow memory, and decision support."},
			{"id": "act", "name": "Act", "description": "Approved autonomous or semi-autonomous agents."},
		},
		"risk_overlays": []map[string]any{
			{"id": "human-review", "label": "Human review required", "severity": "medium"},
			{"id": "evidence-required", "label": "Evidence required before rollout", "severity": "medium"},
		},
		"learning_modules": []map[string]any{
			{"id": "ai-operating-map", "title": "AI operating map review", "audience": "team leads"},
			{"id": "approval-patterns", "title": "Approval patterns for agent rollout", "audience": "operators"},
		},
		"roi_notes": []map[string]any{
			{"id": "triage-time", "label": "Triage time reduction", "measurement": "minutes saved per case"},
			{"id": "knowledge-freshness", "label": "Knowledge freshness", "measurement": "stale source count"},
		},
		"evidence_refs": refs,
		"confidence":    confidence,
	}
	if runID != nil {
		payload["created_by_run_id"] = *runID
	}
	encoded, _ := json.Marshal(payload)
	return encoded
}

func (r *WikiRepo) publishOperatingMapWikiPage(ctx context.Context, orgID string, proposedVersion json.RawMessage) error {
	content := operatingMapMarkdown(proposedVersion)
	// Phase 1 RLS: no direct queries — every callee opens its own scope for
	// orgID, sequentially. Deliberately not wrapped itself: doing so would
	// hold a scoped transaction open across nested WithOrgScope calls, each
	// of which checks out a second pool connection.
	page, err := r.GetPageByPath(ctx, orgID, "/operating-map")
	if err != nil {
		_, _, err = r.CreatePage(ctx, model.CreatePageInput{
			OrgID:          orgID,
			WorkspaceID:    "knowledge",
			Title:          "AI Operating Map",
			Path:           "/operating-map",
			InitialContent: content,
		})
		return err
	}
	_, err = r.CreateVersion(ctx, model.UpdateVersionInput{
		PageID:     page.PageID,
		OrgID:      orgID,
		NewContent: content,
		EditReason: "accepted operating map proposal",
	})
	return err
}

func operatingMapMarkdown(raw json.RawMessage) string {
	var pretty bytes.Buffer
	if err := json.Indent(&pretty, raw, "", "  "); err != nil {
		pretty.Write(raw)
	}
	return fmt.Sprintf("# AI Operating Map\n\nAccepted Verevon operating map generated from Knowledge evidence.\n\n```json\n%s\n```\n", pretty.String())
}
