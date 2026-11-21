package threadworkspace

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"time"

	"github.com/jackc/pgx/v5"
)

// The orphan sweep (O-13, D-f06-A) erases the workspace of a thread that no longer
// exists. Thread ownership is checked on every request, but a thread deleted
// elsewhere leaves its rows and blobs here until something removes them. The
// retention sweep removes them after 30 idle days and session-core's service erase
// removes them on a delete it reports; this pass is the net under both, and it
// reuses their machinery: the same row erase writes the same tombstone, the same
// blob removal runs after it, and a blob failure is retried by the F-f06-02 retry.
//
// It reads thread ids only. ZDR needs nothing extra: a ZDR run never writes a thread
// workspace (F-f02-05), and this pass reads no content.

// DefaultOrphanGrace is how old a workspace must be before the sweep asks about its
// thread, so a workspace written moments ago is never a candidate.
const DefaultOrphanGrace = 24 * time.Hour

// orphanPageSize is how many workspaces one existence question covers at most, and
// orphanPagesPerPass bounds a pass, so a long table is worked through over several
// intervals instead of in one pass. The sweeper remembers where it stopped.
const (
	orphanPageSize     = 100
	orphanPagesPerPass = 10
)

// ThreadExistence answers which of an org's thread ids exist. It must answer
// explicitly: a thread id missing from the map is unknown, and the sweep keeps its
// workspace. Only an explicit false is "the thread is gone". Any error means the
// sweep keeps everything it asked about.
type ThreadExistence interface {
	ThreadsExist(ctx context.Context, orgID string, threadIDs []string) (map[string]bool, error)
}

// orphanEraser is the part of Eraser the orphan pass needs, so its tests need no
// database.
type orphanEraser interface {
	ListOlderThan(ctx context.Context, cutoff time.Time, after ThreadKey, limit int) ([]ThreadKey, error)
	EraseOrphan(ctx context.Context, key ThreadKey, cutoff time.Time) (bool, int64, error)
	CompleteBlobErasure(ctx context.Context, key ThreadKey) error
}

// orphanSweep is one sweeper's orphan configuration and its place in the table.
type orphanSweep struct {
	rows   orphanEraser
	exists ThreadExistence
	grace  time.Duration
	// cursor is the last workspace key the previous page ended on. Keys are walked in
	// primary-key order and the walk starts over once the end is reached. It is
	// touched only by the sweep goroutine.
	cursor ThreadKey
}

// WithOrphanSweep returns a copy that also erases the workspaces of threads that no
// longer exist, asking exists which threads those are. A non-positive grace takes
// DefaultOrphanGrace. Without it the pass does not run.
func (s *Sweeper) WithOrphanSweep(rows orphanEraser, exists ThreadExistence, grace time.Duration) *Sweeper {
	out := *s
	if grace <= 0 {
		grace = DefaultOrphanGrace
	}
	out.orphans = orphanSweep{rows: rows, exists: exists, grace: grace}
	return &out
}

// OrphanGraceFromEnv reads SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS. An unset
// variable takes the 24 hour default. A set but unparseable or non-positive one
// fails startup, like the retention window: a typo must not quietly widen or
// shrink the time a workspace is safe from this pass.
func OrphanGraceFromEnv(getenv func(string) string) (time.Duration, error) {
	raw := getenv("SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS")
	if raw == "" {
		return DefaultOrphanGrace, nil
	}
	hours, err := strconv.Atoi(raw)
	if err != nil || hours <= 0 {
		return 0, fmt.Errorf("SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS must be a positive whole number of hours")
	}
	return time.Duration(hours) * time.Hour, nil
}

// ListOlderThan returns up to limit workspaces created before cutoff whose primary
// key sorts after the given one, in primary-key order. The zero key starts at the
// beginning.
func (e *Eraser) ListOlderThan(ctx context.Context, cutoff time.Time, after ThreadKey, limit int) ([]ThreadKey, error) {
	rows, err := e.pool.Query(ctx, `
		SELECT org_id, owner_id, thread_id FROM thread_workspaces
		WHERE created_at < $1
		  AND (org_id, owner_id, thread_id) > ($2, $3, $4)
		ORDER BY org_id, owner_id, thread_id
		LIMIT $5
	`, cutoff, after.OrgID, after.OwnerID, after.ThreadID, limit)
	if err != nil {
		return nil, fmt.Errorf("list thread workspaces for the orphan sweep: %w", err)
	}
	defer rows.Close()

	var keys []ThreadKey
	for rows.Next() {
		var key ThreadKey
		if err := rows.Scan(&key.OrgID, &key.OwnerID, &key.ThreadID); err != nil {
			return nil, fmt.Errorf("scan thread workspace for the orphan sweep: %w", err)
		}
		keys = append(keys, key)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list thread workspaces for the orphan sweep: %w", err)
	}
	return keys, nil
}

// EraseOrphan erases the workspace only if it is still older than cutoff. The header
// is locked and its creation time read under that lock, so a workspace that was
// erased and written again since the sweep listed it is a new one and is kept. It
// writes the same tombstone every erase writes. Unlike EraseIdle it does not refuse
// a thread id that another owner also holds: the caller has learned that the thread
// does not exist, so every owner's rows under it are orphans too.
func (e *Eraser) EraseOrphan(ctx context.Context, key ThreadKey, cutoff time.Time) (bool, int64, error) {
	tx, err := e.pool.Begin(ctx)
	if err != nil {
		return false, 0, fmt.Errorf("begin orphan thread workspace erase: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var createdAt time.Time
	err = tx.QueryRow(ctx, `
		SELECT created_at FROM thread_workspaces
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
		FOR UPDATE
	`, key.OrgID, key.OwnerID, key.ThreadID).Scan(&createdAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, 0, nil
	}
	if err != nil {
		return false, 0, fmt.Errorf("lock orphan thread workspace: %w", err)
	}
	if !createdAt.Before(cutoff) {
		return false, 0, nil
	}
	files, err := eraseOwnedRows(ctx, tx, key.OrgID, key.OwnerID, key.ThreadID)
	if err != nil {
		return false, 0, err
	}
	if err := tx.Commit(ctx); err != nil {
		return false, 0, fmt.Errorf("commit orphan thread workspace erase: %w", err)
	}
	return true, files, nil
}

// sweepOrphans runs the orphan pass and returns how many workspaces it erased. It
// walks up to orphanPagesPerPass pages from where the last pass stopped.
func (s *Sweeper) sweepOrphans(ctx context.Context, now time.Time) (int, error) {
	o := &s.orphans
	if o.rows == nil || o.exists == nil {
		return 0, nil
	}
	cutoff := now.Add(-o.grace)
	erased := 0
	for page := 0; page < orphanPagesPerPass; page++ {
		if err := ctx.Err(); err != nil {
			return erased, err
		}
		keys, err := o.rows.ListOlderThan(ctx, cutoff, o.cursor, orphanPageSize)
		if err != nil {
			return erased, err
		}
		if len(keys) == 0 {
			o.cursor = ThreadKey{}
			return erased, nil
		}
		o.cursor = keys[len(keys)-1]
		n, err := s.sweepOrphanPage(ctx, cutoff, keys)
		erased += n
		if err != nil {
			return erased, err
		}
		if len(keys) < orphanPageSize {
			o.cursor = ThreadKey{}
			return erased, nil
		}
	}
	return erased, nil
}

// threadGroup is one (org, thread) pair and the owners whose workspace the pass
// erased under it.
type threadGroup struct {
	orgID    string
	threadID string
	owners   []string
}

// sweepOrphanPage asks about one page of workspaces, an org at a time because the
// existence read is org-scoped, and erases those whose thread is explicitly gone.
// The blobs of each erased thread go after all its rows in the page, once.
func (s *Sweeper) sweepOrphanPage(ctx context.Context, cutoff time.Time, keys []ThreadKey) (int, error) {
	o := &s.orphans
	byOrg := map[string][]ThreadKey{}
	var orgOrder []string
	for _, key := range keys {
		if _, seen := byOrg[key.OrgID]; !seen {
			orgOrder = append(orgOrder, key.OrgID)
		}
		byOrg[key.OrgID] = append(byOrg[key.OrgID], key)
	}

	erased := 0
	var groups []*threadGroup
	groupIndex := map[[2]string]*threadGroup{}
	for _, orgID := range orgOrder {
		orgKeys := byOrg[orgID]
		ids := make([]string, 0, len(orgKeys))
		seen := map[string]bool{}
		for _, key := range orgKeys {
			if !seen[key.ThreadID] {
				seen[key.ThreadID] = true
				ids = append(ids, key.ThreadID)
			}
		}
		answers, err := o.exists.ThreadsExist(ctx, orgID, ids)
		if err != nil {
			if ctx.Err() != nil {
				return erased, ctx.Err()
			}
			// Fail closed: not knowing is a reason to keep every workspace asked about.
			slog.Warn("thread existence could not be read; the orphan sweep keeps these workspaces", "org_id", orgID, "threads", len(ids))
			continue
		}
		for _, key := range orgKeys {
			exists, answered := answers[key.ThreadID]
			if !answered || exists {
				continue
			}
			ok, files, err := o.rows.EraseOrphan(ctx, key, cutoff)
			if err != nil {
				return erased, err
			}
			if !ok {
				continue
			}
			erased++
			slog.Info("thread workspace erased: its thread no longer exists", "org_id", key.OrgID, "thread_id", key.ThreadID, "files", files)
			gk := [2]string{key.OrgID, key.ThreadID}
			group := groupIndex[gk]
			if group == nil {
				group = &threadGroup{orgID: key.OrgID, threadID: key.ThreadID}
				groupIndex[gk] = group
				groups = append(groups, group)
			}
			group.owners = append(group.owners, key.OwnerID)
		}
	}

	for _, group := range groups {
		// The tombstones the erase wrote cover a failure here: they stay for the retry,
		// which waits until no header is left under the thread. Only a successful removal
		// closes them.
		if _, err := s.blobs.DeleteThreadBlobs(ctx, group.orgID, group.threadID); err != nil {
			slog.Error("orphan thread workspace blobs were not removed after their rows were erased; the erasure is kept for a retry", "org_id", group.orgID, "thread_id", group.threadID)
			continue
		}
		for _, ownerID := range group.owners {
			key := ThreadKey{OrgID: group.orgID, OwnerID: ownerID, ThreadID: group.threadID}
			if err := o.rows.CompleteBlobErasure(ctx, key); err != nil && ctx.Err() == nil {
				slog.Warn("thread blob erasure is not closed; the next pass closes it", "org_id", group.orgID, "thread_id", group.threadID)
			}
		}
	}
	return erased, nil
}
