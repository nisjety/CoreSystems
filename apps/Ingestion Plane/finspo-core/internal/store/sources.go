package store

import (
	"context"
	"errors"
	"fmt"
	"path"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Source kinds. A "drive" source is the historical document-library delta
// sync (optionally scoped to one folder subtree); a "site_pages" source syncs
// a SharePoint site's pages via the Graph sitePages API.
const (
	SourceKindDrive     = "drive"
	SourceKindSitePages = "site_pages"
)

// Source mirrors one row of the sources table.
type Source struct {
	ID             uuid.UUID `json:"id"`
	OrganizationID string    `json:"organization_id"`
	TenantID       string    `json:"tenant_id"`
	Kind           string    `json:"kind"`
	SiteID         string    `json:"site_id"`
	SiteWebURL     string    `json:"site_web_url,omitempty"`
	DriveID        string    `json:"drive_id,omitempty"`
	DriveName      string    `json:"drive_name,omitempty"`
	DriveType      string    `json:"drive_type,omitempty"`
	FolderID       string    `json:"folder_id,omitempty"`
	FolderPath     string    `json:"folder_path,omitempty"`
	Enabled        bool      `json:"enabled"`
	CreatedAt      time.Time `json:"created_at"`
	UpdatedAt      time.Time `json:"updated_at"`
}

// NormalizeKind maps a raw kind string onto a known source kind. Empty input
// keeps the historical default ("drive") so pre-folder-scope callers and rows
// behave unchanged.
func NormalizeKind(kind string) string {
	trimmed := strings.TrimSpace(kind)
	if trimmed == "" {
		return SourceKindDrive
	}
	return trimmed
}

// NormalizeFolderPath canonicalizes a drive-root-relative folder path:
// leading slash, no trailing slash. "", "/", and "." all mean "whole
// library" and normalize to "".
func NormalizeFolderPath(folderPath string) string {
	trimmed := strings.TrimSpace(folderPath)
	if trimmed == "" {
		return ""
	}
	cleaned := path.Clean("/" + strings.TrimPrefix(trimmed, "/"))
	if cleaned == "/" || cleaned == "." {
		return ""
	}
	return cleaned
}

type Sources struct {
	pool *pgxpool.Pool
}

const sourceColumns = `id, organization_id, COALESCE(tenant_id,''), COALESCE(kind,'drive'),
       site_id, COALESCE(site_web_url,''),
       COALESCE(drive_id,''), COALESCE(drive_name,''), COALESCE(drive_type,''),
       COALESCE(folder_id,''), COALESCE(folder_path,''), enabled,
       created_at, updated_at`

func scanSource(row pgx.Row) (Source, error) {
	var out Source
	err := row.Scan(
		&out.ID, &out.OrganizationID, &out.TenantID, &out.Kind,
		&out.SiteID, &out.SiteWebURL,
		&out.DriveID, &out.DriveName, &out.DriveType,
		&out.FolderID, &out.FolderPath, &out.Enabled,
		&out.CreatedAt, &out.UpdatedAt,
	)
	return out, err
}

// EnsureSource inserts the source if absent, returning the resulting row.
// Uniqueness depends on the kind: drive sources conflict on
// (organization_id, drive_id, folder scope) so an org can register several
// disjoint folders of one drive, while site-pages sources conflict on
// (organization_id, site_id). Existing rows keep their enabled flag — the
// caller is responsible for applying updates explicitly.
func (s *Sources) EnsureSource(ctx context.Context, src Source) (Source, error) {
	src.Kind = NormalizeKind(src.Kind)
	src.FolderPath = NormalizeFolderPath(src.FolderPath)
	if src.Kind == SourceKindSitePages {
		return s.ensureSitePagesSource(ctx, src)
	}
	return s.ensureDriveSource(ctx, src)
}

func (s *Sources) ensureDriveSource(ctx context.Context, src Source) (Source, error) {
	const q = `
INSERT INTO sources
    (organization_id, tenant_id, kind, site_id, site_web_url, drive_id, drive_name, drive_type, folder_id, folder_path, enabled)
VALUES ($1,$2,'drive',$3,$4,$5,$6,$7,$8,$9,COALESCE($10,TRUE))
ON CONFLICT (organization_id, drive_id, COALESCE(folder_id, '')) WHERE kind = 'drive' DO UPDATE
    SET site_id      = EXCLUDED.site_id,
        site_web_url = EXCLUDED.site_web_url,
        drive_name   = EXCLUDED.drive_name,
        drive_type   = EXCLUDED.drive_type,
        folder_path  = EXCLUDED.folder_path,
        tenant_id    = COALESCE(NULLIF(EXCLUDED.tenant_id, ''), sources.tenant_id)
RETURNING ` + sourceColumns

	out, err := scanSource(s.pool.QueryRow(ctx, q,
		src.OrganizationID,
		nullableString(src.TenantID),
		src.SiteID,
		nullableString(src.SiteWebURL),
		src.DriveID,
		nullableString(src.DriveName),
		nullableString(src.DriveType),
		nullableString(src.FolderID),
		nullableString(src.FolderPath),
		src.Enabled,
	))
	if err != nil {
		return Source{}, fmt.Errorf("ensure source: %w", err)
	}
	return out, nil
}

func (s *Sources) ensureSitePagesSource(ctx context.Context, src Source) (Source, error) {
	// drive_name doubles as the display label for site-pages sources (the
	// existing UI and gateway label logic already fall back on it); the drive
	// columns proper stay NULL because there is no drive behind the sitePages
	// API.
	const q = `
INSERT INTO sources
    (organization_id, tenant_id, kind, site_id, site_web_url, drive_name, enabled)
VALUES ($1,$2,'site_pages',$3,$4,$5,COALESCE($6,TRUE))
ON CONFLICT (organization_id, site_id) WHERE kind = 'site_pages' DO UPDATE
    SET site_web_url = EXCLUDED.site_web_url,
        drive_name   = COALESCE(NULLIF(EXCLUDED.drive_name, ''), sources.drive_name),
        tenant_id    = COALESCE(NULLIF(EXCLUDED.tenant_id, ''), sources.tenant_id)
RETURNING ` + sourceColumns

	out, err := scanSource(s.pool.QueryRow(ctx, q,
		src.OrganizationID,
		nullableString(src.TenantID),
		src.SiteID,
		nullableString(src.SiteWebURL),
		nullableString(src.DriveName),
		src.Enabled,
	))
	if err != nil {
		return Source{}, fmt.Errorf("ensure site pages source: %w", err)
	}
	return out, nil
}

// Get fetches a source by primary key.
func (s *Sources) Get(ctx context.Context, id uuid.UUID) (Source, error) {
	q := `SELECT ` + sourceColumns + ` FROM sources WHERE id = $1`
	out, err := scanSource(s.pool.QueryRow(ctx, q, id))
	if errors.Is(err, pgx.ErrNoRows) {
		return Source{}, ErrNotFound
	}
	if err != nil {
		return Source{}, fmt.Errorf("get source: %w", err)
	}
	return out, nil
}

// ListByOrganization returns every source belonging to the given org.
func (s *Sources) ListByOrganization(ctx context.Context, organizationID string) ([]Source, error) {
	q := `SELECT ` + sourceColumns + ` FROM sources WHERE organization_id = $1 ORDER BY created_at`
	rows, err := s.pool.Query(ctx, q, organizationID)
	if err != nil {
		return nil, fmt.Errorf("list sources: %w", err)
	}
	defer rows.Close()
	var out []Source
	for rows.Next() {
		src, err := scanSource(rows)
		if err != nil {
			return nil, fmt.Errorf("scan source: %w", err)
		}
		out = append(out, src)
	}
	return out, rows.Err()
}

// ListEnabled returns every enabled source across all organizations — used by
// the background sync worker (Phase 2.x) when fanning out work.
func (s *Sources) ListEnabled(ctx context.Context) ([]Source, error) {
	q := `SELECT ` + sourceColumns + ` FROM sources WHERE enabled = TRUE ORDER BY updated_at`
	rows, err := s.pool.Query(ctx, q)
	if err != nil {
		return nil, fmt.Errorf("list enabled sources: %w", err)
	}
	defer rows.Close()
	var out []Source
	for rows.Next() {
		src, err := scanSource(rows)
		if err != nil {
			return nil, fmt.Errorf("scan source: %w", err)
		}
		out = append(out, src)
	}
	return out, rows.Err()
}

func nullableString(s string) any {
	if s == "" {
		return nil
	}
	return s
}
