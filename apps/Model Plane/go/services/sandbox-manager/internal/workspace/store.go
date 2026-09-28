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

// Promote merges runID's own workspace_files overlay into the Space's
// durable rows (run_id IS NULL) — S3.3 step 4 (design doc §4, §8 item 4).
// One compare-and-swap upsert per path, never one all-or-nothing
// transaction across the whole overlay: a conflict on one path never
// blocks any other path in the same run's overlay from merging.
//
// A path merges when the Space's current row either doesn't exist yet, or
// its content_hash matches the overlay row's base_hash (the hash this run
// saw when it hydrated) — first-time merge — or already matches the
// overlay row's OWN content_hash — an already-merged path, so a second
// Promote call for the same overlay is a safe no-op, not a false conflict
// (this is the one subtlety a naive base_hash-only check gets wrong: after
// a successful merge the Space's hash no longer equals base_hash, since it
// now equals what THIS run itself just wrote, not a stranger's change).
// Anything else — the Space's current content differs from both — is
// reported in the returned slice; every other path still merges.
//
// Deliberately re-reads each path's overlay row FRESH inside the same
// statement that performs its compare-and-swap merge, rather than reading
// the whole overlay once up front and reusing those Go-side values across
// the per-path loop. An adversarial review of the first draft (which did
// the latter) found a real lost-update: PromoteWorkspace and SnapshotSandbox
// are independent RPCs with no lock between them, so a caller's
// SnapshotSandbox call landing between this method's initial read and its
// later per-path write would have its newer overlay content silently
// discarded in favor of the stale value this method captured earlier — and
// the Space row would then mismatch BOTH the stale write and the overlay's
// actual current content, false-conflicting a future, perfectly mergeable
// Promote call. Folding the read into each path's own INSERT ... SELECT ...
// FROM (a one-row CTE) closes the window entirely: Postgres evaluates the
// CTE and the conflict-checking WHERE clause within the SAME atomic
// statement, so whatever this call writes is always whatever the overlay
// held at the moment of that specific write, never an earlier snapshot.
func (s *Store) Promote(ctx context.Context, orgID, spaceID, runID string) ([]string, error) {
	if orgID == "" || spaceID == "" || runID == "" {
		return nil, fmt.Errorf("org_id, space_id, and run_id are required")
	}
	rows, err := s.pool.Query(ctx, `
		SELECT path
		FROM workspace_files
		WHERE org_id = $1 AND space_id = $2 AND run_id = $3
	`, orgID, spaceID, runID)
	if err != nil {
		return nil, fmt.Errorf("promote workspace: list overlay paths: %w", err)
	}
	var paths []string
	for rows.Next() {
		var path string
		if scanErr := rows.Scan(&path); scanErr != nil {
			rows.Close()
			return nil, fmt.Errorf("promote workspace: scan overlay path: %w", scanErr)
		}
		paths = append(paths, path)
	}
	rowsErr := rows.Err()
	rows.Close()
	if rowsErr != nil {
		return nil, fmt.Errorf("promote workspace: list overlay paths: %w", rowsErr)
	}

	var conflicts []string
	for _, path := range paths {
		// The `overlay` CTE re-reads this ONE path's current row as part of
		// this same statement — see the doc comment above for why that
		// matters. If the overlay row somehow vanished between the listing
		// query above and this statement (nothing in this codebase deletes
		// an overlay row today, so this is not a reachable case in
		// practice), the CTE returns no rows, the INSERT ... SELECT inserts
		// nothing, and RowsAffected() is 0 — reported as a conflict, which
		// is a harmless mislabel for a case that cannot currently occur.
		tag, execErr := s.pool.Exec(ctx, `
			WITH overlay AS (
				SELECT content_hash, size_bytes, COALESCE(base_hash, '') AS base_hash
				FROM workspace_files
				WHERE org_id = $1 AND space_id = $2 AND run_id = $3 AND path = $4
			)
			INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes, updated_at)
			SELECT $1, $2, NULL, $4, overlay.content_hash, overlay.size_bytes, now()
			FROM overlay
			ON CONFLICT (org_id, space_id, COALESCE(run_id, ''), path)
			DO UPDATE SET content_hash = EXCLUDED.content_hash, size_bytes = EXCLUDED.size_bytes, updated_at = now()
			WHERE workspace_files.content_hash = (SELECT base_hash FROM overlay)
			   OR workspace_files.content_hash = EXCLUDED.content_hash
		`, orgID, spaceID, runID, path)
		if execErr != nil {
			return nil, fmt.Errorf("promote workspace path %q: %w", path, execErr)
		}
		if tag.RowsAffected() == 0 {
			conflicts = append(conflicts, path)
		}
	}
	return conflicts, nil
}
