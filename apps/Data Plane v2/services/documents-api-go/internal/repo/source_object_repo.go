package repo

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

type SourceObjectRepo struct {
	pool *pgxpool.Pool
}

func NewSourceObjectRepo(pool *pgxpool.Pool) *SourceObjectRepo {
	return &SourceObjectRepo{pool: pool}
}

type UpsertSourceObjectResult struct {
	SourceObject *model.SourceObject
	Inserted     bool
	// ContentChanged is true when an update changed the object's content_hash
	// (i.e. the underlying file content actually changed, not just metadata or
	// a no-op delta re-touch). Always false on a fresh insert — callers treat
	// inserts as new content via Inserted.
	ContentChanged bool
}

// sourceObjectContentChanged reports whether an upsert changed the indexable
// content of an existing source object. Inserts are reported via Inserted, so
// this is false on insert and true only when an update moved the content hash.
func sourceObjectContentChanged(inserted bool, oldHash, newHash string) bool {
	if inserted {
		return false
	}
	return oldHash != newHash
}

func (r *SourceObjectRepo) Upsert(ctx context.Context, input model.UpsertSourceObjectInput) (*UpsertSourceObjectResult, error) {
	meta := input.Metadata
	if len(meta) == 0 {
		meta = json.RawMessage(`{}`)
	}

	row := r.pool.QueryRow(ctx, `
		WITH prev AS (
			SELECT content_hash AS old_content_hash
			FROM source_objects
			WHERE org_id = $1 AND connector = $2 AND external_id = $4
		)
		INSERT INTO source_objects (
			org_id, connector, source, external_id, site_id, drive_id, item_id,
			parent_id, path, name, mime_type, size_bytes, etag, ctag,
			quickxor_hash, sha1_hash, content_hash, acl_tags, metadata, modified_at,
			deleted_at
		)
		VALUES (
			$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
			COALESCE($18::text[], '{}'::text[]), $19::jsonb, $20, NULL
		)
		ON CONFLICT (org_id, connector, external_id) DO UPDATE
		   SET source        = EXCLUDED.source,
		       site_id       = EXCLUDED.site_id,
		       drive_id      = EXCLUDED.drive_id,
		       item_id       = EXCLUDED.item_id,
		       parent_id     = EXCLUDED.parent_id,
		       path          = EXCLUDED.path,
		       name          = EXCLUDED.name,
		       mime_type     = EXCLUDED.mime_type,
		       size_bytes    = EXCLUDED.size_bytes,
		       etag          = EXCLUDED.etag,
		       ctag          = EXCLUDED.ctag,
		       quickxor_hash = EXCLUDED.quickxor_hash,
		       sha1_hash     = EXCLUDED.sha1_hash,
		       content_hash  = EXCLUDED.content_hash,
		       acl_tags      = EXCLUDED.acl_tags,
		       metadata      = EXCLUDED.metadata,
		       modified_at   = EXCLUDED.modified_at,
		       deleted_at    = NULL
		RETURNING source_object_id, org_id, connector, source, external_id,
		          COALESCE(site_id,''), COALESCE(drive_id,''), COALESCE(item_id,''),
		          COALESCE(parent_id,''), COALESCE(path,''), name, COALESCE(mime_type,''),
		          size_bytes, COALESCE(etag,''), COALESCE(ctag,''), COALESCE(quickxor_hash,''),
		          COALESCE(sha1_hash,''), COALESCE(content_hash,''), acl_tags, metadata,
		          modified_at, discovered_at, deleted_at, updated_at, (xmax = 0) AS inserted,
		          COALESCE((SELECT old_content_hash FROM prev), '') AS old_content_hash
	`,
		input.OrgID,
		input.Connector,
		input.Source,
		input.ExternalID,
		nilIfEmpty(input.SiteID),
		nilIfEmpty(input.DriveID),
		nilIfEmpty(input.ItemID),
		nilIfEmpty(input.ParentID),
		nilIfEmpty(input.Path),
		input.Name,
		nilIfEmpty(input.MimeType),
		input.SizeBytes,
		nilIfEmpty(input.ETag),
		nilIfEmpty(input.CTag),
		nilIfEmpty(input.QuickXorHash),
		nilIfEmpty(input.SHA1Hash),
		nilIfEmpty(input.ContentHash),
		input.ACLTags,
		meta,
		input.ModifiedAt,
	)

	var obj model.SourceObject
	var inserted bool
	var oldContentHash string
	if err := row.Scan(
		&obj.SourceObjectID, &obj.OrgID, &obj.Connector, &obj.Source, &obj.ExternalID,
		&obj.SiteID, &obj.DriveID, &obj.ItemID, &obj.ParentID, &obj.Path, &obj.Name, &obj.MimeType,
		&obj.SizeBytes, &obj.ETag, &obj.CTag, &obj.QuickXorHash, &obj.SHA1Hash, &obj.ContentHash,
		&obj.ACLTags, &obj.Metadata, &obj.ModifiedAt, &obj.DiscoveredAt, &obj.DeletedAt, &obj.UpdatedAt,
		&inserted, &oldContentHash,
	); err != nil {
		return nil, fmt.Errorf("upsert source object: %w", err)
	}

	r.bumpOrgVersion(ctx, input.OrgID)
	return &UpsertSourceObjectResult{
		SourceObject:   &obj,
		Inserted:       inserted,
		ContentChanged: sourceObjectContentChanged(inserted, oldContentHash, obj.ContentHash),
	}, nil
}

func (r *SourceObjectRepo) SoftDelete(ctx context.Context, input model.DeleteSourceObjectInput) (*model.SourceObject, error) {
	var row pgx.Row
	if input.SourceObjectID != "" {
		row = r.pool.QueryRow(ctx, `
			UPDATE source_objects
			   SET deleted_at = COALESCE(deleted_at, NOW())
			 WHERE org_id = $1 AND source_object_id = $2
			 RETURNING source_object_id, org_id, connector, source, external_id,
			           COALESCE(site_id,''), COALESCE(drive_id,''), COALESCE(item_id,''),
			           COALESCE(parent_id,''), COALESCE(path,''), name, COALESCE(mime_type,''),
			           size_bytes, COALESCE(etag,''), COALESCE(ctag,''), COALESCE(quickxor_hash,''),
			           COALESCE(sha1_hash,''), COALESCE(content_hash,''), acl_tags, metadata,
			           modified_at, discovered_at, deleted_at, updated_at
		`, input.OrgID, input.SourceObjectID)
	} else {
		row = r.pool.QueryRow(ctx, `
			UPDATE source_objects
			   SET deleted_at = COALESCE(deleted_at, NOW())
			 WHERE org_id = $1 AND connector = $2 AND external_id = $3
			 RETURNING source_object_id, org_id, connector, source, external_id,
			           COALESCE(site_id,''), COALESCE(drive_id,''), COALESCE(item_id,''),
			           COALESCE(parent_id,''), COALESCE(path,''), name, COALESCE(mime_type,''),
			           size_bytes, COALESCE(etag,''), COALESCE(ctag,''), COALESCE(quickxor_hash,''),
			           COALESCE(sha1_hash,''), COALESCE(content_hash,''), acl_tags, metadata,
			           modified_at, discovered_at, deleted_at, updated_at
		`, input.OrgID, input.Connector, input.ExternalID)
	}

	var obj model.SourceObject
	err := row.Scan(
		&obj.SourceObjectID, &obj.OrgID, &obj.Connector, &obj.Source, &obj.ExternalID,
		&obj.SiteID, &obj.DriveID, &obj.ItemID, &obj.ParentID, &obj.Path, &obj.Name, &obj.MimeType,
		&obj.SizeBytes, &obj.ETag, &obj.CTag, &obj.QuickXorHash, &obj.SHA1Hash, &obj.ContentHash,
		&obj.ACLTags, &obj.Metadata, &obj.ModifiedAt, &obj.DiscoveredAt, &obj.DeletedAt, &obj.UpdatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("soft delete source object: %w", err)
	}

	r.bumpOrgVersion(ctx, input.OrgID)
	return &obj, nil
}

func (r *SourceObjectRepo) Duplicates(ctx context.Context, input model.ListSourceObjectDuplicatesInput) ([]model.SourceObjectDuplicateGroup, error) {
	if input.MinCount < 2 {
		input.MinCount = 2
	}
	if input.MaxGroups <= 0 {
		input.MaxGroups = 100
	}

	rows, err := r.pool.Query(ctx, `
		WITH live AS (
			SELECT
				source_object_id, connector, source, external_id,
				COALESCE(site_id,'') AS site_id,
				COALESCE(drive_id,'') AS drive_id,
				COALESCE(item_id,'') AS item_id,
				COALESCE(path,'') AS path,
				name,
				COALESCE(mime_type,'') AS mime_type,
				COALESCE(size_bytes, 0) AS size_bytes,
				modified_at,
				COALESCE(quickxor_hash,'') AS quickxor_hash,
				COALESCE(sha1_hash,'') AS sha1_hash,
				COALESCE(content_hash,'') AS content_hash,
				CASE
					WHEN COALESCE(content_hash,'') <> '' THEN content_hash
					WHEN COALESCE(sha1_hash,'') <> '' THEN 'sha1:' || sha1_hash
					WHEN COALESCE(quickxor_hash,'') <> '' THEN 'quickxor:' || quickxor_hash
					ELSE NULL
				END AS dedupe_hash
			FROM source_objects
			WHERE org_id = $1
			  AND deleted_at IS NULL
			  AND COALESCE(size_bytes, 0) >= $3
			  AND ($5::TEXT = '' OR source = $5)
		),
		groups AS (
			SELECT
				dedupe_hash,
				COUNT(*)::INT AS group_count,
				COALESCE(SUM(size_bytes), 0)::BIGINT AS total_bytes
			FROM live
			WHERE dedupe_hash IS NOT NULL
			GROUP BY dedupe_hash
			HAVING COUNT(*) >= $2
			ORDER BY total_bytes DESC, dedupe_hash
			LIMIT $4
		)
		SELECT
			g.dedupe_hash, g.group_count, g.total_bytes,
			l.source_object_id, l.connector, l.source, l.external_id,
			l.site_id, l.drive_id, l.item_id, l.path, l.name, l.mime_type,
			l.size_bytes, l.modified_at, l.quickxor_hash, l.sha1_hash, l.content_hash
		FROM groups g
		JOIN live l USING (dedupe_hash)
		ORDER BY g.total_bytes DESC, g.dedupe_hash, l.modified_at NULLS LAST, l.path
	`,
		input.OrgID,
		input.MinCount,
		input.MinSizeBytes,
		input.MaxGroups,
		input.Source,
	)
	if err != nil {
		return nil, fmt.Errorf("list source object duplicates: %w", err)
	}
	defer rows.Close()

	byHash := make(map[string]*model.SourceObjectDuplicateGroup)
	order := make([]string, 0)
	for rows.Next() {
		var (
			hash       string
			count      int
			totalBytes int64
			member     model.SourceObjectDuplicateMember
		)
		if err := rows.Scan(
			&hash, &count, &totalBytes,
			&member.SourceObjectID, &member.Connector, &member.Source, &member.ExternalID,
			&member.SiteID, &member.DriveID, &member.ItemID, &member.Path, &member.Name, &member.MimeType,
			&member.SizeBytes, &member.ModifiedAt, &member.QuickXorHash, &member.SHA1Hash, &member.ContentHash,
		); err != nil {
			return nil, fmt.Errorf("scan source object duplicate: %w", err)
		}

		group, ok := byHash[hash]
		if !ok {
			kind, value := splitDedupeHash(hash)
			group = &model.SourceObjectDuplicateGroup{
				HashKind:   kind,
				HashValue:  value,
				Count:      count,
				TotalBytes: totalBytes,
				Members:    make([]model.SourceObjectDuplicateMember, 0, count),
			}
			byHash[hash] = group
			order = append(order, hash)
		}
		group.Members = append(group.Members, member)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("source object duplicates rows: %w", err)
	}

	out := make([]model.SourceObjectDuplicateGroup, 0, len(order))
	for _, hash := range order {
		out = append(out, *byHash[hash])
	}
	return out, nil
}

func (r *SourceObjectRepo) bumpOrgVersion(ctx context.Context, orgID string) {
	_, err := r.pool.Exec(ctx, `
		INSERT INTO org_versions (org_id, version, bumped_at)
			VALUES ($1, 2, NOW())
		ON CONFLICT (org_id) DO UPDATE
			SET version = org_versions.version + 1,
			    bumped_at = NOW()
	`, orgID)
	if err != nil {
		fmt.Printf("warn: org_version bump failed for %s: %v\n", orgID, err)
	}
}

func splitDedupeHash(hash string) (string, string) {
	for i := 0; i < len(hash); i++ {
		if hash[i] == ':' {
			return hash[:i], hash[i+1:]
		}
	}
	return "", hash
}
