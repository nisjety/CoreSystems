package threadworkspace

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// DefaultRetention is how long a thread's workspace may sit idle before the
// sweeper erases it. Thirty days is the DEC-23 default idle TTL.
const DefaultRetention = 30 * 24 * time.Hour

// DefaultSweepInterval is how often the sweeper looks for idle threads.
const DefaultSweepInterval = time.Hour

// sweepBatch bounds the threads one pass erases, so a long backlog is worked off
// over several intervals instead of in one pass.
const sweepBatch = 100

// ErrThreadShared refuses to erase a thread id that another owner also holds in
// the same org. Blobs live under the org and thread id alone, so erasing a
// shared id would delete the other owner's blobs. Nothing is removed.
var ErrThreadShared = errors.New("thread workspace is held by another owner")

// ThreadKey names one thread's workspace: the owner-scoped key its header and
// its files share.
type ThreadKey struct {
	OrgID    string
	OwnerID  string
	ThreadID string
}

// BlobEraser removes a thread's blobs from the content-addressed store. The
// objectstore client is the production implementation.
type BlobEraser interface {
	DeleteThreadBlobs(ctx context.Context, orgID, threadID string) (int64, error)
}

// Eraser deletes thread workspace rows. Blobs are removed afterwards by the
// caller, and only once the rows are gone, so an erase that fails halfway can
// never leave a row pointing at a blob that was already removed.
type Eraser struct {
	pool database
}

// NewEraser constructs an Eraser over pool.
func NewEraser(pool *pgxpool.Pool) (*Eraser, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Eraser{pool: pool}, nil
}

// Erase removes one thread's files and header in one transaction and reports how
// many files it removed. Erasing a thread that holds nothing removes nothing and
// is not an error, so a retried erase finishes what the first one started.
func (e *Eraser) Erase(ctx context.Context, orgID, ownerID, threadID string) (int64, error) {
	tx, err := e.pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("begin thread workspace erase: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	files, err := eraseRows(ctx, tx, orgID, ownerID, threadID)
	if err != nil {
		return 0, err
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("commit thread workspace erase: %w", err)
	}
	return files, nil
}

// EraseIdle erases the thread only if it is still idle. The header is locked and
// its last use is read under that lock, so an apply or a read that lands after
// the sweep chose the thread keeps it. It reports whether it erased and how many
// files it removed.
func (e *Eraser) EraseIdle(ctx context.Context, key ThreadKey, cutoff time.Time) (bool, int64, error) {
	tx, err := e.pool.Begin(ctx)
	if err != nil {
		return false, 0, fmt.Errorf("begin idle thread workspace erase: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var lastUsed time.Time
	err = tx.QueryRow(ctx, `
		SELECT last_used_at FROM thread_workspaces
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
		FOR UPDATE
	`, key.OrgID, key.OwnerID, key.ThreadID).Scan(&lastUsed)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, 0, nil
	}
	if err != nil {
		return false, 0, fmt.Errorf("lock idle thread workspace: %w", err)
	}
	if !lastUsed.Before(cutoff) {
		return false, 0, nil
	}

	files, err := eraseRows(ctx, tx, key.OrgID, key.OwnerID, key.ThreadID)
	if err != nil {
		return false, 0, err
	}
	if err := tx.Commit(ctx); err != nil {
		return false, 0, fmt.Errorf("commit idle thread workspace erase: %w", err)
	}
	return true, files, nil
}

// ListIdle returns up to limit threads whose last use is before cutoff, oldest
// first.
func (e *Eraser) ListIdle(ctx context.Context, cutoff time.Time, limit int) ([]ThreadKey, error) {
	rows, err := e.pool.Query(ctx, `
		SELECT org_id, owner_id, thread_id FROM thread_workspaces
		WHERE last_used_at < $1
		ORDER BY last_used_at
		LIMIT $2
	`, cutoff, limit)
	if err != nil {
		return nil, fmt.Errorf("list idle thread workspaces: %w", err)
	}
	defer rows.Close()

	var keys []ThreadKey
	for rows.Next() {
		var key ThreadKey
		if err := rows.Scan(&key.OrgID, &key.OwnerID, &key.ThreadID); err != nil {
			return nil, fmt.Errorf("scan idle thread workspace: %w", err)
		}
		keys = append(keys, key)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list idle thread workspaces: %w", err)
	}
	return keys, nil
}

// EraseStaleLeases deletes the thread lease rows that are destroyed (state 4) or
// expired, and that were created before cutoff, and reports how many went. A
// thread lease names its owner and its thread id, so the row keeps that link after
// the workspace is gone. The statement is idempotent, so replicas may run it at
// once, and Postgres rechecks the predicate on any row an acquire slides forward
// while it runs, so a lease in use again is kept.
func (e *Eraser) EraseStaleLeases(ctx context.Context, cutoff time.Time) (int64, error) {
	tag, err := e.pool.Exec(ctx, `
		DELETE FROM leases
		WHERE scope_type = 'thread'
		  AND (state = 4 OR expires_at < $1)
		  AND created_at < $1
	`, cutoff)
	if err != nil {
		return 0, fmt.Errorf("erase stale thread leases: %w", err)
	}
	return tag.RowsAffected(), nil
}

// eraseRows deletes a thread's files and then its header, inside the caller's
// transaction, and reports how many files went.
func eraseRows(ctx context.Context, tx pgx.Tx, orgID, ownerID, threadID string) (int64, error) {
	var shared bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM thread_workspaces
			WHERE org_id = $1 AND thread_id = $2 AND owner_id <> $3
		)
	`, orgID, threadID, ownerID).Scan(&shared); err != nil {
		return 0, fmt.Errorf("check thread workspace owners: %w", err)
	}
	if shared {
		return 0, ErrThreadShared
	}
	files, err := tx.Exec(ctx, `
		DELETE FROM thread_workspace_files
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID)
	if err != nil {
		return 0, fmt.Errorf("erase thread workspace files: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		DELETE FROM thread_workspaces
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, orgID, ownerID, threadID); err != nil {
		return 0, fmt.Errorf("erase thread workspace header: %w", err)
	}
	return files.RowsAffected(), nil
}

// idleEraser is the part of Eraser the sweeper needs, so its tests need no
// database.
type idleEraser interface {
	ListIdle(ctx context.Context, cutoff time.Time, limit int) ([]ThreadKey, error)
	EraseIdle(ctx context.Context, key ThreadKey, cutoff time.Time) (bool, int64, error)
	EraseStaleLeases(ctx context.Context, cutoff time.Time) (int64, error)
}

// Sweeper erases thread workspaces idle past the retention window (DEC-23), and
// the thread lease rows that outlive them. It follows the process sweeper: a pass
// runs at once on start and then once per interval, and a failed pass is logged
// and tried again rather than stopping the service.
type Sweeper struct {
	rows      idleEraser
	blobs     BlobEraser
	retention time.Duration
	interval  time.Duration
}

// NewSweeper constructs a sweeper with the default cadence.
func NewSweeper(rows idleEraser, blobs BlobEraser, retention time.Duration) *Sweeper {
	return &Sweeper{rows: rows, blobs: blobs, retention: retention, interval: DefaultSweepInterval}
}

// WithTiming returns a copy using the given cadence. A non-positive interval
// keeps the default.
func (s *Sweeper) WithTiming(interval time.Duration) *Sweeper {
	out := *s
	if interval > 0 {
		out.interval = interval
	}
	return &out
}

// Start runs the sweep loop until ctx is cancelled.
func (s *Sweeper) Start(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	s.sweepAndLog(ctx, time.Now().UTC())
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.sweepAndLog(ctx, time.Now().UTC())
		}
	}
}

func (s *Sweeper) sweepAndLog(ctx context.Context, now time.Time) {
	if _, err := s.sweep(ctx, now); err != nil {
		if ctx.Err() != nil {
			return
		}
		slog.Warn("thread workspace retention sweep failed", "error", err)
	}
}

// sweep erases every thread idle past the retention window as of now and returns
// how many it erased. The rows go first and the blobs after, so a blob failure
// leaves orphaned blobs that no row reaches, and that failure is logged for an
// operator instead of being retried. The stale lease rows go last, so a failure
// there cannot hold back an erasure in the same pass.
func (s *Sweeper) sweep(ctx context.Context, now time.Time) (int, error) {
	cutoff := now.Add(-s.retention)
	keys, err := s.rows.ListIdle(ctx, cutoff, sweepBatch)
	if err != nil {
		return 0, err
	}
	erased := 0
	for _, key := range keys {
		ok, files, err := s.rows.EraseIdle(ctx, key, cutoff)
		if errors.Is(err, ErrThreadShared) {
			slog.Warn("idle thread workspace kept: its thread id is held by another owner", "org_id", key.OrgID, "thread_id", key.ThreadID)
			continue
		}
		if err != nil {
			return erased, err
		}
		if !ok {
			continue
		}
		erased++
		slog.Info("thread workspace erased after retention", "org_id", key.OrgID, "thread_id", key.ThreadID, "files", files)
		if _, err := s.blobs.DeleteThreadBlobs(ctx, key.OrgID, key.ThreadID); err != nil {
			slog.Error("thread workspace blobs were not removed after their rows were erased", "org_id", key.OrgID, "thread_id", key.ThreadID, "error", err)
		}
	}
	stale, err := s.rows.EraseStaleLeases(ctx, cutoff)
	if err != nil {
		return erased, err
	}
	if stale > 0 {
		slog.Info("stale thread leases erased after retention", "leases", stale)
	}
	return erased, nil
}

// RetentionFromEnv reads SANDBOX_MANAGER_THREAD_RETENTION_DAYS. An unset variable
// takes the DEC-23 default. A set but unparseable or non-positive one fails
// startup, because a typo must not quietly change how long a user's files are
// kept.
func RetentionFromEnv(getenv func(string) string) (time.Duration, error) {
	raw := getenv("SANDBOX_MANAGER_THREAD_RETENTION_DAYS")
	if raw == "" {
		return DefaultRetention, nil
	}
	days, err := strconv.Atoi(raw)
	if err != nil || days <= 0 {
		return 0, fmt.Errorf("SANDBOX_MANAGER_THREAD_RETENTION_DAYS must be a positive whole number of days")
	}
	return time.Duration(days) * 24 * time.Hour, nil
}
