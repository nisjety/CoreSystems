// Package threadworkspace holds the per-thread workspace: the files one user's
// thread keeps across runs (WS22-S1, DEC-23). It is deliberately a different
// table family from the Space workspace in internal/workspace. A thread row
// is keyed by (org_id, owner_id, thread_id) and a Space row by (org_id,
// space_id, run_id, path), and the two never share a table or an RPC.
//
// PlanChanges holds every rule a batch has to satisfy: per-path compare-and-
// swap on base_hash, idempotent retries, and the file and byte caps. Both the
// Postgres Store here and the in-memory store in the server package apply its
// plan, so the rules are written once and the tests of the rules need no
// database.
package threadworkspace

import (
	"context"
	"errors"
	"fmt"
	"path"
	"strconv"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// DEC-23 D6 caps. A thread is small by design: these bound what it can hold
// and what one apply can add.
const (
	DefaultMaxFiles int64 = 256
	DefaultMaxBytes int64 = 64 << 20
)

// maxPathBytes bounds one path so a single row cannot carry an unbounded key.
const maxPathBytes = 1024

// ErrQuotaExceeded refuses a batch whose result would exceed a cap. Nothing in
// the batch is applied when it is returned.
var ErrQuotaExceeded = errors.New("thread workspace quota exceeded")

// ErrInvalidChange refuses a malformed batch: a bad path, a missing hash, or
// one path named twice.
var ErrInvalidChange = errors.New("invalid thread workspace change")

// Limits caps one thread's workspace.
type Limits struct {
	MaxFiles int64
	MaxBytes int64
}

// LimitsFromEnv reads SANDBOX_MANAGER_THREAD_MAX_FILES and
// SANDBOX_MANAGER_THREAD_MAX_BYTES. An unset variable takes its DEC-23 default;
// a set but unparseable or non-positive one fails startup rather than silently
// loosening a cap.
func LimitsFromEnv(getenv func(string) string) (Limits, error) {
	limits := Limits{MaxFiles: DefaultMaxFiles, MaxBytes: DefaultMaxBytes}
	if raw := getenv("SANDBOX_MANAGER_THREAD_MAX_FILES"); raw != "" {
		n, err := strconv.ParseInt(raw, 10, 64)
		if err != nil || n <= 0 {
			return Limits{}, fmt.Errorf("SANDBOX_MANAGER_THREAD_MAX_FILES must be a positive integer")
		}
		limits.MaxFiles = n
	}
	if raw := getenv("SANDBOX_MANAGER_THREAD_MAX_BYTES"); raw != "" {
		n, err := strconv.ParseInt(raw, 10, 64)
		if err != nil || n <= 0 {
			return Limits{}, fmt.Errorf("SANDBOX_MANAGER_THREAD_MAX_BYTES must be a positive integer")
		}
		limits.MaxBytes = n
	}
	return limits, nil
}

// File is one path a thread holds now.
type File struct {
	Path        string
	ContentHash string
	SizeBytes   int64
}

// Change is one file the caller wrote. BaseHash is the content hash the caller
// last saw at Path. Empty means the caller believed the path did not exist.
type Change struct {
	Path        string
	ContentHash string
	SizeBytes   int64
	BaseHash    string
}

// Deletion is one path the caller removed, with the hash it believed it had.
type Deletion struct {
	Path     string
	BaseHash string
}

// Snapshot is the thread's files at one revision.
type Snapshot struct {
	Files    []File
	Revision int64
}

// Result is the outcome of one applied batch.
type Result struct {
	Conflicts  []string
	Revision   int64
	FileCount  int64
	TotalBytes int64
}

// Plan is what a batch does to the current files. It is built, never written,
// by PlanChanges.
type Plan struct {
	Upserts    []File
	Deletes    []string
	Conflicts  []string
	FileCount  int64
	TotalBytes int64
}

// Changed reports whether the plan writes anything. An all-no-op retry changes
// nothing, so it must not advance the revision either.
func (p Plan) Changed() bool {
	return len(p.Upserts) > 0 || len(p.Deletes) > 0
}

// PlanChanges decides a batch against the files a thread holds now.
//
// Each path is compare-and-swapped on its own, so one conflict never blocks
// another path from applying. A change applies when the path is absent and
// the caller expected absence, or when the path still holds the caller's
// base_hash. A change whose content the path already holds is a retry and is
// a no-op, not a conflict. Anything else is reported in Conflicts and the
// current content is left as it is. A delete follows the same rules, and a
// delete of a path that is already gone is a no-op.
//
// The caps are checked on the state after the whole batch. A batch that would
// exceed either cap returns ErrQuotaExceeded and applies nothing.
func PlanChanges(current map[string]File, changes []Change, deletions []Deletion, limits Limits) (Plan, error) {
	seen := make(map[string]struct{}, len(changes)+len(deletions))
	for _, c := range changes {
		if err := checkPath(c.Path, seen); err != nil {
			return Plan{}, err
		}
		if c.ContentHash == "" || c.SizeBytes < 0 {
			return Plan{}, fmt.Errorf("%w: content_hash and a non-negative size are required for %q", ErrInvalidChange, c.Path)
		}
	}
	for _, d := range deletions {
		if err := checkPath(d.Path, seen); err != nil {
			return Plan{}, err
		}
	}

	after := make(map[string]File, len(current)+len(changes))
	for p, f := range current {
		after[p] = f
	}

	var plan Plan
	for _, c := range changes {
		cur, exists := current[c.Path]
		switch {
		case exists && cur.ContentHash == c.ContentHash:
			continue
		case exists && cur.ContentHash == c.BaseHash:
		case !exists && c.BaseHash == "":
		default:
			plan.Conflicts = append(plan.Conflicts, c.Path)
			continue
		}
		f := File{Path: c.Path, ContentHash: c.ContentHash, SizeBytes: c.SizeBytes}
		plan.Upserts = append(plan.Upserts, f)
		after[c.Path] = f
	}
	for _, d := range deletions {
		cur, exists := current[d.Path]
		switch {
		case !exists:
			continue
		case cur.ContentHash == d.BaseHash:
			plan.Deletes = append(plan.Deletes, d.Path)
			delete(after, d.Path)
		default:
			plan.Conflicts = append(plan.Conflicts, d.Path)
		}
	}

	for _, f := range after {
		plan.FileCount++
		plan.TotalBytes += f.SizeBytes
	}
	if plan.Changed() && (plan.FileCount > limits.MaxFiles || plan.TotalBytes > limits.MaxBytes) {
		return Plan{}, fmt.Errorf("%w: the batch would hold %d files and %d bytes (caps %d and %d)",
			ErrQuotaExceeded, plan.FileCount, plan.TotalBytes, limits.MaxFiles, limits.MaxBytes)
	}
	return plan, nil
}

// checkPath refuses a path that is empty, absolute, not clean, or that climbs
// out of the workspace root, and refuses one already named in this batch.
func checkPath(p string, seen map[string]struct{}) error {
	if p == "" || len(p) > maxPathBytes || strings.HasPrefix(p, "/") ||
		path.Clean(p) != p || p == ".." || strings.HasPrefix(p, "../") {
		return fmt.Errorf("%w: %q is not a clean relative path", ErrInvalidChange, p)
	}
	if _, dup := seen[p]; dup {
		return fmt.Errorf("%w: %q is named more than once", ErrInvalidChange, p)
	}
	seen[p] = struct{}{}
	return nil
}

type database interface {
	Begin(context.Context) (pgx.Tx, error)
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// Store is the durable per-thread workspace over thread_workspaces and
// thread_workspace_files (migration 0005).
type Store struct {
	pool database
}

// NewStore constructs a Store over pool. There is no in-memory fallback here:
// a thread workspace that vanished on restart would be a silent loss.
func NewStore(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Store{pool: pool}, nil
}

// Get returns the thread's files and revision. A thread that has never been
// written reads as empty at revision 0.
func (s *Store) Get(ctx context.Context, orgID, ownerID, threadID string) (Snapshot, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT path, content_hash, size_bytes
		FROM thread_workspace_files
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
		ORDER BY path
	`, orgID, ownerID, threadID)
	if err != nil {
		return Snapshot{}, fmt.Errorf("get thread workspace files: %w", err)
	}
	files, err := scanFiles(rows)
	if err != nil {
		return Snapshot{}, err
	}

	var revision int64
	err = s.pool.QueryRow(ctx, `
		SELECT revision FROM thread_workspaces
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID).Scan(&revision)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return Snapshot{}, fmt.Errorf("get thread workspace revision: %w", err)
	}

	// A read keeps the thread alive for the retention sweeper, which deletes
	// by idle time. Without this a thread a user only reads would be swept.
	if _, err := s.pool.Exec(ctx, `
		UPDATE thread_workspaces SET last_used_at = now()
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID); err != nil {
		return Snapshot{}, fmt.Errorf("touch thread workspace: %w", err)
	}
	return Snapshot{Files: files, Revision: revision}, nil
}

// Apply plans and writes one batch under a row lock on the thread's header,
// so two applies to the same thread serialize and each one's base_hash is
// checked against what the other committed. Nothing is written unless the
// whole batch is valid and within the caps.
func (s *Store) Apply(ctx context.Context, orgID, ownerID, threadID string, changes []Change, deletions []Deletion, limits Limits) (Result, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return Result{}, fmt.Errorf("begin thread workspace apply: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, `
		INSERT INTO thread_workspaces (org_id, owner_id, thread_id)
		VALUES ($1, $2, $3)
		ON CONFLICT (org_id, owner_id, thread_id) DO NOTHING
	`, orgID, ownerID, threadID); err != nil {
		return Result{}, fmt.Errorf("ensure thread workspace: %w", err)
	}
	var revision int64
	if err := tx.QueryRow(ctx, `
		SELECT revision FROM thread_workspaces
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
		FOR UPDATE
	`, orgID, ownerID, threadID).Scan(&revision); err != nil {
		return Result{}, fmt.Errorf("lock thread workspace: %w", err)
	}

	rows, err := tx.Query(ctx, `
		SELECT path, content_hash, size_bytes
		FROM thread_workspace_files
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID)
	if err != nil {
		return Result{}, fmt.Errorf("list thread workspace files: %w", err)
	}
	listed, err := scanFiles(rows)
	if err != nil {
		return Result{}, err
	}
	current := make(map[string]File, len(listed))
	for _, f := range listed {
		current[f.Path] = f
	}

	plan, err := PlanChanges(current, changes, deletions, limits)
	if err != nil {
		return Result{}, err
	}
	for _, f := range plan.Upserts {
		if _, err := tx.Exec(ctx, `
			INSERT INTO thread_workspace_files (org_id, owner_id, thread_id, path, content_hash, size_bytes, updated_at)
			VALUES ($1, $2, $3, $4, $5, $6, now())
			ON CONFLICT (org_id, owner_id, thread_id, path)
			DO UPDATE SET content_hash = EXCLUDED.content_hash, size_bytes = EXCLUDED.size_bytes, updated_at = now()
		`, orgID, ownerID, threadID, f.Path, f.ContentHash, f.SizeBytes); err != nil {
			return Result{}, fmt.Errorf("write thread workspace file %q: %w", f.Path, err)
		}
	}
	for _, p := range plan.Deletes {
		if _, err := tx.Exec(ctx, `
			DELETE FROM thread_workspace_files
			WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3 AND path = $4
		`, orgID, ownerID, threadID, p); err != nil {
			return Result{}, fmt.Errorf("delete thread workspace file %q: %w", p, err)
		}
	}
	if plan.Changed() {
		revision++
	}
	if _, err := tx.Exec(ctx, `
		UPDATE thread_workspaces
		SET revision = $4, file_count = $5, total_bytes = $6, last_used_at = now()
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID, revision, plan.FileCount, plan.TotalBytes); err != nil {
		return Result{}, fmt.Errorf("update thread workspace header: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return Result{}, fmt.Errorf("commit thread workspace apply: %w", err)
	}
	return Result{
		Conflicts:  plan.Conflicts,
		Revision:   revision,
		FileCount:  plan.FileCount,
		TotalBytes: plan.TotalBytes,
	}, nil
}

func scanFiles(rows pgx.Rows) ([]File, error) {
	defer rows.Close()
	var files []File
	for rows.Next() {
		var f File
		if err := rows.Scan(&f.Path, &f.ContentHash, &f.SizeBytes); err != nil {
			return nil, fmt.Errorf("scan thread workspace file: %w", err)
		}
		files = append(files, f)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list thread workspace files: %w", err)
	}
	return files, nil
}
