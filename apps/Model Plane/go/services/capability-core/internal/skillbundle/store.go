package skillbundle

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// ErrNotFound means the org holds no bundle for the skill, or no file of it has that digest.
var ErrNotFound = errors.New("skill bundle not found")

// DB is the subset of the capability-core pool the store uses. Writes run in a transaction,
// so a replaced bundle is never left half-written.
type DB interface {
	Begin(ctx context.Context) (pgx.Tx, error)
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Store reads and writes skill bundles. Every statement filters on the org, and a blob is
// addressed by org and digest, so one org never reads another org's bytes.
type Store struct {
	db DB
}

// NewStore wraps the pool.
func NewStore(db DB) *Store {
	return &Store{db: db}
}

// Summary describes one stored bundle without its files.
type Summary struct {
	SkillID        string
	ManifestSHA256 string
	FileCount      int
	TotalBytes     int64
	CreatedBy      string
	CreatedAt      time.Time
	// DeckTemplate is true when the bundle holds ThemePath (EP-5 S6). Derived from the bundle's
	// files on every read rather than stored, so no migration carries it and it cannot go stale.
	DeckTemplate bool
}

// Manifest is one stored bundle's files, in path order, with bodies left out.
type Manifest struct {
	SkillID        string
	ManifestSHA256 string
	Files          []File
}

// PutResult says whether the upload replaced a bundle the skill already had.
type PutResult struct {
	Replaced bool
	Summary  Summary
}

// lockOrgSQL serializes bundle writes for one org. The blob GC in a write must not race a
// concurrent write that is about to reference the same digest, and the lock is released with
// the transaction.
const lockOrgSQL = `SELECT pg_advisory_xact_lock(hashtextextended('skill_bundles:' || $1::text, 0))`

// Put stores a validated bundle for the skill, replacing any bundle it had. The old bundle's
// files go with it through the cascade, and any blob no bundle of the org still names is
// dropped. The write is one transaction that also appends a registry_audit_log row which
// names the digest and the totals, never a path or a body.
func (s *Store) Put(ctx context.Context, orgID, skillID, actor string, bundle Bundle) (PutResult, error) {
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return PutResult{}, fmt.Errorf("begin skill bundle write: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return PutResult{}, fmt.Errorf("lock skill bundles: %w", err)
	}
	deleted, err := tx.Exec(ctx, `DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2`, orgID, skillID)
	if err != nil {
		return PutResult{}, fmt.Errorf("replace skill bundle: %w", err)
	}
	bundleID := "skb_" + uuid.NewString()
	var createdAt time.Time
	err = tx.QueryRow(ctx, `
		INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING created_at
	`, bundleID, orgID, skillID, bundle.ManifestSHA256, len(bundle.Files), bundle.TotalBytes, actor).Scan(&createdAt)
	if err != nil {
		return PutResult{}, fmt.Errorf("insert skill bundle: %w", err)
	}

	blobDigests, blobBodies, blobSizes := make([]string, 0, len(bundle.Files)), make([][]byte, 0, len(bundle.Files)), make([]int32, 0, len(bundle.Files))
	seen := make(map[string]bool, len(bundle.Files))
	paths, digests, sizes, executables := make([]string, 0, len(bundle.Files)), make([]string, 0, len(bundle.Files)), make([]int32, 0, len(bundle.Files)), make([]bool, 0, len(bundle.Files))
	binaries := make([]bool, 0, len(bundle.Files))
	for _, file := range bundle.Files {
		if !seen[file.SHA256] {
			seen[file.SHA256] = true
			blobDigests = append(blobDigests, file.SHA256)
			blobBodies = append(blobBodies, file.Body)
			blobSizes = append(blobSizes, int32(file.Size))
		}
		paths = append(paths, file.Path)
		digests = append(digests, file.SHA256)
		sizes = append(sizes, int32(file.Size))
		executables = append(executables, file.Executable)
		binaries = append(binaries, file.Binary)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO skill_bundle_blobs (org_id, sha256, content, size_bytes)
		SELECT $1, u.sha256, u.content, u.size_bytes
		FROM unnest($2::text[], $3::bytea[], $4::int[]) AS u(sha256, content, size_bytes)
		ON CONFLICT (org_id, sha256) DO NOTHING
	`, orgID, blobDigests, blobBodies, blobSizes); err != nil {
		return PutResult{}, fmt.Errorf("store skill bundle blobs: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO skill_bundle_files (bundle_id, path, sha256, size_bytes, executable, is_binary)
		SELECT $1, u.path, u.sha256, u.size_bytes, u.executable, u.is_binary
		FROM unnest($2::text[], $3::text[], $4::int[], $5::bool[], $6::bool[])
		     AS u(path, sha256, size_bytes, executable, is_binary)
	`, bundleID, paths, digests, sizes, executables, binaries); err != nil {
		return PutResult{}, fmt.Errorf("store skill bundle files: %w", err)
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return PutResult{}, fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	if err := appendAudit(ctx, tx, "skill_bundle.uploaded", orgID, skillID, actor, bundle.ManifestSHA256,
		len(bundle.Files), bundle.TotalBytes); err != nil {
		return PutResult{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return PutResult{}, fmt.Errorf("commit skill bundle write: %w", err)
	}
	return PutResult{
		Replaced: deleted.RowsAffected() > 0,
		Summary: Summary{
			SkillID: skillID, ManifestSHA256: bundle.ManifestSHA256, FileCount: len(bundle.Files),
			TotalBytes: bundle.TotalBytes, CreatedBy: actor, CreatedAt: createdAt,
		},
	}, nil
}

// Delete removes the org's bundle for the skill. Its files cascade, and blobs no bundle still
// names are dropped. ErrNotFound when there is no bundle to remove.
func (s *Store) Delete(ctx context.Context, orgID, skillID, actor string) error {
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin skill bundle delete: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	var manifest string
	var fileCount int
	var totalBytes int64
	err = tx.QueryRow(ctx, `
		DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2
		RETURNING manifest_sha256, file_count, total_bytes
	`, orgID, skillID).Scan(&manifest, &fileCount, &totalBytes)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("delete skill bundle: %w", err)
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	if err := appendAudit(ctx, tx, "skill_bundle.deleted", orgID, skillID, actor, manifest, fileCount, totalBytes); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit skill bundle delete: %w", err)
	}
	return nil
}

// gcBlobsSQL drops the org's blobs that no bundle file names any more.
const gcBlobsSQL = `
DELETE FROM skill_bundle_blobs b
WHERE b.org_id = $1
  AND NOT EXISTS (
      SELECT 1 FROM skill_bundle_files f
      JOIN skill_bundles sb ON sb.id = f.bundle_id
      WHERE sb.org_id = b.org_id AND f.sha256 = b.sha256
  )
`

// appendAudit writes the registry_audit_log row for a bundle change. The diff holds the
// digest and the totals only: a skill bundle's paths and bodies are never audit content.
func appendAudit(ctx context.Context, tx pgx.Tx, action, orgID, skillID, actor, manifest string, fileCount int, totalBytes int64) error {
	_, err := tx.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1, 'skill_bundle', $2, $3, $4, $5,
		        jsonb_build_object('manifest_sha256', $6::text, 'file_count', $7::int, 'total_bytes', $8::bigint))
	`, "ral_"+uuid.NewString(), skillID, action, actor, orgID, manifest, fileCount, totalBytes)
	if err != nil {
		return fmt.Errorf("append skill bundle audit: %w", err)
	}
	return nil
}

// List returns the org's bundle summaries, narrowed to skillIDs when any are given.
func (s *Store) List(ctx context.Context, orgID string, skillIDs []string) ([]Summary, error) {
	var filter []string
	if len(skillIDs) > 0 {
		filter = skillIDs
	}
	rows, err := s.db.Query(ctx, `
		SELECT b.skill_id, b.manifest_sha256, b.file_count, b.total_bytes, b.created_by, b.created_at,
		       EXISTS (SELECT 1 FROM skill_bundle_files f WHERE f.bundle_id = b.id AND f.path = $3)
		FROM skill_bundles b
		WHERE b.org_id = $1 AND ($2::text[] IS NULL OR b.skill_id = ANY($2::text[]))
		ORDER BY b.skill_id
	`, orgID, filter, ThemePath)
	if err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	defer rows.Close()
	summaries := []Summary{}
	for rows.Next() {
		var summary Summary
		if err := rows.Scan(&summary.SkillID, &summary.ManifestSHA256, &summary.FileCount,
			&summary.TotalBytes, &summary.CreatedBy, &summary.CreatedAt, &summary.DeckTemplate); err != nil {
			return nil, fmt.Errorf("scan skill bundle: %w", err)
		}
		summaries = append(summaries, summary)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	return summaries, nil
}

// Manifest returns the org's bundle for the skill: its digest and every file's metadata.
// ErrNotFound when the org has no bundle for the skill.
func (s *Store) Manifest(ctx context.Context, orgID, skillID string) (Manifest, error) {
	rows, err := s.db.Query(ctx, `
		SELECT b.manifest_sha256, f.path, f.sha256, f.size_bytes, f.executable, f.is_binary
		FROM skill_bundles b
		JOIN skill_bundle_files f ON f.bundle_id = b.id
		WHERE b.org_id = $1 AND b.skill_id = $2
		ORDER BY f.path
	`, orgID, skillID)
	if err != nil {
		return Manifest{}, fmt.Errorf("read skill bundle manifest: %w", err)
	}
	defer rows.Close()
	manifest := Manifest{SkillID: skillID, Files: []File{}}
	for rows.Next() {
		var file File
		if err := rows.Scan(&manifest.ManifestSHA256, &file.Path, &file.SHA256, &file.Size,
			&file.Executable, &file.Binary); err != nil {
			return Manifest{}, fmt.Errorf("scan skill bundle file: %w", err)
		}
		manifest.Files = append(manifest.Files, file)
	}
	if err := rows.Err(); err != nil {
		return Manifest{}, fmt.Errorf("read skill bundle manifest: %w", err)
	}
	if len(manifest.Files) == 0 {
		return Manifest{}, ErrNotFound
	}
	return manifest, nil
}

// Blob returns the body of one file of the org's bundle for the skill. A digest the skill's
// bundle does not name is ErrNotFound, even when another skill of the org stores the same
// bytes, so a read is bound to the bundle it was asked about.
func (s *Store) Blob(ctx context.Context, orgID, skillID, sha256 string) ([]byte, error) {
	var content []byte
	err := s.db.QueryRow(ctx, `
		SELECT b.content
		FROM skill_bundle_blobs b
		WHERE b.org_id = $1 AND b.sha256 = $3
		  AND EXISTS (
		      SELECT 1 FROM skill_bundle_files f
		      JOIN skill_bundles sb ON sb.id = f.bundle_id
		      WHERE sb.org_id = $1 AND sb.skill_id = $2 AND f.sha256 = $3
		  )
	`, orgID, skillID, sha256).Scan(&content)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("read skill blob: %w", err)
	}
	return content, nil
}
