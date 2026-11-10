package pg

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const defaultMaxPage = 500

// cursor is an opaque (created_at, id) pair for keyset pagination.
// Encoded as base64("<created_at>|<id>"). Decoded filters rows where
// (created_at, id) < (cursor.created_at, cursor.id) — newest first.
type cursor struct {
	CreatedAt int64
	ID        string
}

func decodeCursor(s string) (*cursor, error) {
	if s == "" {
		return nil, nil
	}
	raw, err := base64.RawURLEncoding.DecodeString(s)
	if err != nil {
		return nil, fmt.Errorf("cursor: %w", err)
	}
	parts := strings.SplitN(string(raw), "|", 2)
	if len(parts) != 2 {
		return nil, errors.New("cursor: malformed")
	}
	c := &cursor{ID: parts[1]}
	if _, err := fmt.Sscan(parts[0], &c.CreatedAt); err != nil {
		return nil, fmt.Errorf("cursor ts: %w", err)
	}
	return c, nil
}

func encodeCursor(createdAt int64, id string) string {
	return base64.RawURLEncoding.EncodeToString(
		[]byte(fmt.Sprintf("%d|%s", createdAt, id)),
	)
}

// ---- jobs -----------------------------------------------------------------

type jobsStore struct{ pool *pgxpool.Pool }

// jobColumns is the one column list every jobs SELECT uses, in the order
// scanJob reads it. Adding a column means changing this and scanJob, not seven
// queries that must agree.
const jobColumns = `id, org_id, kind, status, policy, params, schedule_id, created_at, run_id, idempotency_key, statistics, origin_type, origin_actor, origin_ref, created_by, desired_state, desired_seq, applied_seq, workflow_id, deadline_at, terminal_reason, finished_at`

type jobScanner interface{ Scan(dest ...any) error }

// scanJob reads one row selected with jobColumns. A row whose JSON columns do
// not decode keeps the zero value for that field, as the per-site code did.
func scanJob(row jobScanner) (store.Job, error) {
	var (
		j          store.Job
		policy     []byte
		params     []byte
		statistics []byte
		sid        *string
		rid        *string
		idem       *string
		deadline   *int64
		finished   *int64
	)
	if err := row.Scan(&j.ID, &j.OrgID, &j.Kind, &j.Status, &policy, &params, &sid, &j.CreatedAt, &rid, &idem, &statistics,
		&j.Origin.Type, &j.Origin.Actor, &j.Origin.Ref, &j.Origin.CreatedBy,
		&j.DesiredState, &j.DesiredSeq, &j.AppliedSeq, &j.WorkflowID, &deadline, &j.TerminalReason, &finished); err != nil {
		return store.Job{}, err
	}
	_ = json.Unmarshal(policy, &j.Policy)
	if len(params) > 0 {
		_ = json.Unmarshal(params, &j.Params)
	}
	if len(statistics) > 0 {
		_ = json.Unmarshal(statistics, &j.Statistics)
	}
	if sid != nil {
		v := quarrycontracts.ID(*sid)
		j.ScheduleID = &v
	}
	if rid != nil {
		v := quarrycontracts.ID(*rid)
		j.RunID = &v
	}
	j.IdempotencyKey = idem
	j.DeadlineAt = deadline
	j.FinishedAt = finished
	return j, nil
}

func (s *jobsStore) Create(j store.Job) error {
	policy, _ := json.Marshal(j.Policy)
	params, _ := json.Marshal(j.Params)
	var sid *string
	if j.ScheduleID != nil {
		v := string(*j.ScheduleID)
		sid = &v
	}
	var rid *string
	if j.RunID != nil {
		v := string(*j.RunID)
		rid = &v
	}
	originType := j.Origin.Type
	if originType == "" {
		originType = "unknown"
	}
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO jobs(id, org_id, kind, status, policy, params, schedule_id, created_at, run_id, idempotency_key,
		   origin_type, origin_actor, origin_ref, created_by, workflow_id, deadline_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)`,
		string(j.ID), j.OrgID, j.Kind, j.Status, policy, params, sid, j.CreatedAt, rid, j.IdempotencyKey,
		originType, j.Origin.Actor, j.Origin.Ref, j.Origin.CreatedBy, j.WorkflowID, j.DeadlineAt)
	return mapPgErr(err)
}

// FindByIdempotencyKey returns the existing job for an Idempotency-Key
// scoped to orgID, if any. Uses the partial unique index on
// (org_id, idempotency_key) for an O(log n) lookup (migration 028, which
// replaced 007's global index); the org_id predicate means a key collision
// with another tenant's job is reported as a miss, never returned across
// tenants, and since 028 the other tenant's key no longer blocks the insert
// either: the same key is unique per org, not across orgs.
func (s *jobsStore) FindByIdempotencyKey(orgID, key string) (store.Job, bool) {
	if key == "" {
		return store.Job{}, false
	}
	j, err := scanJob(s.pool.QueryRow(context.Background(),
		`SELECT `+jobColumns+` FROM jobs WHERE idempotency_key=$1 AND org_id=$2`, key, orgID))
	if err != nil {
		return store.Job{}, false
	}
	return j, true
}

func (s *jobsStore) Get(id quarrycontracts.ID) (store.Job, bool) {
	j, err := scanJob(s.pool.QueryRow(context.Background(),
		`SELECT `+jobColumns+` FROM jobs WHERE id=$1`, string(id)))
	if err != nil {
		return store.Job{}, false
	}
	return j, true
}

// GetByOrg returns a job scoped to org — mirrors sourcesStore.GetByOrg's
// WHERE-clause guard: a cross-tenant id returns (Job{}, false), never
// another org's row.
func (s *jobsStore) GetByOrg(orgID string, id quarrycontracts.ID) (store.Job, bool) {
	j, err := scanJob(s.pool.QueryRow(context.Background(),
		`SELECT `+jobColumns+` FROM jobs WHERE id=$1 AND org_id=$2`, string(id), orgID))
	if err != nil {
		return store.Job{}, false
	}
	return j, true
}

// GetByRunID returns the job that owns a dispatched Temporal run id, using
// the partial index from 006_jobs_run_id.sql. Unscoped by org — see the
// store.JobsStore doc comment for why.
func (s *jobsStore) GetByRunID(runID quarrycontracts.ID) (store.Job, bool) {
	j, err := scanJob(s.pool.QueryRow(context.Background(),
		`SELECT `+jobColumns+` FROM jobs WHERE run_id=$1`, string(runID)))
	if err != nil {
		return store.Job{}, false
	}
	return j, true
}

// Update replaces the mutable fields of a job: status, run_id, statistics, the
// workflow id, the deadline, the terminal reason and the finish time. Kind,
// params, policy, schedule_id and origin are set at create time and never
// change. Used by the orchestrator's jobs dispatcher and by terminal events.
func (s *jobsStore) Update(j store.Job) error {
	tag, err := s.pool.Exec(context.Background(), updateJobSQL, jobUpdateArgs(j)...)
	if err != nil {
		return mapPgErr(err)
	}
	if tag.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// UpdateIfStatus is Update as a compare-and-set: the write happens only while
// the stored status is expected. A job in another status is ErrConflict; a
// missing job is ErrNotFound.
func (s *jobsStore) UpdateIfStatus(j store.Job, expected string) error {
	args := append(jobUpdateArgs(j), expected)
	tag, err := s.pool.Exec(context.Background(), updateJobSQL+` AND status = $9`, args...)
	if err != nil {
		return mapPgErr(err)
	}
	if tag.RowsAffected() > 0 {
		return nil
	}
	var exists int
	if err := s.pool.QueryRow(context.Background(), `SELECT 1 FROM jobs WHERE id=$1`, string(j.ID)).Scan(&exists); err != nil {
		return store.ErrNotFound
	}
	return store.ErrConflict
}

// updateJobSQL writes the mutable job columns of the row with $1 as id. A NULL
// statistics argument keeps what is stored, so a status-only write does not
// erase the statistics a terminal event recorded.
const updateJobSQL = `UPDATE jobs SET status=$2, run_id=$3, statistics=COALESCE($4::jsonb, statistics), workflow_id=$5, deadline_at=$6, terminal_reason=$7, finished_at=$8 WHERE id=$1`

// jobUpdateArgs is the argument list of updateJobSQL for a job.
func jobUpdateArgs(j store.Job) []any {
	var rid *string
	if j.RunID != nil {
		v := string(*j.RunID)
		rid = &v
	}
	var statistics []byte
	if len(j.Statistics) > 0 {
		statistics, _ = json.Marshal(j.Statistics)
	}
	return []any{string(j.ID), j.Status, rid, statistics, j.WorkflowID, j.DeadlineAt, j.TerminalReason, j.FinishedAt}
}

// claimAcceptedSQL leases waiting jobs in one statement. in_use counts what
// each organization already occupies (running, plus leased and not yet
// started); ranked orders every waiting job oldest first within its
// organization; picked keeps those whose rank fits the organization's room,
// first jobs of every organization before any second job, so one tenant's
// backlog cannot crowd out another's. A cap that is not a plain number falls
// back to the default instead of failing the whole claim.
const claimAcceptedSQL = `
WITH in_use AS (
  SELECT org_id, count(*)::int AS n FROM jobs
   WHERE status = 'running' OR (status = 'accepted' AND claimed_until > $1)
   GROUP BY org_id
), ranked AS (
  SELECT j.id, j.org_id, j.created_at,
         row_number() OVER (PARTITION BY j.org_id ORDER BY j.created_at, j.id) AS rn,
         CASE WHEN (j.params->'admission'->>'cap') ~ '^[0-9]{1,6}$' AND (j.params->'admission'->>'cap')::int >= 1
              THEN (j.params->'admission'->>'cap')::int ELSE $2 END AS cap
    FROM jobs j
   WHERE j.status = 'accepted' AND j.kind IN ('scrape', 'crawl', 'batch')
     AND (j.claimed_until IS NULL OR j.claimed_until <= $1)
), picked AS (
  SELECT r.id FROM ranked r LEFT JOIN in_use u ON u.org_id = r.org_id
   WHERE r.rn <= GREATEST(r.cap - COALESCE(u.n, 0), 0)
   ORDER BY r.rn, r.created_at, r.id
   LIMIT $3
)
UPDATE jobs SET claimed_until = $1 + make_interval(secs => $4)
 WHERE id IN (SELECT id FROM picked)
   AND status = 'accepted' AND (claimed_until IS NULL OR claimed_until <= $1)
RETURNING ` + jobColumns

// jobClaimLockKey is the transaction-scoped advisory lock that serialises
// claims. Under READ COMMITTED every statement reads its own snapshot, so two
// claimers that ran the claim statement at the same time could each count the
// same jobs in use and each lease up to the cap. Taking the lock first makes
// the second claimer wait for the first to commit; its statement then starts
// after those leases are committed and sees them.
const jobClaimLockKey int64 = 7245110311

// ClaimAccepted implements store.JobsStore.
func (s *jobsStore) ClaimAccepted(limit int, now time.Time, lease time.Duration, defaultCap int) ([]store.Job, error) {
	if limit <= 0 {
		return nil, nil
	}
	ctx := context.Background()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, mapPgErr(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock($1)`, jobClaimLockKey); err != nil {
		return nil, mapPgErr(err)
	}
	rows, err := tx.Query(ctx, claimAcceptedSQL, now, defaultCap, limit, lease.Seconds())
	if err != nil {
		return nil, mapPgErr(err)
	}
	var out []store.Job
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			rows.Close()
			return nil, err
		}
		out = append(out, j)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, mapPgErr(err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, mapPgErr(err)
	}
	// RETURNING has no order. Hand the jobs back in claim order: each
	// organization's oldest first, round-robin across organizations. An
	// organization's claimed jobs are its oldest waiting ones, so the rank of a
	// job within the returned set is its rank in the claim.
	sort.SliceStable(out, func(a, b int) bool {
		if out[a].CreatedAt != out[b].CreatedAt {
			return out[a].CreatedAt < out[b].CreatedAt
		}
		return out[a].ID < out[b].ID
	})
	rank := make(map[quarrycontracts.ID]int, len(out))
	seen := map[string]int{}
	for _, j := range out {
		seen[j.OrgID]++
		rank[j.ID] = seen[j.OrgID]
	}
	sort.SliceStable(out, func(a, b int) bool { return rank[out[a].ID] < rank[out[b].ID] })
	return out, nil
}

// CountAccepted implements store.JobsStore.
func (s *jobsStore) CountAccepted(orgID string, now time.Time) int {
	var n int
	err := s.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM jobs WHERE org_id = $1 AND status = 'accepted'
		    AND (claimed_until IS NULL OR claimed_until <= $2)`, orgID, now).Scan(&n)
	if err != nil {
		return 0
	}
	return n
}

// FailExpired implements store.JobsStore.
func (s *jobsStore) FailExpired(createdBefore, now time.Time) ([]store.Job, error) {
	rows, err := s.pool.Query(context.Background(),
		`UPDATE jobs SET status = 'failed',
		        params = COALESCE(params, '{}'::jsonb) || jsonb_build_object('failure_reason', 'backlog_expired')
		  WHERE status = 'accepted' AND created_at < $1
		    AND (claimed_until IS NULL OR claimed_until <= $2)
		RETURNING `+jobColumns, createdBefore.UnixMilli(), now)
	if err != nil {
		return nil, mapPgErr(err)
	}
	defer rows.Close()
	var out []store.Job
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, j)
	}
	return out, mapPgErr(rows.Err())
}

func (s *jobsStore) List(limit int, cur string) ([]store.Job, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)

	q := `SELECT ` + jobColumns + ` FROM jobs`
	args := []any{}
	if c != nil {
		q += ` WHERE (created_at, id) < ($1, $2)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ` + fmt.Sprint(limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()

	out := make([]store.Job, 0, limit)
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, j)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

// ListByOrg returns the org's jobs, newest-first — backs GET /v1/jobs.
// Mirrors sourcesStore.ListByOrg's WHERE org_id = $1 guard, using the
// (org_id, kind, created_at) index (migration 010_jobs_org_id.sql).
func (s *jobsStore) ListByOrg(orgID string, limit int, cur string) ([]store.Job, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)

	q := `SELECT ` + jobColumns + ` FROM jobs WHERE org_id = $1`
	args := []any{orgID}
	if c != nil {
		q += ` AND (created_at, id) < ($2, $3)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ` + fmt.Sprint(limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()

	out := make([]store.Job, 0, limit)
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, j)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

// ListByKind returns jobs whose Kind matches exactly AND whose OrgID matches
// orgID, newest-first, using the (org_id, kind, created_at) index (migration
// 010_jobs_org_id.sql) so the WHERE + ORDER BY are index-served.
func (s *jobsStore) ListByKind(orgID, kind string, limit int, cur string) ([]store.Job, string) {
	return s.ListByKindStatus(orgID, kind, nil, limit, cur)
}

// ListByKindStatus is ListByKind restricted to the given statuses; an empty list
// means any status. Backs GET /v1/{kind}/jobs?status=a,b.
func (s *jobsStore) ListByKindStatus(orgID, kind string, statuses []string, limit int, cur string) ([]store.Job, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)

	q := `SELECT ` + jobColumns + ` FROM jobs WHERE org_id = $1 AND kind = $2`
	args := []any{orgID, kind}
	if len(statuses) > 0 {
		args = append(args, statuses)
		q += fmt.Sprintf(` AND status = ANY($%d)`, len(args))
	}
	if c != nil {
		args = append(args, c.CreatedAt, c.ID)
		q += fmt.Sprintf(` AND (created_at, id) < ($%d, $%d)`, len(args)-1, len(args))
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ` + fmt.Sprint(limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()

	out := make([]store.Job, 0, limit)
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, j)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

// ListActiveByOrg returns the organization's jobs that are still in flight
// (accepted, running, paused), oldest first. Served by idx_jobs_active.
func (s *jobsStore) ListActiveByOrg(orgID string, limit int) []store.Job {
	limit = pageLimit(limit, defaultMaxPage)
	rows, err := s.pool.Query(context.Background(),
		`SELECT `+jobColumns+` FROM jobs
		  WHERE org_id = $1 AND status IN ('accepted', 'running', 'paused')
		  ORDER BY created_at ASC, id ASC LIMIT $2`, orgID, limit)
	if err != nil {
		return nil
	}
	defer rows.Close()
	out := make([]store.Job, 0, limit)
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil
		}
		out = append(out, j)
	}
	return out
}

func (s *jobsStore) ListBySchedule(scheduleID quarrycontracts.ID, limit int, cur string) ([]store.Job, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)

	q := `SELECT ` + jobColumns + ` FROM jobs WHERE schedule_id = $1`
	args := []any{string(scheduleID)}
	if c != nil {
		q += ` AND (created_at, id) < ($2, $3)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ` + fmt.Sprint(limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()

	out := make([]store.Job, 0, limit)
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, j)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *jobsStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM jobs WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- stores (named stores) ------------------------------------------------

type storesStore struct{ pool *pgxpool.Pool }

func (s *storesStore) Create(v store.NamedStore) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO stores(id, name, kind, created_at) VALUES ($1,$2,$3,$4)`,
		string(v.ID), v.Name, v.Kind, v.CreatedAt)
	return mapPgErr(err)
}

func (s *storesStore) Get(id quarrycontracts.ID) (store.NamedStore, bool) {
	var v store.NamedStore
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, name, kind, created_at FROM stores WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.Name, &v.Kind, &v.CreatedAt)
	if err != nil {
		return store.NamedStore{}, false
	}
	return v, true
}

func (s *storesStore) List(limit int, cur string) ([]store.NamedStore, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, name, kind, created_at FROM stores`
	args := []any{}
	if c != nil {
		q += ` WHERE (created_at, id) < ($1, $2)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.NamedStore, 0, limit)
	for rows.Next() {
		var v store.NamedStore
		if err := rows.Scan(&v.ID, &v.Name, &v.Kind, &v.CreatedAt); err != nil {
			return nil, ""
		}
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *storesStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM stores WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- snapshots ------------------------------------------------------------

type snapshotsStore struct{ pool *pgxpool.Pool }

func (s *snapshotsStore) Create(v store.Snapshot) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO snapshots(id, run_id, bucket, created_at) VALUES ($1,$2,$3,$4)`,
		string(v.ID), string(v.RunID), v.Bucket, v.CreatedAt)
	return mapPgErr(err)
}

func (s *snapshotsStore) Get(id quarrycontracts.ID) (store.Snapshot, bool) {
	var v store.Snapshot
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, run_id, bucket, created_at FROM snapshots WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.RunID, &v.Bucket, &v.CreatedAt)
	if err != nil {
		return store.Snapshot{}, false
	}
	return v, true
}

func (s *snapshotsStore) List(limit int, cur string) ([]store.Snapshot, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, run_id, bucket, created_at FROM snapshots`
	args := []any{}
	if c != nil {
		q += ` WHERE (created_at, id) < ($1, $2)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.Snapshot, 0, limit)
	for rows.Next() {
		var v store.Snapshot
		if err := rows.Scan(&v.ID, &v.RunID, &v.Bucket, &v.CreatedAt); err != nil {
			return nil, ""
		}
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *snapshotsStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM snapshots WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- artifacts ------------------------------------------------------------

type artifactsStore struct{ pool *pgxpool.Pool }

func (s *artifactsStore) Create(v store.Artifact) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO artifacts(id, run_id, kind, key, bytes, created_at)
		 VALUES ($1,$2,$3,$4,$5,$6)`,
		string(v.ID), string(v.RunID), v.Kind, v.Key, int64(v.Bytes), v.CreatedAt)
	return mapPgErr(err)
}

func (s *artifactsStore) Get(id quarrycontracts.ID) (store.Artifact, bool) {
	var (
		v     store.Artifact
		bytes int64
	)
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, run_id, kind, key, bytes, created_at FROM artifacts WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.RunID, &v.Kind, &v.Key, &bytes, &v.CreatedAt)
	if err != nil {
		return store.Artifact{}, false
	}
	v.Bytes = uint64(bytes)
	return v, true
}

func (s *artifactsStore) List(limit int, cur string) ([]store.Artifact, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, run_id, kind, key, bytes, created_at FROM artifacts`
	args := []any{}
	if c != nil {
		q += ` WHERE (created_at, id) < ($1, $2)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.Artifact, 0, limit)
	for rows.Next() {
		var (
			v     store.Artifact
			bytes int64
		)
		if err := rows.Scan(&v.ID, &v.RunID, &v.Kind, &v.Key, &bytes, &v.CreatedAt); err != nil {
			return nil, ""
		}
		v.Bytes = uint64(bytes)
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *artifactsStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM artifacts WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- profiles -------------------------------------------------------------

type profilesStore struct{ pool *pgxpool.Pool }

func (s *profilesStore) Create(v store.BrowserProfile) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO profiles(id, name, snapshot_uri, created_at) VALUES ($1,$2,$3,$4)`,
		string(v.ID), v.Name, v.SnapshotURI, v.CreatedAt)
	return mapPgErr(err)
}

func (s *profilesStore) Get(id quarrycontracts.ID) (store.BrowserProfile, bool) {
	var v store.BrowserProfile
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, name, snapshot_uri, created_at FROM profiles WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.Name, &v.SnapshotURI, &v.CreatedAt)
	if err != nil {
		return store.BrowserProfile{}, false
	}
	return v, true
}

func (s *profilesStore) List(limit int, cur string) ([]store.BrowserProfile, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, name, snapshot_uri, created_at FROM profiles`
	args := []any{}
	if c != nil {
		q += ` WHERE (created_at, id) < ($1, $2)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.BrowserProfile, 0, limit)
	for rows.Next() {
		var v store.BrowserProfile
		if err := rows.Scan(&v.ID, &v.Name, &v.SnapshotURI, &v.CreatedAt); err != nil {
			return nil, ""
		}
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *profilesStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM profiles WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- schedules ------------------------------------------------------------

type schedulesStore struct{ pool *pgxpool.Pool }

// scheduleCols is the one column list every schedule SELECT uses. scanSchedule
// reads it back in the same order (WS15.S1, migration 017).
const scheduleCols = `id, org_id, name, config, cron, target_kind, target_ref, timezone, overlap_policy,
	catchup_window_s, pause_on_failure, jitter_s, enabled, created_at, updated_at, created_by, zdr,
	last_run_at, next_run_at, skipped_overlap_count, tag`

// rowScanner is the part of pgx.Row and pgx.Rows that scanSchedule needs.
type rowScanner interface {
	Scan(dest ...any) error
}

func scanSchedule(row rowScanner) (store.Schedule, error) {
	var (
		v      store.Schedule
		config []byte
	)
	if err := row.Scan(&v.ID, &v.OrgID, &v.Name, &config, &v.Cron, &v.TargetKind, &v.TargetRef,
		&v.Timezone, &v.OverlapPolicy, &v.CatchupWindowS, &v.PauseOnFailure, &v.JitterS, &v.Enabled,
		&v.CreatedAt, &v.UpdatedAt, &v.CreatedBy, &v.Zdr,
		&v.LastRunAt, &v.NextRunAt, &v.SkippedOverlapCount, &v.Tag); err != nil {
		return store.Schedule{}, err
	}
	if len(config) > 0 {
		if err := json.Unmarshal(config, &v.Config); err != nil {
			return store.Schedule{}, fmt.Errorf("schedule %s config: %w", v.ID, err)
		}
	}
	return v, nil
}

func (s *schedulesStore) Create(v store.Schedule) error {
	tag, err := store.NormalizeTag(v.Tag)
	if err != nil {
		return err
	}
	v.Tag = tag
	v.ApplyDefaults(time.Now().Unix())
	config, err := json.Marshal(v.Config)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(context.Background(),
		`INSERT INTO schedules(id, org_id, name, config, cron, target_kind, target_ref, timezone, overlap_policy,
		   catchup_window_s, pause_on_failure, jitter_s, enabled, created_at, updated_at, created_by, zdr, tag)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)`,
		string(v.ID), v.OrgID, v.Name, string(config), v.Cron, v.TargetKind, v.TargetRef, v.Timezone,
		v.OverlapPolicy, v.CatchupWindowS, v.PauseOnFailure, v.JitterS, v.Enabled, v.CreatedAt,
		v.UpdatedAt, v.CreatedBy, v.Zdr, v.Tag)
	return mapPgErr(err)
}

// Get is unscoped. Only the orchestrator reconciler and the internal source
// monitor path reach it; the edge goes through GetByOrg.
func (s *schedulesStore) Get(id quarrycontracts.ID) (store.Schedule, bool) {
	v, err := scanSchedule(s.pool.QueryRow(context.Background(),
		`SELECT `+scheduleCols+` FROM schedules WHERE id=$1`, string(id)))
	if err != nil {
		return store.Schedule{}, false
	}
	return v, true
}

func (s *schedulesStore) GetByOrg(id quarrycontracts.ID, orgID string) (store.Schedule, bool) {
	v, err := scanSchedule(s.pool.QueryRow(context.Background(),
		`SELECT `+scheduleCols+` FROM schedules WHERE id=$1 AND org_id=$2`, string(id), orgID))
	if err != nil {
		return store.Schedule{}, false
	}
	return v, true
}

// List is unscoped (the legacy GET /v1/schedules without org_id, and the
// orchestrator's reconcile read).
func (s *schedulesStore) List(limit int, cur string) ([]store.Schedule, string) {
	return s.page(false, "", nil, limit, cur)
}

func (s *schedulesStore) ListByOrg(orgID string, limit int, cur string) ([]store.Schedule, string) {
	return s.page(true, orgID, nil, limit, cur)
}

// ListByOrgTag is ListByOrg restricted to the schedules of orgID whose tag is
// tag. The tag is normalised first (store.NormalizeTag), so the filter and the
// stored value agree. An empty orgID, or a tag that does not normalise, matches
// nothing.
func (s *schedulesStore) ListByOrgTag(orgID string, tag string, limit int, cur string) ([]store.Schedule, string) {
	norm, err := store.NormalizeTag(tag)
	if err != nil || orgID == "" {
		return nil, ""
	}
	return s.page(true, orgID, &norm, limit, cur)
}

func (s *schedulesStore) ListDesired(limit int, cur string) ([]store.Schedule, string) {
	return s.page(false, "", nil, limit, cur)
}

// page is the keyset page over (created_at, id), newest first. When scoped,
// the query carries org_id in the WHERE clause, so no row of another org can
// be returned. A non-nil tag adds tag = $n to the same clause.
func (s *schedulesStore) page(scoped bool, orgID string, tag *string, limit int, cur string) ([]store.Schedule, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	var (
		args  []any
		where []string
	)
	if scoped {
		args = append(args, orgID)
		where = append(where, fmt.Sprintf("org_id = $%d", len(args)))
	}
	if tag != nil {
		args = append(args, *tag)
		where = append(where, fmt.Sprintf("tag = $%d", len(args)))
	}
	if c != nil {
		args = append(args, c.CreatedAt, c.ID)
		where = append(where, fmt.Sprintf("(created_at, id) < ($%d, $%d)", len(args)-1, len(args)))
	}
	q := `SELECT ` + scheduleCols + ` FROM schedules`
	if len(where) > 0 {
		q += ` WHERE ` + strings.Join(where, ` AND `)
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.Schedule, 0, limit)
	for rows.Next() {
		v, err := scanSchedule(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *schedulesStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM schedules WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func (s *schedulesStore) DeleteByOrg(id quarrycontracts.ID, orgID string) error {
	ct, err := s.pool.Exec(context.Background(),
		`DELETE FROM schedules WHERE id=$1 AND org_id=$2`, string(id), orgID)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// UpdateEnabled stamps updated_at. The migration keys on updated_at = 0, so a
// row someone re-enabled is never disabled again by a re-run of the guard.
func (s *schedulesStore) UpdateEnabled(id quarrycontracts.ID, enabled bool) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedules SET enabled=$2, updated_at=$3 WHERE id=$1`,
		string(id), enabled, time.Now().Unix())
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func (s *schedulesStore) UpdateEnabledByOrg(id quarrycontracts.ID, orgID string, enabled bool) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedules SET enabled=$2, updated_at=$3 WHERE id=$1 AND org_id=$4`,
		string(id), enabled, time.Now().Unix(), orgID)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// UpdateTagByOrg sets the tag of a schedule the org owns ("" clears it). A row of
// another org answers ErrNotFound, as a missing one does. updated_at is not moved:
// it stays the create or enable/disable stamp (Schedule.UpdatedAt).
func (s *schedulesStore) UpdateTagByOrg(id quarrycontracts.ID, orgID string, tag string) error {
	norm, err := store.NormalizeTag(tag)
	if err != nil {
		return err
	}
	if orgID == "" {
		return store.ErrNotFound
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedules SET tag=$3 WHERE id=$1 AND org_id=$2`,
		string(id), orgID, norm)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func (s *schedulesStore) UpdateRuntime(id quarrycontracts.ID, lastRunAt, nextRunAt *int64, skippedOverlap int64) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedules SET last_run_at=$2, next_run_at=$3, skipped_overlap_count=$4 WHERE id=$1`,
		string(id), lastRunAt, nextRunAt, skippedOverlap)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- sources --------------------------------------------------------------

// sourcesStore is the pg-backed SourcesStore over `quarry_sources` (migration
// 005). Every query carries an `org_id = $` predicate AND `deleted_at IS NULL`
// so a tenant can never read or mutate another tenant's rows, and soft-deleted
// rows stay invisible. created_at/updated_at are stored as TIMESTAMPTZ; we
// convert to/from the store's unix-millis convention at the SQL boundary.
type sourcesStore struct{ pool *pgxpool.Pool }

// sourceCols is the one column list every source SELECT uses. The Scan calls read
// it back in this order; tag is the last column (E3.5-6, migration 027).
const sourceCols = `source_id, org_id, name, url, kind, status, config,
	(extract(epoch from created_at) * 1000)::bigint,
	(extract(epoch from updated_at) * 1000)::bigint, tag`

func (s *sourcesStore) Create(v store.Source) error {
	tag, err := store.NormalizeTag(v.Tag)
	if err != nil {
		return err
	}
	v.Tag = tag
	config := v.Config
	if config == nil {
		config = map[string]any{}
	}
	configJSON, err := json.Marshal(config)
	if err != nil {
		return fmt.Errorf("marshal config: %w", err)
	}
	status := v.Status
	if status == "" {
		status = "active"
	}
	_, err = s.pool.Exec(context.Background(),
		`INSERT INTO quarry_sources(source_id, org_id, name, url, kind, status, config, created_at, updated_at, tag)
		 VALUES ($1,$2,$3,$4,$5,$6,$7, to_timestamp($8::double precision / 1000.0), to_timestamp($9::double precision / 1000.0), $10)`,
		string(v.ID), v.OrgID, v.Name, v.URL, v.Kind, status, configJSON, v.CreatedAt, v.UpdatedAt, v.Tag)
	return mapPgErr(err)
}

// UpsertByOrgAndURL is the idempotent counterpart to Create: a live row for
// the same (org_id, url) already existing is refreshed (updated_at bumped)
// and returned as-is rather than erroring, so repeat registrations for the
// same website — one per crawled page, or a re-run of the same crawl —
// never duplicate. The `quarry_sources_org_url_uniq` partial unique index
// (migration 011) backs the ON CONFLICT target; `xmax = 0` is the standard
// Postgres trick for telling an upsert's INSERT branch apart from its
// UPDATE branch within a single round trip. A repeat registration keeps the tag
// the row already has: tag is written on insert only.
func (s *sourcesStore) UpsertByOrgAndURL(v store.Source) (store.Source, bool, error) {
	tag, err := store.NormalizeTag(v.Tag)
	if err != nil {
		return store.Source{}, false, err
	}
	v.Tag = tag
	config := v.Config
	if config == nil {
		config = map[string]any{}
	}
	configJSON, err := json.Marshal(config)
	if err != nil {
		return store.Source{}, false, fmt.Errorf("marshal config: %w", err)
	}
	status := v.Status
	if status == "" {
		status = "active"
	}
	var (
		out       store.Source
		outConfig []byte
		inserted  bool
	)
	err = s.pool.QueryRow(context.Background(),
		`INSERT INTO quarry_sources(source_id, org_id, name, url, kind, status, config, created_at, updated_at, tag)
		 VALUES ($1,$2,$3,$4,$5,$6,$7, to_timestamp($8::double precision / 1000.0), to_timestamp($9::double precision / 1000.0), $10)
		 ON CONFLICT (org_id, url) WHERE deleted_at IS NULL
		 DO UPDATE SET updated_at = to_timestamp($9::double precision / 1000.0)
		 RETURNING source_id, org_id, name, url, kind, status, config,
		           (extract(epoch from created_at) * 1000)::bigint,
		           (extract(epoch from updated_at) * 1000)::bigint,
		           tag, (xmax = 0)`,
		string(v.ID), v.OrgID, v.Name, v.URL, v.Kind, status, configJSON, v.CreatedAt, v.UpdatedAt, v.Tag,
	).Scan(&out.ID, &out.OrgID, &out.Name, &out.URL, &out.Kind, &out.Status, &outConfig,
		&out.CreatedAt, &out.UpdatedAt, &out.Tag, &inserted)
	if err != nil {
		return store.Source{}, false, mapPgErr(err)
	}
	_ = json.Unmarshal(outConfig, &out.Config)
	return out, inserted, nil
}

func (s *sourcesStore) GetByOrg(orgID string, id quarrycontracts.ID) (store.Source, bool) {
	var (
		v          store.Source
		configJSON []byte
	)
	err := s.pool.QueryRow(context.Background(),
		`SELECT `+sourceCols+`
		   FROM quarry_sources
		  WHERE source_id = $1 AND org_id = $2 AND deleted_at IS NULL`,
		string(id), orgID,
	).Scan(&v.ID, &v.OrgID, &v.Name, &v.URL, &v.Kind, &v.Status, &configJSON, &v.CreatedAt, &v.UpdatedAt, &v.Tag)
	if err != nil {
		return store.Source{}, false
	}
	_ = json.Unmarshal(configJSON, &v.Config)
	return v, true
}

func (s *sourcesStore) ListByOrg(orgID string, limit int, cur string) ([]store.Source, string) {
	return s.listByOrg(orgID, nil, limit, cur)
}

// ListByOrgTag is ListByOrg restricted to the org's live sources whose tag is
// tag, after store.NormalizeTag. An empty orgID, or a tag that does not normalise,
// matches nothing.
func (s *sourcesStore) ListByOrgTag(orgID, tag string, limit int, cur string) ([]store.Source, string) {
	norm, err := store.NormalizeTag(tag)
	if err != nil || orgID == "" {
		return nil, ""
	}
	return s.listByOrg(orgID, &norm, limit, cur)
}

// listByOrg is the keyset page over (created_at, source_id), newest first, for one
// org. A non-nil tag adds tag = $n to the same org-scoped WHERE clause.
func (s *sourcesStore) listByOrg(orgID string, tag *string, limit int, cur string) ([]store.Source, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT ` + sourceCols + `
	        FROM quarry_sources
	       WHERE org_id = $1 AND deleted_at IS NULL`
	args := []any{orgID}
	if tag != nil {
		args = append(args, *tag)
		q += fmt.Sprintf(` AND tag = $%d`, len(args))
	}
	if c != nil {
		args = append(args, c.CreatedAt, c.ID)
		q += fmt.Sprintf(` AND (extract(epoch from created_at) * 1000, source_id) < ($%d, $%d)`, len(args)-1, len(args))
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, source_id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.Source, 0, limit)
	for rows.Next() {
		var (
			v          store.Source
			configJSON []byte
		)
		if err := rows.Scan(&v.ID, &v.OrgID, &v.Name, &v.URL, &v.Kind, &v.Status, &configJSON, &v.CreatedAt, &v.UpdatedAt, &v.Tag); err != nil {
			return nil, ""
		}
		_ = json.Unmarshal(configJSON, &v.Config)
		out = append(out, v)
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *sourcesStore) SoftDeleteByOrg(orgID string, id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE quarry_sources
		    SET deleted_at = NOW(), status = 'deleted', updated_at = NOW()
		  WHERE source_id = $1 AND org_id = $2 AND deleted_at IS NULL`,
		string(id), orgID)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// UpdateTagByOrg sets the tag of a live source the org owns ("" clears it) and
// bumps updated_at. A cross-tenant or soft-deleted id answers ErrNotFound.
func (s *sourcesStore) UpdateTagByOrg(orgID string, id quarrycontracts.ID, tag string) error {
	norm, err := store.NormalizeTag(tag)
	if err != nil {
		return err
	}
	if orgID == "" {
		return store.ErrNotFound
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE quarry_sources
		    SET tag = $3, updated_at = NOW()
		  WHERE source_id = $1 AND org_id = $2 AND deleted_at IS NULL`,
		string(id), orgID, norm)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// UpdateConfigByOrg replaces the config document of a live source the org owns and
// bumps updated_at (E4.1-6b). Config is JSONB, so no migration backs it: the map is
// marshalled as UpsertByOrgAndURL marshals it. A cross-tenant or soft-deleted id
// answers ErrNotFound, as UpdateTagByOrg does.
func (s *sourcesStore) UpdateConfigByOrg(orgID string, id quarrycontracts.ID, config map[string]any) error {
	if orgID == "" {
		return store.ErrNotFound
	}
	if config == nil {
		config = map[string]any{}
	}
	configJSON, err := json.Marshal(config)
	if err != nil {
		return fmt.Errorf("marshal config: %w", err)
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE quarry_sources
		    SET config = $3, updated_at = NOW()
		  WHERE source_id = $1 AND org_id = $2 AND deleted_at IS NULL`,
		string(id), orgID, configJSON)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- events ---------------------------------------------------------------

type eventLog struct{ pool *pgxpool.Pool }

// eventExecer is what an insert of an event needs: the pool or a transaction.
type eventExecer interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
}

func insertEvent(ctx context.Context, x eventExecer, evt quarrycontracts.Event) error {
	payload, _ := json.Marshal(evt.Payload)
	var runID, jobID *string
	if evt.RunID != nil {
		s := string(*evt.RunID)
		runID = &s
	}
	if evt.JobID != nil {
		s := string(*evt.JobID)
		jobID = &s
	}
	_, err := x.Exec(ctx,
		`INSERT INTO events(event_id, run_id, job_id, type, ts, seq, payload, idempotency_key)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,NULLIF($8,''))`,
		string(evt.EventID), runID, jobID, string(evt.Type), evt.Timestamp,
		int64(evt.Seq), payload, evt.IdempotencyKey)
	return mapPgErr(err)
}

func (l *eventLog) Append(evt quarrycontracts.Event) error {
	return insertEvent(context.Background(), l.pool, evt)
}

// AppendNext takes a transaction-scoped advisory lock on the run, reads the
// run's max seq, inserts at max+1 and commits (D2-016). Concurrent writers of the
// same run queue on the lock, so each sees the previous one's committed row and
// the seqs commit in order. Writers of other runs do not wait: the lock key is
// the run id. A conflict on the event id or the idempotency key is returned as
// store.ErrConflict, as Append returns it, and the transaction rolls back.
func (l *eventLog) AppendNext(evt quarrycontracts.Event) (uint64, error) {
	ctx := context.Background()
	if evt.RunID == nil {
		evt.Seq = 1
		return evt.Seq, l.Append(evt)
	}
	tx, err := l.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback(ctx) }()
	run := string(*evt.RunID)
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, "quarry-control:events:"+run); err != nil {
		return 0, err
	}
	var maxSeq int64
	if err := tx.QueryRow(ctx, `SELECT COALESCE(MAX(seq), 0) FROM events WHERE run_id = $1`, run).Scan(&maxSeq); err != nil {
		return 0, err
	}
	evt.Seq = uint64(maxSeq) + 1
	if err := insertEvent(ctx, tx, evt); err != nil {
		return 0, err
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	return evt.Seq, nil
}

// NextSeq queries `MAX(seq) + 1` for the given run_id. Returns 1 when
// no events exist yet. It is a read, not a reservation: two writers can read the
// same value (D2-016). Writers that number server-side use AppendNext, which
// reads and inserts under one lock; NextSeq stays for readers that only report
// the next number.
func (l *eventLog) NextSeq(runID quarrycontracts.ID) uint64 {
	var n int64
	err := l.pool.QueryRow(context.Background(),
		`SELECT COALESCE(MAX(seq), 0) FROM events WHERE run_id = $1`,
		string(runID),
	).Scan(&n)
	if err != nil {
		return 1
	}
	return uint64(n) + 1
}

func (l *eventLog) ForRun(runID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event {
	return l.forField("run_id", string(runID), afterSeq, limit)
}

func (l *eventLog) ForJob(jobID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event {
	return l.forField("job_id", string(jobID), afterSeq, limit)
}

// FindByIdempotencyKey resolves an edge change-webhook retry to its already-
// persisted event. The WHERE clause must stay in lockstep with the partial
// unique index from migration 013_change_webhook_idempotency.sql — that index
// is what turns a concurrent double-delivery into a benign ErrConflict at
// Append time; this lookup is what makes a retry AFTER the first append
// succeeded return the original instead of attempting a second insert.
func (l *eventLog) FindByIdempotencyKey(key string) (quarrycontracts.Event, bool) {
	if key == "" {
		return quarrycontracts.Event{}, false
	}
	var (
		evt     quarrycontracts.Event
		runID   *string
		jobID   *string
		payload []byte
		seq     int64
	)
	err := l.pool.QueryRow(context.Background(),
		`SELECT event_id, run_id, job_id, type, ts, seq, payload, COALESCE(idempotency_key,'')
		   FROM events
		  WHERE idempotency_key = $1 AND type = 'change_detected'`,
		key,
	).Scan(&evt.EventID, &runID, &jobID, &evt.Type, &evt.Timestamp,
		&seq, &payload, &evt.IdempotencyKey)
	if err != nil {
		return quarrycontracts.Event{}, false
	}
	evt.Seq = uint64(seq)
	if runID != nil {
		id := quarrycontracts.ID(*runID)
		evt.RunID = &id
	}
	if jobID != nil {
		id := quarrycontracts.ID(*jobID)
		evt.JobID = &id
	}
	if len(payload) > 0 {
		_ = json.Unmarshal(payload, &evt.Payload)
	}
	return evt, true
}

// SummaryForJob counts a job's events by type (one grouped query).
func (l *eventLog) SummaryForJob(jobID quarrycontracts.ID) map[quarrycontracts.EventType]store.EventTypeSummary {
	out := map[quarrycontracts.EventType]store.EventTypeSummary{}
	rows, err := l.pool.Query(context.Background(),
		`SELECT type, COUNT(*), MAX(seq), MIN(ts), MAX(ts) FROM events WHERE job_id = $1 GROUP BY type`,
		string(jobID))
	if err != nil {
		return out
	}
	defer rows.Close()
	for rows.Next() {
		var (
			typ    string
			sum    store.EventTypeSummary
			maxSeq int64
		)
		if err := rows.Scan(&typ, &sum.Count, &maxSeq, &sum.First, &sum.Last); err != nil {
			return out
		}
		sum.MaxSeq = uint64(maxSeq)
		out[quarrycontracts.EventType(typ)] = sum
	}
	return out
}

// CountsForJobs counts events by type for each listed job in one grouped query.
func (l *eventLog) CountsForJobs(jobIDs []quarrycontracts.ID) map[quarrycontracts.ID]map[quarrycontracts.EventType]int64 {
	out := map[quarrycontracts.ID]map[quarrycontracts.EventType]int64{}
	if len(jobIDs) == 0 {
		return out
	}
	ids := make([]string, 0, len(jobIDs))
	for _, id := range jobIDs {
		ids = append(ids, string(id))
	}
	rows, err := l.pool.Query(context.Background(),
		`SELECT job_id, type, COUNT(*) FROM events WHERE job_id = ANY($1) GROUP BY job_id, type`, ids)
	if err != nil {
		return out
	}
	defer rows.Close()
	for rows.Next() {
		var (
			jobID string
			typ   string
			n     int64
		)
		if err := rows.Scan(&jobID, &typ, &n); err != nil {
			return out
		}
		id := quarrycontracts.ID(jobID)
		counts := out[id]
		if counts == nil {
			counts = map[quarrycontracts.EventType]int64{}
			out[id] = counts
		}
		counts[quarrycontracts.EventType(typ)] = n
	}
	return out
}

// ForJobTypes is ForJob restricted to the given event types. The type filter is
// an array match on the events_job_id_idx scan, so a page of a long job stays
// bounded by limit.
func (l *eventLog) ForJobTypes(jobID quarrycontracts.ID, types []quarrycontracts.EventType, afterSeq uint64, limit int) []quarrycontracts.Event {
	limit = pageLimit(limit, defaultMaxPage)
	names := make([]string, 0, len(types))
	for _, t := range types {
		names = append(names, string(t))
	}
	rows, err := l.pool.Query(context.Background(),
		`SELECT event_id, run_id, job_id, type, ts, seq, payload, COALESCE(idempotency_key,'')
		   FROM events WHERE job_id = $1 AND type = ANY($2) AND seq > $3 ORDER BY seq ASC LIMIT $4`,
		string(jobID), names, int64(afterSeq), limit)
	if err != nil {
		return nil
	}
	defer rows.Close()
	out := make([]quarrycontracts.Event, 0, limit)
	for rows.Next() {
		var (
			evt     quarrycontracts.Event
			runID   *string
			jobRef  *string
			payload []byte
			seq     int64
		)
		if err := rows.Scan(&evt.EventID, &runID, &jobRef, &evt.Type, &evt.Timestamp,
			&seq, &payload, &evt.IdempotencyKey); err != nil {
			return out
		}
		evt.Seq = uint64(seq)
		if runID != nil {
			id := quarrycontracts.ID(*runID)
			evt.RunID = &id
		}
		if jobRef != nil {
			id := quarrycontracts.ID(*jobRef)
			evt.JobID = &id
		}
		if len(payload) > 0 {
			_ = json.Unmarshal(payload, &evt.Payload)
		}
		out = append(out, evt)
	}
	return out
}

func (l *eventLog) forField(col, val string, afterSeq uint64, limit int) []quarrycontracts.Event {
	limit = pageLimit(limit, defaultMaxPage)
	q := fmt.Sprintf(
		`SELECT event_id, run_id, job_id, type, ts, seq, payload, COALESCE(idempotency_key,'')
		 FROM events WHERE %s=$1 AND seq > $2 ORDER BY seq ASC LIMIT $3`, col)
	rows, err := l.pool.Query(context.Background(), q, val, int64(afterSeq), limit)
	if err != nil {
		return nil
	}
	defer rows.Close()

	out := make([]quarrycontracts.Event, 0, limit)
	for rows.Next() {
		var (
			evt     quarrycontracts.Event
			runID   *string
			jobID   *string
			payload []byte
			seq     int64
		)
		if err := rows.Scan(&evt.EventID, &runID, &jobID, &evt.Type, &evt.Timestamp,
			&seq, &payload, &evt.IdempotencyKey); err != nil {
			return out
		}
		evt.Seq = uint64(seq)
		if runID != nil {
			id := quarrycontracts.ID(*runID)
			evt.RunID = &id
		}
		if jobID != nil {
			id := quarrycontracts.ID(*jobID)
			evt.JobID = &id
		}
		if len(payload) > 0 {
			_ = json.Unmarshal(payload, &evt.Payload)
		}
		out = append(out, evt)
	}
	return out
}

// ---- schedule requests (WS15.S3, migration 026) ----------------------------

// scheduleRequestsStore is the pull-model request table. It needs no state of
// its own beyond the pool, so the DB hands out a fresh value per call and the
// postgresDB struct does not change.
type scheduleRequestsStore struct{ pool *pgxpool.Pool }

// ScheduleRequests implements store.ScheduleRequestsProvider.
func (d *postgresDB) ScheduleRequests() store.ScheduleRequestsStore {
	return &scheduleRequestsStore{pool: d.pool}
}

const scheduleRequestCols = `id, schedule_id, org_id, action, params, status, claimed_by, lease_until,
	attempts, result, error, requested_by, COALESCE(idempotency_key, ''), created_at, finished_at`

func scanScheduleRequest(row interface{ Scan(dest ...any) error }) (store.ScheduleRequest, error) {
	var v store.ScheduleRequest
	var id, scheduleID string
	var params, result []byte
	if err := row.Scan(&id, &scheduleID, &v.OrgID, &v.Action, &params, &v.Status, &v.ClaimedBy,
		&v.LeaseUntil, &v.Attempts, &result, &v.Error, &v.RequestedBy, &v.IdempotencyKey,
		&v.CreatedAt, &v.FinishedAt); err != nil {
		return store.ScheduleRequest{}, mapPgErr(err)
	}
	v.ID = quarrycontracts.ID(id)
	v.ScheduleID = quarrycontracts.ID(scheduleID)
	if len(params) > 0 {
		if err := json.Unmarshal(params, &v.Params); err != nil {
			return store.ScheduleRequest{}, err
		}
		if len(v.Params) == 0 {
			v.Params = nil
		}
	}
	if len(result) > 0 {
		if err := json.Unmarshal(result, &v.Result); err != nil {
			return store.ScheduleRequest{}, err
		}
	}
	return v, nil
}

// Create inserts a pending row. The partial unique index on open rows of one
// schedule and the (org, idempotency key) index both surface as ErrConflict.
func (s *scheduleRequestsStore) Create(v store.ScheduleRequest) error {
	params := v.Params
	if params == nil {
		params = map[string]any{}
	}
	paramsJSON, err := json.Marshal(params)
	if err != nil {
		return err
	}
	status := v.Status
	if status == "" {
		status = store.ScheduleRequestPending
	}
	var idem any
	if v.IdempotencyKey != "" {
		idem = v.IdempotencyKey
	}
	_, err = s.pool.Exec(context.Background(),
		`INSERT INTO schedule_requests(id, schedule_id, org_id, action, params, status, requested_by,
		   idempotency_key, created_at)
		 VALUES ($1,$2,$3,$4,$5::jsonb,$6,$7,$8,$9)`,
		string(v.ID), string(v.ScheduleID), v.OrgID, v.Action, string(paramsJSON), status,
		v.RequestedBy, idem, v.CreatedAt)
	return mapPgErr(err)
}

func (s *scheduleRequestsStore) GetByOrg(id quarrycontracts.ID, orgID string) (store.ScheduleRequest, bool) {
	if orgID == "" {
		return store.ScheduleRequest{}, false
	}
	v, err := scanScheduleRequest(s.pool.QueryRow(context.Background(),
		`SELECT `+scheduleRequestCols+` FROM schedule_requests WHERE id=$1 AND org_id=$2`,
		string(id), orgID))
	if err != nil {
		return store.ScheduleRequest{}, false
	}
	return v, true
}

func (s *scheduleRequestsStore) HasPending(scheduleID quarrycontracts.ID, action string) bool {
	var exists bool
	err := s.pool.QueryRow(context.Background(),
		`SELECT EXISTS(SELECT 1 FROM schedule_requests
		   WHERE schedule_id=$1 AND status IN ('pending','claimed') AND ($2 = '' OR action = $2))`,
		string(scheduleID), action).Scan(&exists)
	if err != nil {
		// Fail closed: an unreadable table must not let a second request in. The
		// unique index refuses it anyway; this keeps the friendly 409.
		return true
	}
	return exists
}

// Claim hands each claimable row to exactly one worker: SKIP LOCKED makes two
// concurrent claims take disjoint rows, and a claimed row whose lease ran out
// (its worker died) is claimable again.
func (s *scheduleRequestsStore) Claim(worker string, limit int, lease time.Duration, now time.Time) ([]store.ScheduleRequest, error) {
	if worker == "" {
		return nil, errors.New("claim: worker required")
	}
	if limit <= 0 {
		limit = 1
	}
	nowMs := now.UnixMilli()
	rows, err := s.pool.Query(context.Background(),
		`UPDATE schedule_requests
		    SET status='claimed', claimed_by=$1, lease_until=$2, attempts=attempts+1
		  WHERE id IN (
		    SELECT id FROM schedule_requests
		     WHERE status='pending' OR (status='claimed' AND lease_until < $3)
		     ORDER BY created_at ASC, id ASC
		     LIMIT $4
		     FOR UPDATE SKIP LOCKED)
		 RETURNING `+scheduleRequestCols,
		worker, now.Add(lease).UnixMilli(), nowMs, limit)
	if err != nil {
		return nil, mapPgErr(err)
	}
	defer rows.Close()
	var out []store.ScheduleRequest
	for rows.Next() {
		v, err := scanScheduleRequest(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, v)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	// RETURNING order is not the subquery order.
	sort.Slice(out, func(i, j int) bool {
		if out[i].CreatedAt != out[j].CreatedAt {
			return out[i].CreatedAt < out[j].CreatedAt
		}
		return out[i].ID < out[j].ID
	})
	return out, nil
}

// heldErr tells a missing row (ErrNotFound) from one the worker no longer holds
// (ErrConflict) after an update matched nothing.
func (s *scheduleRequestsStore) heldErr(id quarrycontracts.ID) error {
	var exists bool
	if err := s.pool.QueryRow(context.Background(),
		`SELECT EXISTS(SELECT 1 FROM schedule_requests WHERE id=$1)`, string(id)).Scan(&exists); err != nil {
		return mapPgErr(err)
	}
	if !exists {
		return store.ErrNotFound
	}
	return store.ErrConflict
}

func (s *scheduleRequestsStore) Complete(id quarrycontracts.ID, worker string, result map[string]any, now time.Time) error {
	var resultJSON any
	if result != nil {
		b, err := json.Marshal(result)
		if err != nil {
			return err
		}
		resultJSON = string(b)
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedule_requests
		    SET status='done', result=$3::jsonb, error='', lease_until=0, finished_at=$4
		  WHERE id=$1 AND status='claimed' AND claimed_by=$2`,
		string(id), worker, resultJSON, now.UnixMilli())
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return s.heldErr(id)
	}
	return nil
}

func (s *scheduleRequestsStore) Fail(id quarrycontracts.ID, worker, status, reason string, now time.Time) error {
	if status != store.ScheduleRequestFailed && status != store.ScheduleRequestRejected {
		return errors.New("fail: status must be failed or rejected")
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedule_requests
		    SET status=$3, error=$4, lease_until=0, finished_at=$5
		  WHERE id=$1 AND status='claimed' AND claimed_by=$2`,
		string(id), worker, status, store.ClipScheduleRequestReason(reason), now.UnixMilli())
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return s.heldErr(id)
	}
	return nil
}

func (s *scheduleRequestsStore) Release(id quarrycontracts.ID, worker, reason string) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedule_requests
		    SET status='pending', claimed_by='', lease_until=0, error=$3
		  WHERE id=$1 AND status='claimed' AND claimed_by=$2`,
		string(id), worker, store.ClipScheduleRequestReason(reason))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return s.heldErr(id)
	}
	return nil
}
