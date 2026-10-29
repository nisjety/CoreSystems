package store

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/finspo/internal/sharepoint"
)

// Item is the in-memory projection of one row in the items table.
type Item struct {
	ID             uuid.UUID  `json:"id"`
	SourceID       uuid.UUID  `json:"source_id"`
	OrganizationID string     `json:"organization_id"`
	ItemID         string     `json:"item_id"`
	ParentItemID   string     `json:"parent_item_id,omitempty"`
	Path           string     `json:"path"`
	Name           string     `json:"name"`
	MimeType       string     `json:"mime_type,omitempty"`
	SizeBytes      int64      `json:"size_bytes"`
	IsFolder       bool       `json:"is_folder"`
	ModifiedAt     *time.Time `json:"modified_at,omitempty"`
	ETag           string     `json:"etag,omitempty"`
	CTag           string     `json:"ctag,omitempty"`
	WebURL         string     `json:"web_url,omitempty"`
	QuickXorHash   string     `json:"quick_xor_hash,omitempty"`
	SHA1Hash       string     `json:"sha1_hash,omitempty"`
	DeletedAt      *time.Time `json:"deleted_at,omitempty"`
}

// UpsertResult reports whether an upsert created a new row or updated an
// existing one. Callers use this to choose which event subject to publish.
type UpsertResult struct {
	Item     Item
	Inserted bool
}

type Items struct {
	pool *pgxpool.Pool
}

// Upsert inserts or refreshes one item from a Graph DriveItem. The deleted
// facet is handled separately via SoftDelete — Upsert always treats the
// item as live.
func (i *Items) Upsert(ctx context.Context, sourceID uuid.UUID, organizationID string, d sharepoint.DriveItem, rawJSON []byte) (UpsertResult, error) {
	const q = `
INSERT INTO items (
    source_id, organization_id, item_id, parent_item_id, path, name,
    mime_type, size_bytes, is_folder, modified_at, etag, ctag, web_url,
    quick_xor_hash, sha1_hash, deleted_at, raw
)
VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NULL,COALESCE($16::jsonb, '{}'::jsonb))
ON CONFLICT (source_id, item_id) DO UPDATE
   SET parent_item_id = EXCLUDED.parent_item_id,
       path           = EXCLUDED.path,
       name           = EXCLUDED.name,
       mime_type      = EXCLUDED.mime_type,
       size_bytes     = EXCLUDED.size_bytes,
       is_folder      = EXCLUDED.is_folder,
       modified_at    = EXCLUDED.modified_at,
       etag           = EXCLUDED.etag,
       ctag           = EXCLUDED.ctag,
       web_url        = EXCLUDED.web_url,
       quick_xor_hash = EXCLUDED.quick_xor_hash,
       sha1_hash      = EXCLUDED.sha1_hash,
       deleted_at     = NULL,
       raw            = EXCLUDED.raw
RETURNING id, source_id, organization_id, item_id,
          COALESCE(parent_item_id,''), path, name,
          COALESCE(mime_type,''), COALESCE(size_bytes,0), is_folder,
          modified_at, COALESCE(etag,''), COALESCE(ctag,''),
          COALESCE(web_url,''), COALESCE(quick_xor_hash,''),
          COALESCE(sha1_hash,''), deleted_at,
          (xmax = 0) AS inserted`

	var (
		out      Item
		inserted bool
	)
	err := i.pool.QueryRow(ctx, q,
		sourceID,
		organizationID,
		d.ID,
		nullableString(d.ParentItemID()),
		d.FullPath(),
		d.Name,
		nullableString(d.MimeType()),
		d.Size,
		d.IsFolder(),
		d.LastModifiedDateTime,
		nullableString(d.ETag),
		nullableString(d.CTag),
		nullableString(d.WebURL),
		nullableString(d.QuickXorHash()),
		nullableString(d.SHA1Hash()),
		rawJSON,
	).Scan(
		&out.ID, &out.SourceID, &out.OrganizationID, &out.ItemID,
		&out.ParentItemID, &out.Path, &out.Name,
		&out.MimeType, &out.SizeBytes, &out.IsFolder,
		&out.ModifiedAt, &out.ETag, &out.CTag,
		&out.WebURL, &out.QuickXorHash, &out.SHA1Hash, &out.DeletedAt,
		&inserted,
	)
	if err != nil {
		return UpsertResult{}, fmt.Errorf("upsert item %s: %w", d.ID, err)
	}
	return UpsertResult{Item: out, Inserted: inserted}, nil
}

// SoftDelete marks an item as deleted (deleted_at = NOW()) if it exists.
// Returns ErrNotFound when the item has never been seen — Graph sometimes
// emits delete tombstones for items the connector never observed (e.g. items
// created and removed between two delta windows), which is not an error.
func (i *Items) SoftDelete(ctx context.Context, sourceID uuid.UUID, itemID string) (Item, error) {
	const q = `
UPDATE items
   SET deleted_at = COALESCE(deleted_at, NOW())
 WHERE source_id = $1 AND item_id = $2
RETURNING id, source_id, organization_id, item_id,
          COALESCE(parent_item_id,''), path, name,
          COALESCE(mime_type,''), COALESCE(size_bytes,0), is_folder,
          modified_at, COALESCE(etag,''), COALESCE(ctag,''),
          COALESCE(web_url,''), COALESCE(quick_xor_hash,''),
          COALESCE(sha1_hash,''), deleted_at`

	var out Item
	err := i.pool.QueryRow(ctx, q, sourceID, itemID).Scan(
		&out.ID, &out.SourceID, &out.OrganizationID, &out.ItemID,
		&out.ParentItemID, &out.Path, &out.Name,
		&out.MimeType, &out.SizeBytes, &out.IsFolder,
		&out.ModifiedAt, &out.ETag, &out.CTag,
		&out.WebURL, &out.QuickXorHash, &out.SHA1Hash, &out.DeletedAt,
	)
	if errors.Is(err, pgxNoRows) {
		return Item{}, ErrNotFound
	}
	if err != nil {
		return Item{}, fmt.Errorf("soft-delete item %s: %w", itemID, err)
	}
	return out, nil
}

// ExecTarget is the minimal projection the executor needs to act on one item
// in Microsoft Graph: the drive (from the joined source) plus the Graph item
// id and enough metadata to make a safety decision and write an audit row.
type ExecTarget struct {
	ItemPK         uuid.UUID
	OrganizationID string
	SourceID       uuid.UUID
	DriveID        string
	ItemID         string
	Path           string
	Name           string
	IsFolder       bool
	AlreadyDeleted bool
}

// ResolveForExecution looks up one item by primary key and joins its source to
// recover the drive id Graph mutations need. Returns ErrNotFound when the
// item PK does not exist.
func (i *Items) ResolveForExecution(ctx context.Context, itemPK uuid.UUID) (ExecTarget, error) {
	const q = `
SELECT it.id, it.organization_id, it.source_id, s.drive_id, it.item_id,
       it.path, it.name, it.is_folder, (it.deleted_at IS NOT NULL)
  FROM items it
  JOIN sources s ON s.id = it.source_id
 WHERE it.id = $1`
	var t ExecTarget
	err := i.pool.QueryRow(ctx, q, itemPK).Scan(
		&t.ItemPK, &t.OrganizationID, &t.SourceID, &t.DriveID, &t.ItemID,
		&t.Path, &t.Name, &t.IsFolder, &t.AlreadyDeleted,
	)
	if errors.Is(err, pgxNoRows) {
		return ExecTarget{}, ErrNotFound
	}
	if err != nil {
		return ExecTarget{}, fmt.Errorf("resolve item %s for execution: %w", itemPK, err)
	}
	return t, nil
}

// SoftDeleteByPK marks an item deleted by its primary key. Used by the
// executor after a successful Graph delete (the next delta would also do this
// via the tombstone, but acting immediately keeps local state honest).
func (i *Items) SoftDeleteByPK(ctx context.Context, itemPK uuid.UUID) error {
	ct, err := i.pool.Exec(ctx, `UPDATE items SET deleted_at = COALESCE(deleted_at, NOW()) WHERE id = $1`, itemPK)
	if err != nil {
		return fmt.Errorf("soft-delete item pk %s: %w", itemPK, err)
	}
	if ct.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (i *Items) ListLiveBySource(ctx context.Context, sourceID uuid.UUID, limit, offset int) ([]Item, error) {
	if limit <= 0 {
		limit = 500
	}
	if offset < 0 {
		offset = 0
	}

	const q = `
SELECT id, source_id, organization_id, item_id,
       COALESCE(parent_item_id,''), path, name,
       COALESCE(mime_type,''), COALESCE(size_bytes,0), is_folder,
       modified_at, COALESCE(etag,''), COALESCE(ctag,''),
       COALESCE(web_url,''), COALESCE(quick_xor_hash,''),
       COALESCE(sha1_hash,''), deleted_at
  FROM items
 WHERE source_id = $1
   AND deleted_at IS NULL
 ORDER BY id
 LIMIT $2 OFFSET $3`

	rows, err := i.pool.Query(ctx, q, sourceID, limit, offset)
	if err != nil {
		return nil, fmt.Errorf("list live items by source: %w", err)
	}
	defer rows.Close()

	var out []Item
	for rows.Next() {
		var item Item
		if err := rows.Scan(
			&item.ID, &item.SourceID, &item.OrganizationID, &item.ItemID,
			&item.ParentItemID, &item.Path, &item.Name,
			&item.MimeType, &item.SizeBytes, &item.IsFolder,
			&item.ModifiedAt, &item.ETag, &item.CTag,
			&item.WebURL, &item.QuickXorHash, &item.SHA1Hash, &item.DeletedAt,
		); err != nil {
			return nil, fmt.Errorf("scan live item: %w", err)
		}
		out = append(out, item)
	}
	return out, rows.Err()
}
