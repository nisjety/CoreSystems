package repo

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/events"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
)

type WikiRepo struct {
	pool      *pgxpool.Pool
	publisher *events.Publisher
}

func NewWikiRepo(pool *pgxpool.Pool) *WikiRepo {
	return &WikiRepo{pool: pool}
}

// SetPublisher attaches a NATS event publisher to the repo. Optional —
// nil keeps the repo silent (used in tests). §16.3.8 wires this in
// cmd/main.go so every published version emits a NATS event that
// embedding-engine subscribes to.
func (r *WikiRepo) SetPublisher(p *events.Publisher) {
	r.publisher = p
}

// emitPublished is a best-effort hook called after CreatePage /
// CreateVersion commit. Failure logs but does not propagate — the
// version is already durable in Postgres.
func (r *WikiRepo) emitPublished(evt events.WikiVersionPublishedEvent) {
	if r.publisher == nil {
		return
	}
	if err := r.publisher.PublishWikiVersionPublished(evt); err != nil {
		fmt.Printf("warn: wiki publish event failed: %v\n", err)
	}
}

// ListPages — Wave 3.1 / Wave 11.C-b close: paginated wiki page enumeration
// for the velion sidebar (which previously had to fall back to localStorage
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

	// Four static query shapes — keeps tenant-isolation static check happy
	// and avoids any %s-formatted SQL. The branching is verbose but each
	// shape is unambiguous and audit-friendly.
	var total int
	var rows pgx.Rows
	var err error

	switch {
	case workspaceID != "" && status != "":
		err = r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND workspace_id = $2 AND page_status = $3 AND deleted_at IS NULL`,
			orgID, workspaceID, status,
		).Scan(&total)
		if err != nil {
			return nil, 0, fmt.Errorf("count wiki pages: %w", err)
		}
		rows, err = r.pool.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND workspace_id = $2 AND page_status = $3 AND deleted_at IS NULL
            ORDER BY updated_at DESC LIMIT $4 OFFSET $5
        `, orgID, workspaceID, status, limit, offset)
	case workspaceID != "":
		err = r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND workspace_id = $2 AND deleted_at IS NULL`,
			orgID, workspaceID,
		).Scan(&total)
		if err != nil {
			return nil, 0, fmt.Errorf("count wiki pages: %w", err)
		}
		rows, err = r.pool.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND workspace_id = $2 AND deleted_at IS NULL
            ORDER BY updated_at DESC LIMIT $3 OFFSET $4
        `, orgID, workspaceID, limit, offset)
	case status != "":
		err = r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_pages
             WHERE org_id = $1 AND page_status = $2 AND deleted_at IS NULL`,
			orgID, status,
		).Scan(&total)
		if err != nil {
			return nil, 0, fmt.Errorf("count wiki pages: %w", err)
		}
		rows, err = r.pool.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND page_status = $2 AND deleted_at IS NULL
            ORDER BY updated_at DESC LIMIT $3 OFFSET $4
        `, orgID, status, limit, offset)
	default:
		err = r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM wiki_pages WHERE org_id = $1 AND deleted_at IS NULL`,
			orgID,
		).Scan(&total)
		if err != nil {
			return nil, 0, fmt.Errorf("count wiki pages: %w", err)
		}
		rows, err = r.pool.Query(ctx, `
            SELECT page_id, org_id, workspace_id, title, path, current_version_id,
                   page_status, backlinks, metadata, created_at, updated_at
            FROM wiki_pages
            WHERE org_id = $1 AND deleted_at IS NULL
            ORDER BY updated_at DESC LIMIT $2 OFFSET $3
        `, orgID, limit, offset)
	}
	if err != nil {
		return nil, 0, fmt.Errorf("list wiki pages: %w", err)
	}
	defer rows.Close()

	var pages []model.WikiPage
	for rows.Next() {
		var p model.WikiPage
		if err := rows.Scan(
			&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path,
			&p.CurrentVersionID, &p.Status, &p.Backlinks, &p.Metadata,
			&p.CreatedAt, &p.UpdatedAt,
		); err != nil {
			return nil, 0, fmt.Errorf("scan wiki page: %w", err)
		}
		pages = append(pages, p)
	}
	return pages, total, nil
}

func (r *WikiRepo) GetPage(ctx context.Context, orgID, pageID string) (*model.WikiPage, error) {
	var p model.WikiPage
	err := r.pool.QueryRow(ctx, `
		SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
		       backlinks, metadata, created_at, updated_at
		FROM wiki_pages WHERE page_id = $1 AND org_id = $2
	`, pageID, orgID).Scan(
		&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
		&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("get page: %w", err)
	}
	return &p, nil
}

func (r *WikiRepo) GetPageByPath(ctx context.Context, orgID, path string) (*model.WikiPage, error) {
	var p model.WikiPage
	err := r.pool.QueryRow(ctx, `
		SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
		       backlinks, metadata, created_at, updated_at
		FROM wiki_pages WHERE org_id = $1 AND path = $2
	`, orgID, path).Scan(
		&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
		&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("get page by path: %w", err)
	}
	return &p, nil
}

func (r *WikiRepo) GetVersion(ctx context.Context, versionID string) (*model.WikiPageVersion, error) {
	var v model.WikiPageVersion
	err := r.pool.QueryRow(ctx, `
		SELECT version_id, page_id, content, source_refs, proposed_by_agent, proposed_by_user,
		       approved_by, edit_reason, version_status, metadata, created_at, published_at
		FROM wiki_page_versions WHERE version_id = $1
	`, versionID).Scan(
		&v.VersionID, &v.PageID, &v.Content, &v.SourceRefs, &v.ProposedByAgent,
		&v.ProposedByUser, &v.ApprovedBy, &v.EditReason, &v.VersionStatus,
		&v.Metadata, &v.CreatedAt, &v.PublishedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("get version: %w", err)
	}
	return &v, nil
}

func (r *WikiRepo) ListVersions(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.WikiPageVersion, int, error) {
	var total int
	err := r.pool.QueryRow(ctx,
		"SELECT COUNT(*) FROM wiki_page_versions v JOIN wiki_pages p ON v.page_id = p.page_id WHERE v.page_id = $1 AND p.org_id = $2",
		pageID, orgID).Scan(&total)
	if err != nil {
		return nil, 0, fmt.Errorf("count versions: %w", err)
	}

	rows, err := r.pool.Query(ctx, `
		SELECT v.version_id, v.page_id, v.content, v.source_refs, v.proposed_by_agent,
		       v.proposed_by_user, v.approved_by, v.edit_reason, v.version_status,
		       v.metadata, v.created_at, v.published_at
		FROM wiki_page_versions v JOIN wiki_pages p ON v.page_id = p.page_id
		WHERE v.page_id = $1 AND p.org_id = $2
		ORDER BY v.created_at DESC LIMIT $3 OFFSET $4
	`, pageID, orgID, limit, offset)
	if err != nil {
		return nil, 0, fmt.Errorf("list versions: %w", err)
	}
	defer rows.Close()

	var versions []model.WikiPageVersion
	for rows.Next() {
		var v model.WikiPageVersion
		if err := rows.Scan(
			&v.VersionID, &v.PageID, &v.Content, &v.SourceRefs, &v.ProposedByAgent,
			&v.ProposedByUser, &v.ApprovedBy, &v.EditReason, &v.VersionStatus,
			&v.Metadata, &v.CreatedAt, &v.PublishedAt,
		); err != nil {
			return nil, 0, fmt.Errorf("scan version: %w", err)
		}
		versions = append(versions, v)
	}
	return versions, total, nil
}

func (r *WikiRepo) CreatePage(ctx context.Context, input model.CreatePageInput) (*model.WikiPage, *model.WikiPageVersion, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, nil, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	pageID := uuid.New().String()
	versionID := uuid.New().String()
	now := time.Now()

	_, err = tx.Exec(ctx, `
		INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path, current_version_id, page_status)
		VALUES ($1, $2, $3, $4, $5, $6, 'published')
	`, pageID, input.OrgID, input.WorkspaceID, input.Title, input.Path, versionID)
	if err != nil {
		return nil, nil, fmt.Errorf("insert page: %w", err)
	}

	_, err = tx.Exec(ctx, `
		INSERT INTO wiki_page_versions (version_id, page_id, content, version_status, created_at, published_at)
		VALUES ($1, $2, $3, 'published', $4, $4)
	`, versionID, pageID, input.InitialContent, now)
	if err != nil {
		return nil, nil, fmt.Errorf("insert version: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, nil, fmt.Errorf("commit: %w", err)
	}

	page, _ := r.GetPage(ctx, input.OrgID, pageID)
	version, _ := r.GetVersion(ctx, versionID)

	// §16.3.8 — emit publish event so embedding-engine writes this version
	// into the wiki_block_embeddings Qdrant collection. Best-effort.
	r.emitPublished(events.WikiVersionPublishedEvent{
		PageID:      pageID,
		VersionID:   versionID,
		OrgID:       input.OrgID,
		WorkspaceID: input.WorkspaceID,
		Title:       input.Title,
		Path:        input.Path,
		Content:     input.InitialContent,
	})
	return page, version, nil
}

func (r *WikiRepo) CreateVersion(ctx context.Context, input model.UpdateVersionInput) (*model.WikiPageVersion, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	versionID := uuid.New().String()
	now := time.Now()

	_, err = tx.Exec(ctx, `
		INSERT INTO wiki_page_versions (version_id, page_id, content, edit_reason, proposed_by_user, version_status, created_at, published_at)
		VALUES ($1, $2, $3, $4, $5, 'published', $6, $6)
	`, versionID, input.PageID, input.NewContent, input.EditReason, input.ProposedBy, now)
	if err != nil {
		return nil, fmt.Errorf("insert version: %w", err)
	}

	_, err = tx.Exec(ctx,
		"UPDATE wiki_pages SET current_version_id = $1 WHERE page_id = $2 AND org_id = $3",
		versionID, input.PageID, input.OrgID)
	if err != nil {
		return nil, fmt.Errorf("update page version: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit: %w", err)
	}

	// §16.3.8 — emit publish event so embedding-engine reembeds this
	// version. We re-fetch the page to surface workspace_id / title for
	// the consumer; a fetch failure logs but doesn't block.
	if page, err := r.GetPage(ctx, input.OrgID, input.PageID); err == nil && page != nil {
		r.emitPublished(events.WikiVersionPublishedEvent{
			PageID:      input.PageID,
			VersionID:   versionID,
			OrgID:       input.OrgID,
			WorkspaceID: page.WorkspaceID,
			Title:       page.Title,
			Path:        page.Path,
			Content:     input.NewContent,
		})
	}

	return r.GetVersion(ctx, versionID)
}

func (r *WikiRepo) SubmitProposal(ctx context.Context, input model.SubmitProposalInput) (*model.WikiProposal, error) {
	id := uuid.New().String()
	refs, _ := json.Marshal(input.SourceRefs)

	_, err := r.pool.Exec(ctx, `
		INSERT INTO wiki_proposals (proposal_id, page_id, org_id, proposed_content, edit_reason, proposed_by_agent, source_refs, proposal_status)
		VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending')
	`, id, input.PageID, input.OrgID, input.ProposedContent, input.EditReason, input.ProposedByAgent, refs)
	if err != nil {
		return nil, fmt.Errorf("insert proposal: %w", err)
	}

	return r.GetProposal(ctx, input.OrgID, id)
}

func (r *WikiRepo) GetProposal(ctx context.Context, orgID, proposalID string) (*model.WikiProposal, error) {
	var p model.WikiProposal
	err := r.pool.QueryRow(ctx, `
		SELECT proposal_id, page_id, org_id, proposed_content, edit_reason, proposed_by_agent,
		       source_refs, proposal_status, reviewed_by, metadata, created_at
		FROM wiki_proposals WHERE proposal_id = $1 AND org_id = $2
	`, proposalID, orgID).Scan(
		&p.ProposalID, &p.PageID, &p.OrgID, &p.ProposedContent, &p.EditReason,
		&p.ProposedByAgent, &p.SourceRefs, &p.ProposalStatus, &p.ReviewedBy,
		&p.Metadata, &p.CreatedAt,
	)
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

	_, err = r.pool.Exec(ctx,
		"UPDATE wiki_proposals SET proposal_status = $1, reviewed_by = $2 WHERE proposal_id = $3",
		status, input.ReviewedBy, input.ProposalID)
	if err != nil {
		return nil, nil, fmt.Errorf("update proposal status: %w", err)
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
	_, err := r.pool.Exec(ctx, `
		INSERT INTO wiki_source_logs (log_id, org_id, page_id, source_type, source_ref, sync_status, details)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
	`, id, input.OrgID, input.PageID, input.SourceType, input.SourceRef, input.SyncStatus, details)
	if err != nil {
		return nil, fmt.Errorf("insert source log: %w", err)
	}

	var sl model.SourceLog
	err = r.pool.QueryRow(ctx,
		"SELECT log_id, org_id, page_id, source_type, source_ref, sync_status, details, created_at FROM wiki_source_logs WHERE log_id = $1", id,
	).Scan(&sl.LogID, &sl.OrgID, &sl.PageID, &sl.SourceType, &sl.SourceRef, &sl.SyncStatus, &sl.Details, &sl.CreatedAt)
	if err != nil {
		return nil, fmt.Errorf("read source log: %w", err)
	}
	return &sl, nil
}

func (r *WikiRepo) ListSourceLogs(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.SourceLog, int, error) {
	var total int
	err := r.pool.QueryRow(ctx,
		"SELECT COUNT(*) FROM wiki_source_logs WHERE org_id = $1 AND page_id = $2",
		orgID, pageID).Scan(&total)
	if err != nil {
		return nil, 0, fmt.Errorf("count source logs: %w", err)
	}

	rows, err := r.pool.Query(ctx, `
		SELECT log_id, org_id, page_id, source_type, source_ref, sync_status, details, created_at
		FROM wiki_source_logs WHERE org_id = $1 AND page_id = $2
		ORDER BY created_at DESC LIMIT $3 OFFSET $4
	`, orgID, pageID, limit, offset)
	if err != nil {
		return nil, 0, fmt.Errorf("list source logs: %w", err)
	}
	defer rows.Close()

	var logs []model.SourceLog
	for rows.Next() {
		var sl model.SourceLog
		if err := rows.Scan(&sl.LogID, &sl.OrgID, &sl.PageID, &sl.SourceType, &sl.SourceRef, &sl.SyncStatus, &sl.Details, &sl.CreatedAt); err != nil {
			return nil, 0, fmt.Errorf("scan source log: %w", err)
		}
		logs = append(logs, sl)
	}
	return logs, total, nil
}

func (r *WikiRepo) CreateMaintenanceLog(ctx context.Context, input model.CreateMaintenanceLogInput) (*model.MaintenanceLog, error) {
	id := uuid.New().String()
	details := input.Details
	if details == nil {
		details = json.RawMessage(`{}`)
	}
	_, err := r.pool.Exec(ctx, `
		INSERT INTO wiki_maintenance_logs (log_id, org_id, page_id, action, actor, details)
		VALUES ($1, $2, $3, $4, $5, $6)
	`, id, input.OrgID, input.PageID, input.Action, input.Actor, details)
	if err != nil {
		return nil, fmt.Errorf("insert maintenance log: %w", err)
	}

	var ml model.MaintenanceLog
	err = r.pool.QueryRow(ctx,
		"SELECT log_id, org_id, page_id, action, actor, details, created_at FROM wiki_maintenance_logs WHERE log_id = $1", id,
	).Scan(&ml.LogID, &ml.OrgID, &ml.PageID, &ml.Action, &ml.Actor, &ml.Details, &ml.CreatedAt)
	if err != nil {
		return nil, fmt.Errorf("read maintenance log: %w", err)
	}
	return &ml, nil
}

func (r *WikiRepo) ListMaintenanceLogs(ctx context.Context, orgID, pageID string, limit, offset int) ([]model.MaintenanceLog, int, error) {
	var total int
	err := r.pool.QueryRow(ctx,
		"SELECT COUNT(*) FROM wiki_maintenance_logs WHERE org_id = $1 AND page_id = $2",
		orgID, pageID).Scan(&total)
	if err != nil {
		return nil, 0, fmt.Errorf("count maintenance logs: %w", err)
	}

	rows, err := r.pool.Query(ctx, `
		SELECT log_id, org_id, page_id, action, actor, details, created_at
		FROM wiki_maintenance_logs WHERE org_id = $1 AND page_id = $2
		ORDER BY created_at DESC LIMIT $3 OFFSET $4
	`, orgID, pageID, limit, offset)
	if err != nil {
		return nil, 0, fmt.Errorf("list maintenance logs: %w", err)
	}
	defer rows.Close()

	var logs []model.MaintenanceLog
	for rows.Next() {
		var ml model.MaintenanceLog
		if err := rows.Scan(&ml.LogID, &ml.OrgID, &ml.PageID, &ml.Action, &ml.Actor, &ml.Details, &ml.CreatedAt); err != nil {
			return nil, 0, fmt.Errorf("scan maintenance log: %w", err)
		}
		logs = append(logs, ml)
	}
	return logs, total, nil
}

func (r *WikiRepo) GetBacklinks(ctx context.Context, orgID, pageID string) ([]model.WikiPage, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT page_id, org_id, workspace_id, title, path, current_version_id, page_status,
		       backlinks, metadata, created_at, updated_at
		FROM wiki_pages
		WHERE org_id = $1 AND backlinks @> $2::jsonb
	`, orgID, fmt.Sprintf(`["%s"]`, pageID))
	if err != nil {
		return nil, fmt.Errorf("get backlinks: %w", err)
	}
	defer rows.Close()

	var pages []model.WikiPage
	for rows.Next() {
		var p model.WikiPage
		if err := rows.Scan(
			&p.PageID, &p.OrgID, &p.WorkspaceID, &p.Title, &p.Path, &p.CurrentVersionID,
			&p.Status, &p.Backlinks, &p.Metadata, &p.CreatedAt, &p.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan backlink page: %w", err)
		}
		pages = append(pages, p)
	}
	return pages, nil
}
