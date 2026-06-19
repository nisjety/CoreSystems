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

func (r *WikiRepo) GetOperatingMapSnapshot(ctx context.Context, orgID string) (*model.OperatingMapSnapshot, error) {
	opMap, err := r.getOperatingMap(ctx, orgID)
	if err == pgx.ErrNoRows {
		return &model.OperatingMapSnapshot{
			Proposals:            []model.OperatingMapProposal{},
			BlueprintSuggestions: []model.OperatingMapBlueprintSuggestion{},
		}, nil
	}
	if err != nil {
		return nil, err
	}

	var version *model.OperatingMapVersion
	if opMap.CurrentVersionID != nil {
		version, err = r.getOperatingMapVersion(ctx, orgID, *opMap.CurrentVersionID)
		if err != nil {
			return nil, err
		}
	}

	proposals, err := r.listOperatingMapProposals(ctx, orgID, opMap.MapID, 20)
	if err != nil {
		return nil, err
	}

	suggestions, err := r.listOperatingMapBlueprintSuggestions(ctx, orgID, opMap.MapID, 50)
	if err != nil {
		return nil, err
	}

	return &model.OperatingMapSnapshot{
		Map:                  opMap,
		Version:              version,
		Proposals:            proposals,
		BlueprintSuggestions: suggestions,
	}, nil
}

func (r *WikiRepo) SubmitOperatingMapProposal(ctx context.Context, input model.SubmitOperatingMapProposalInput) (*model.OperatingMapProposal, error) {
	opMap, err := r.ensureOperatingMap(ctx, input.OrgID, input.GeneratedFrom)
	if err != nil {
		return nil, err
	}

	proposedVersion := input.ProposedVersion
	if len(proposedVersion) == 0 {
		proposedVersion = defaultOperatingMapVersion(input.GeneratedByRunID, input.EvidenceRefs)
	}
	if !json.Valid(proposedVersion) {
		return nil, fmt.Errorf("proposed_version must be valid JSON")
	}

	refs, err := json.Marshal(input.EvidenceRefs)
	if err != nil {
		return nil, fmt.Errorf("marshal evidence refs: %w", err)
	}

	id := uuid.New().String()
	_, err = r.pool.Exec(ctx, `
		INSERT INTO operating_map_proposals (
			proposal_id, operating_map_id, org_id, proposed_version, evidence_refs,
			generated_by_run_id, proposal_status
		)
		VALUES ($1, $2, $3, $4, $5, $6, 'pending')
	`, id, opMap.MapID, input.OrgID, proposedVersion, json.RawMessage(refs), input.GeneratedByRunID)
	if err != nil {
		return nil, fmt.Errorf("insert operating map proposal: %w", err)
	}

	return r.getOperatingMapProposal(ctx, input.OrgID, id)
}

func (r *WikiRepo) RefreshOperatingMap(ctx context.Context, input model.RefreshOperatingMapInput) (*model.OperatingMapProposal, error) {
	runID := fmt.Sprintf("operating_map_%s", uuid.New().String())
	generatedFrom := input.GeneratedFrom
	if len(generatedFrom) == 0 {
		generatedFrom = json.RawMessage(`{"source":"knowledge-workspace","mode":"deterministic-fallback"}`)
	}
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

	proposal, err := r.getOperatingMapProposal(ctx, input.OrgID, input.ProposalID)
	if err != nil {
		return nil, nil, err
	}

	status := "rejected"
	if input.Decision == "accept" {
		status = "accepted"
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, nil, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	_, err = tx.Exec(ctx, `
		UPDATE operating_map_proposals
		SET proposal_status = $1, reviewed_by = $2, reviewed_at = NOW()
		WHERE proposal_id = $3 AND org_id = $4
	`, status, input.ReviewedBy, input.ProposalID, input.OrgID)
	if err != nil {
		return nil, nil, fmt.Errorf("update operating map proposal: %w", err)
	}

	var versionID string
	if input.Decision == "accept" {
		payload, err := parseOperatingMapVersion(proposal.ProposedVersion, proposal.EvidenceRefs, proposal.GeneratedByRunID)
		if err != nil {
			return nil, nil, err
		}

		versionID = uuid.New().String()
		_, err = tx.Exec(ctx, `
			INSERT INTO operating_map_versions (
				version_id, operating_map_id, org_id, departments, workflows,
				agent_blueprints, rollout_phases, risk_overlays, learning_modules,
				roi_notes, evidence_refs, confidence, created_by_run_id
			)
			VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
		`, versionID, proposal.MapID, input.OrgID, payload.Departments, payload.Workflows,
			payload.AgentBlueprints, payload.RolloutPhases, payload.RiskOverlays,
			payload.LearningModules, payload.ROINotes, payload.EvidenceRefs,
			payload.Confidence, proposal.GeneratedByRunID)
		if err != nil {
			return nil, nil, fmt.Errorf("insert operating map version: %w", err)
		}

		_, err = tx.Exec(ctx, `
			UPDATE operating_maps
			SET current_version_id = $1, map_status = 'published', updated_at = NOW()
			WHERE operating_map_id = $2 AND org_id = $3
		`, versionID, proposal.MapID, input.OrgID)
		if err != nil {
			return nil, nil, fmt.Errorf("update operating map current version: %w", err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, nil, fmt.Errorf("commit: %w", err)
	}

	reviewed, err := r.getOperatingMapProposal(ctx, input.OrgID, input.ProposalID)
	if err != nil {
		return nil, nil, err
	}

	if input.Decision != "accept" {
		return reviewed, nil, nil
	}

	version, err := r.getOperatingMapVersion(ctx, input.OrgID, versionID)
	if err != nil {
		return reviewed, nil, err
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

	opMap, err := r.getOperatingMap(ctx, input.OrgID)
	if err != nil {
		return nil, err
	}
	if input.MapID != "" && input.MapID != opMap.MapID {
		return nil, fmt.Errorf("operating map does not belong to org")
	}
	if input.VersionID == "" {
		if opMap.CurrentVersionID == nil || *opMap.CurrentVersionID == "" {
			return nil, fmt.Errorf("accepted operating map version required")
		}
		input.VersionID = *opMap.CurrentVersionID
	}
	if _, err := r.getOperatingMapVersion(ctx, input.OrgID, input.VersionID); err != nil {
		return nil, err
	}

	payload := input.Payload
	if len(payload) == 0 {
		payload = json.RawMessage(`{}`)
	}
	if !json.Valid(payload) {
		return nil, fmt.Errorf("payload must be valid JSON")
	}

	id := uuid.New().String()
	_, err = r.pool.Exec(ctx, `
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
		input.Role, input.SourceWorkflowID, input.Name, input.RequestedBy, payload)
	if err != nil {
		return nil, fmt.Errorf("insert operating map blueprint suggestion: %w", err)
	}

	return r.getOperatingMapBlueprintSuggestion(ctx, input.OrgID, input.VersionID, input.BlueprintID)
}

func (r *WikiRepo) ensureOperatingMap(ctx context.Context, orgID string, generatedFrom json.RawMessage) (*model.OperatingMap, error) {
	opMap, err := r.getOperatingMap(ctx, orgID)
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
	_, err = r.pool.Exec(ctx, `
		INSERT INTO operating_maps (operating_map_id, org_id, map_status, generated_from)
		VALUES ($1, $2, 'draft', $3)
		ON CONFLICT (org_id) DO NOTHING
	`, id, orgID, generatedFrom)
	if err != nil {
		return nil, fmt.Errorf("insert operating map: %w", err)
	}
	return r.getOperatingMap(ctx, orgID)
}

func (r *WikiRepo) getOperatingMap(ctx context.Context, orgID string) (*model.OperatingMap, error) {
	var opMap model.OperatingMap
	err := r.pool.QueryRow(ctx, `
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

func (r *WikiRepo) getOperatingMapVersion(ctx context.Context, orgID, versionID string) (*model.OperatingMapVersion, error) {
	var version model.OperatingMapVersion
	err := r.pool.QueryRow(ctx, `
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

func (r *WikiRepo) getOperatingMapProposal(ctx context.Context, orgID, proposalID string) (*model.OperatingMapProposal, error) {
	var proposal model.OperatingMapProposal
	err := r.pool.QueryRow(ctx, `
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

func (r *WikiRepo) listOperatingMapProposals(ctx context.Context, orgID, mapID string, limit int) ([]model.OperatingMapProposal, error) {
	if limit <= 0 || limit > 50 {
		limit = 20
	}
	rows, err := r.pool.Query(ctx, `
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

func (r *WikiRepo) getOperatingMapBlueprintSuggestion(ctx context.Context, orgID, versionID, blueprintID string) (*model.OperatingMapBlueprintSuggestion, error) {
	var suggestion model.OperatingMapBlueprintSuggestion
	err := r.pool.QueryRow(ctx, `
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

func (r *WikiRepo) listOperatingMapBlueprintSuggestions(ctx context.Context, orgID, mapID string, limit int) ([]model.OperatingMapBlueprintSuggestion, error) {
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	rows, err := r.pool.Query(ctx, `
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
		"summary": "Evidence-grounded AI Operating Map proposal for this Velion knowledge space.",
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
	return fmt.Sprintf("# AI Operating Map\n\nAccepted Velion operating map generated from Knowledge evidence.\n\n```json\n%s\n```\n", pretty.String())
}
