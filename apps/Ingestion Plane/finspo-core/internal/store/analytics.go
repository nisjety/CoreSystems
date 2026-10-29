package store

import (
	"context"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// LargestItem is one row of the "largest live files" report.
type LargestItem struct {
	ItemPK     uuid.UUID  `json:"item_pk"`
	SourceID   uuid.UUID  `json:"source_id"`
	ItemID     string     `json:"item_id"`
	Path       string     `json:"path"`
	Name       string     `json:"name"`
	SizeBytes  int64      `json:"size_bytes"`
	ModifiedAt *time.Time `json:"modified_at,omitempty"`
	WebURL     string     `json:"web_url,omitempty"`
}

// InactiveItem is one row of the "inactive files" report.
type InactiveItem struct {
	ItemPK         uuid.UUID  `json:"item_pk"`
	SourceID       uuid.UUID  `json:"source_id"`
	ItemID         string     `json:"item_id"`
	Path           string     `json:"path"`
	Name           string     `json:"name"`
	SizeBytes      int64      `json:"size_bytes"`
	ModifiedAt     *time.Time `json:"modified_at,omitempty"`
	DaysSinceTouch int64      `json:"days_since_touch"`
	WebURL         string     `json:"web_url,omitempty"`
}

// SiteAggregate summarizes the live footprint of one site/drive for an org.
type SiteAggregate struct {
	SourceID   uuid.UUID `json:"source_id"`
	SiteID     string    `json:"site_id"`
	DriveID    string    `json:"drive_id"`
	DriveName  string    `json:"drive_name,omitempty"`
	FileCount  int64     `json:"file_count"`
	TotalBytes int64     `json:"total_bytes"`
}

// DuplicateGroup describes one set of live files that share a content hash.
type DuplicateGroup struct {
	HashKind   string             `json:"hash_kind"` // "sha1" | "quickxor"
	HashValue  string             `json:"hash_value"`
	Count      int                `json:"count"`
	TotalBytes int64              `json:"total_bytes"`
	Members    []DuplicateMember  `json:"members"`
}

type DuplicateMember struct {
	ItemPK     uuid.UUID  `json:"item_pk"`
	SourceID   uuid.UUID  `json:"source_id"`
	ItemID     string     `json:"item_id"`
	Path       string     `json:"path"`
	Name       string     `json:"name"`
	SizeBytes  int64      `json:"size_bytes"`
	ModifiedAt *time.Time `json:"modified_at,omitempty"`
	WebURL     string     `json:"web_url,omitempty"`
}

type Analytics struct {
	pool *pgxpool.Pool
}

// Largest returns the largest live files for an organization, ordered by size
// descending. limit is capped by the caller; minSizeBytes filters out small
// files.
func (a *Analytics) Largest(ctx context.Context, organizationID string, limit int, minSizeBytes int64) ([]LargestItem, error) {
	const q = `
SELECT id, source_id, item_id, path, name,
       COALESCE(size_bytes, 0), modified_at, COALESCE(web_url, '')
  FROM items
 WHERE organization_id = $1
   AND deleted_at IS NULL
   AND is_folder = FALSE
   AND COALESCE(size_bytes, 0) >= $2
 ORDER BY size_bytes DESC NULLS LAST
 LIMIT $3`
	rows, err := a.pool.Query(ctx, q, organizationID, minSizeBytes, limit)
	if err != nil {
		return nil, fmt.Errorf("largest items: %w", err)
	}
	defer rows.Close()
	var out []LargestItem
	for rows.Next() {
		var r LargestItem
		if err := rows.Scan(&r.ItemPK, &r.SourceID, &r.ItemID, &r.Path, &r.Name,
			&r.SizeBytes, &r.ModifiedAt, &r.WebURL); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Inactive returns live files whose modified_at is older than the cutoff.
// olderThan is a duration ("days_since_touch >= olderThan") evaluated server-side.
func (a *Analytics) Inactive(ctx context.Context, organizationID string, olderThan time.Duration, limit int) ([]InactiveItem, error) {
	const q = `
SELECT id, source_id, item_id, path, name,
       COALESCE(size_bytes, 0), modified_at,
       EXTRACT(EPOCH FROM (NOW() - modified_at))::bigint / 86400 AS days_since_touch,
       COALESCE(web_url, '')
  FROM items
 WHERE organization_id = $1
   AND deleted_at IS NULL
   AND is_folder = FALSE
   AND modified_at IS NOT NULL
   AND modified_at < NOW() - $2::interval
 ORDER BY modified_at ASC
 LIMIT $3`

	interval := fmt.Sprintf("%d seconds", int64(olderThan.Seconds()))
	rows, err := a.pool.Query(ctx, q, organizationID, interval, limit)
	if err != nil {
		return nil, fmt.Errorf("inactive items: %w", err)
	}
	defer rows.Close()
	var out []InactiveItem
	for rows.Next() {
		var r InactiveItem
		if err := rows.Scan(&r.ItemPK, &r.SourceID, &r.ItemID, &r.Path, &r.Name,
			&r.SizeBytes, &r.ModifiedAt, &r.DaysSinceTouch, &r.WebURL); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// BySite returns one aggregate row per (source) drive for the organization.
func (a *Analytics) BySite(ctx context.Context, organizationID string) ([]SiteAggregate, error) {
	const q = `
SELECT s.id, s.site_id, s.drive_id, COALESCE(s.drive_name, ''),
       COUNT(i.id) FILTER (WHERE i.deleted_at IS NULL AND i.is_folder = FALSE) AS file_count,
       COALESCE(SUM(i.size_bytes) FILTER (WHERE i.deleted_at IS NULL AND i.is_folder = FALSE), 0) AS total_bytes
  FROM sources s
  LEFT JOIN items i ON i.source_id = s.id
 WHERE s.organization_id = $1
 GROUP BY s.id, s.site_id, s.drive_id, s.drive_name
 ORDER BY total_bytes DESC`
	rows, err := a.pool.Query(ctx, q, organizationID)
	if err != nil {
		return nil, fmt.Errorf("by-site: %w", err)
	}
	defer rows.Close()
	var out []SiteAggregate
	for rows.Next() {
		var r SiteAggregate
		if err := rows.Scan(&r.SourceID, &r.SiteID, &r.DriveID, &r.DriveName,
			&r.FileCount, &r.TotalBytes); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Duplicates returns groups of live files that share the same content hash.
// minCount filters out singletons (default: 2). minSizeBytes lets callers
// ignore trivially small files. Each member is returned inline; if you need
// per-group pagination, add OFFSET/LIMIT later.
func (a *Analytics) Duplicates(ctx context.Context, organizationID string, minCount int, minSizeBytes int64, maxGroups int) ([]DuplicateGroup, error) {
	if minCount < 2 {
		minCount = 2
	}
	if maxGroups <= 0 {
		maxGroups = 100
	}

	const q = `
WITH live AS (
    SELECT id, source_id, item_id, path, name, size_bytes, modified_at, web_url,
           CASE
               WHEN sha1_hash       <> '' THEN 'sha1:' || sha1_hash
               WHEN quick_xor_hash  <> '' THEN 'quickxor:' || quick_xor_hash
               ELSE NULL
           END AS content_hash
      FROM items
     WHERE organization_id = $1
       AND deleted_at IS NULL
       AND is_folder = FALSE
       AND COALESCE(size_bytes, 0) >= $3
),
groups AS (
    SELECT content_hash,
           COUNT(*)              AS group_count,
           SUM(size_bytes)::bigint AS total_bytes
      FROM live
     WHERE content_hash IS NOT NULL
     GROUP BY content_hash
    HAVING COUNT(*) >= $2
     ORDER BY total_bytes DESC
     LIMIT $4
)
SELECT g.content_hash, g.group_count, g.total_bytes,
       l.id, l.source_id, l.item_id, l.path, l.name,
       COALESCE(l.size_bytes, 0), l.modified_at, COALESCE(l.web_url, '')
  FROM groups g
  JOIN live l USING (content_hash)
 ORDER BY g.total_bytes DESC, g.content_hash, l.modified_at`

	rows, err := a.pool.Query(ctx, q, organizationID, minCount, minSizeBytes, maxGroups)
	if err != nil {
		return nil, fmt.Errorf("duplicates: %w", err)
	}
	defer rows.Close()

	byHash := make(map[string]*DuplicateGroup)
	var order []string
	for rows.Next() {
		var (
			contentHash string
			count       int
			totalBytes  int64
			m           DuplicateMember
		)
		if err := rows.Scan(&contentHash, &count, &totalBytes,
			&m.ItemPK, &m.SourceID, &m.ItemID, &m.Path, &m.Name,
			&m.SizeBytes, &m.ModifiedAt, &m.WebURL); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		grp, ok := byHash[contentHash]
		if !ok {
			kind, value := splitContentHash(contentHash)
			grp = &DuplicateGroup{
				HashKind:   kind,
				HashValue:  value,
				Count:      count,
				TotalBytes: totalBytes,
			}
			byHash[contentHash] = grp
			order = append(order, contentHash)
		}
		grp.Members = append(grp.Members, m)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	out := make([]DuplicateGroup, 0, len(order))
	for _, h := range order {
		out = append(out, *byHash[h])
	}
	return out, nil
}

// splitContentHash splits "kind:value" into the two parts; falls back to
// ("", contentHash) if no colon is present.
func splitContentHash(hash string) (kind, value string) {
	for i := 0; i < len(hash); i++ {
		if hash[i] == ':' {
			return hash[:i], hash[i+1:]
		}
	}
	return "", hash
}
