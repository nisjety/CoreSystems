// Package workspace holds the durable (Postgres-backed) workspace-manifest
// store over the `workspace_files` table (migration 0001).
//
// Follows capability-core's internal/registry/scope_store.go template
// exactly, the same one sandbox-manager's own internal/lease.Store already
// uses: a narrow Exec/Query interface wrapping *pgxpool.Pool (so unit tests
// substitute a stub without a real database), inline SQL, fmt.Errorf-wrapped
// errors.
//
// Per apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
// §2-3 and §8 item 3.5.C: a row with run_id IS NULL is the Space's own
// merged, durable state; a row with run_id set is one run's not-yet-merged
// overlay. GetManifest resolves the *layered view* a given run sees — its
// own overlay shadowing the Space's rows — via a DISTINCT ON query with no
// existing precedent elsewhere in this Go codebase (checked directly: zero
// DISTINCT ON/UNION hits repo-wide before this file).
package workspace

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ManifestEntry is one file visible to a lease's layered workspace view —
// either a Space-level row (unshadowed) or this run's own overlay row.
type ManifestEntry struct {
	Path        string
	ContentHash string
}

// ChangedFile is one file a run's diff_and_upload found changed or newly
// created, ready to upsert as that run's workspace_files overlay row.
// BaseHash is the Space-level hash this path had when the run's hydrate
// observed it — captured client-side at hydrate time, not re-derived here,
// since step 4's PromoteWorkspace merge compares against what the run
// actually saw, not whatever the Space row says by the time of upload.
// Empty means the path did not exist yet when this run hydrated.
type ChangedFile struct {
	Path        string
	ContentHash string
	SizeBytes   int64
	BaseHash    string
}

type workspaceDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
}

// Store is the durable workspace-manifest registry.
type Store struct {
	pool workspaceDatabase
}

// NewStore constructs a Store over pool. Returns an error for a nil pool —
// there is no in-memory fallback here, matching lease.Store/scope_store.go's
// own convention for a store meant to survive a restart.
func NewStore(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Store{pool: pool}, nil
}

// GetManifest returns the layered manifest visible to runID: every Space
// row (run_id IS NULL) shadowed path-for-path by runID's own overlay rows.
// Empty spaceID is the caller's responsibility to avoid (a non-Space lease
// has no manifest concept at all) — callers should not invoke this for one.
func (s *Store) GetManifest(ctx context.Context, orgID, spaceID, runID string) ([]ManifestEntry, error) {
	if orgID == "" || spaceID == "" || runID == "" {
		return nil, fmt.Errorf("org_id, space_id, and run_id are required")
	}
	rows, err := s.pool.Query(ctx, `
		SELECT DISTINCT ON (path) path, content_hash
		FROM workspace_files
		WHERE org_id = $1 AND space_id = $2 AND (run_id IS NULL OR run_id = $3)
		ORDER BY path, run_id IS NULL ASC
	`, orgID, spaceID, runID)
	if err != nil {
		return nil, fmt.Errorf("get workspace manifest: %w", err)
	}
	defer rows.Close()

	var entries []ManifestEntry
	for rows.Next() {
		var e ManifestEntry
		if err := rows.Scan(&e.Path, &e.ContentHash); err != nil {
			return nil, fmt.Errorf("scan workspace manifest entry: %w", err)
		}
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

// UpsertOverlay records runID's changed/new files as its workspace_files
// overlay (run_id = runID), one upsert per file against the migration's own
// workspace_files_identity_uq index. Idempotent: re-upserting the same
// content is safe, so a caller need not roll back an earlier file in the
// same batch if a later one fails.
func (s *Store) UpsertOverlay(ctx context.Context, orgID, spaceID, runID string, files []ChangedFile) error {
	if orgID == "" || spaceID == "" || runID == "" {
		return fmt.Errorf("org_id, space_id, and run_id are required")
	}
	for _, f := range files {
		if f.Path == "" || f.ContentHash == "" {
			return fmt.Errorf("changed file path and content_hash are required")
		}
		if _, err := s.pool.Exec(ctx, `
			INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, base_hash, size_bytes, updated_at)
			VALUES ($1, $2, $3, $4, $5, NULLIF($6, ''), $7, now())
			ON CONFLICT (org_id, space_id, COALESCE(run_id, ''), path)
			DO UPDATE SET content_hash = EXCLUDED.content_hash, size_bytes = EXCLUDED.size_bytes,
			              base_hash = EXCLUDED.base_hash, updated_at = now()
		`, orgID, spaceID, runID, f.Path, f.ContentHash, f.BaseHash, f.SizeBytes); err != nil {
			return fmt.Errorf("upsert workspace overlay file %q: %w", f.Path, err)
		}
	}
	return nil
}
