package store

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Source mirrors one row of the sources table.
type Source struct {
	ID             uuid.UUID `json:"id"`
	OrganizationID string    `json:"organization_id"`
	TenantID       string    `json:"tenant_id"`
	SiteID         string    `json:"site_id"`
	SiteWebURL     string    `json:"site_web_url,omitempty"`
	DriveID        string    `json:"drive_id"`
	DriveName      string    `json:"drive_name,omitempty"`
	DriveType      string    `json:"drive_type,omitempty"`
	Enabled        bool      `json:"enabled"`
	CreatedAt      time.Time `json:"created_at"`
	UpdatedAt      time.Time `json:"updated_at"`
}

type Sources struct {
	pool *pgxpool.Pool
}

// EnsureSource inserts a (organization_id, drive_id) pair if absent, returning
// the resulting row. Existing rows are returned unchanged — the caller is
// responsible for applying updates explicitly.
func (s *Sources) EnsureSource(ctx context.Context, src Source) (Source, error) {
	const q = `
INSERT INTO sources
    (organization_id, tenant_id, site_id, site_web_url, drive_id, drive_name, drive_type, enabled)
VALUES ($1,$2,$3,$4,$5,$6,$7,COALESCE($8,TRUE))
ON CONFLICT (organization_id, drive_id) DO UPDATE
    SET site_id      = EXCLUDED.site_id,
        site_web_url = EXCLUDED.site_web_url,
        drive_name   = EXCLUDED.drive_name,
        drive_type   = EXCLUDED.drive_type,
        tenant_id    = COALESCE(NULLIF(EXCLUDED.tenant_id, ''), sources.tenant_id)
RETURNING id, organization_id, COALESCE(tenant_id, ''), site_id, COALESCE(site_web_url, ''),
          drive_id, COALESCE(drive_name, ''), COALESCE(drive_type, ''), enabled, created_at, updated_at`

	var out Source
	err := s.pool.QueryRow(ctx, q,
		src.OrganizationID,
		nullableString(src.TenantID),
		src.SiteID,
		nullableString(src.SiteWebURL),
		src.DriveID,
		nullableString(src.DriveName),
		nullableString(src.DriveType),
		src.Enabled,
	).Scan(
		&out.ID, &out.OrganizationID, &out.TenantID, &out.SiteID, &out.SiteWebURL,
		&out.DriveID, &out.DriveName, &out.DriveType, &out.Enabled,
		&out.CreatedAt, &out.UpdatedAt,
	)
	if err != nil {
		return Source{}, fmt.Errorf("ensure source: %w", err)
	}
	return out, nil
}

// Get fetches a source by primary key.
func (s *Sources) Get(ctx context.Context, id uuid.UUID) (Source, error) {
	const q = `
SELECT id, organization_id, COALESCE(tenant_id,''), site_id, COALESCE(site_web_url,''),
       drive_id, COALESCE(drive_name,''), COALESCE(drive_type,''), enabled,
       created_at, updated_at
  FROM sources
 WHERE id = $1`
	var out Source
	err := s.pool.QueryRow(ctx, q, id).Scan(
		&out.ID, &out.OrganizationID, &out.TenantID, &out.SiteID, &out.SiteWebURL,
		&out.DriveID, &out.DriveName, &out.DriveType, &out.Enabled,
		&out.CreatedAt, &out.UpdatedAt,
	)
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
	const q = `
SELECT id, organization_id, COALESCE(tenant_id,''), site_id, COALESCE(site_web_url,''),
       drive_id, COALESCE(drive_name,''), COALESCE(drive_type,''), enabled,
       created_at, updated_at
  FROM sources
 WHERE organization_id = $1
 ORDER BY created_at`
	rows, err := s.pool.Query(ctx, q, organizationID)
	if err != nil {
		return nil, fmt.Errorf("list sources: %w", err)
	}
	defer rows.Close()
	var out []Source
	for rows.Next() {
		var src Source
		if err := rows.Scan(
			&src.ID, &src.OrganizationID, &src.TenantID, &src.SiteID, &src.SiteWebURL,
			&src.DriveID, &src.DriveName, &src.DriveType, &src.Enabled,
			&src.CreatedAt, &src.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan source: %w", err)
		}
		out = append(out, src)
	}
	return out, rows.Err()
}

// ListEnabled returns every enabled source across all organizations — used by
// the background sync worker (Phase 2.x) when fanning out work.
func (s *Sources) ListEnabled(ctx context.Context) ([]Source, error) {
	const q = `
SELECT id, organization_id, COALESCE(tenant_id,''), site_id, COALESCE(site_web_url,''),
       drive_id, COALESCE(drive_name,''), COALESCE(drive_type,''), enabled,
       created_at, updated_at
  FROM sources
 WHERE enabled = TRUE
 ORDER BY updated_at`
	rows, err := s.pool.Query(ctx, q)
	if err != nil {
		return nil, fmt.Errorf("list enabled sources: %w", err)
	}
	defer rows.Close()
	var out []Source
	for rows.Next() {
		var src Source
		if err := rows.Scan(
			&src.ID, &src.OrganizationID, &src.TenantID, &src.SiteID, &src.SiteWebURL,
			&src.DriveID, &src.DriveName, &src.DriveType, &src.Enabled,
			&src.CreatedAt, &src.UpdatedAt,
		); err != nil {
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
