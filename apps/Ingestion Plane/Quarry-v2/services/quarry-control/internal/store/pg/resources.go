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
const jobColumns = `id, org_id, kind, status, policy, params, schedule_id, created_at, run_id, idempotency_key, statistics`

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
	)
	if err := row.Scan(&j.ID, &j.OrgID, &j.Kind, &j.Status, &policy, &params, &sid, &j.CreatedAt, &rid, &idem, &statistics); err != nil {
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
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO jobs(id, org_id, kind, status, policy, params, schedule_id, created_at, run_id, idempotency_key)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
		string(j.ID), j.OrgID, j.Kind, j.Status, policy, params, sid, j.CreatedAt, rid, j.IdempotencyKey)
	return mapPgErr(err)
}

// FindByIdempotencyKey returns the existing job for an Idempotency-Key
// scoped to orgID, if any. Uses the partial unique index for an O(log n)
// lookup; the org_id predicate means a key collision with another
// tenant's job is reported as a miss, never returned across tenants.
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

// Update replaces the mutable fields of a job (status, run_id and, when set,
// statistics).
// Other fields are immutable post-creation: kind, params, policy, and
// schedule_id are set at create-time and shouldn't change. Used by the
// orchestrator's jobs dispatcher.
func (s *jobsStore) Update(j store.Job) error {
	var rid *string
	if j.RunID != nil {
		v := string(*j.RunID)
		rid = &v
	}
	// nil keeps what is stored: only a terminal event ever sets statistics, and a
	// later status-only update must not erase them.
	var statistics []byte
	if len(j.Statistics) > 0 {
		statistics, _ = json.Marshal(j.Statistics)
	}
	tag, err := s.pool.Exec(context.Background(),
		`UPDATE jobs SET status=$2, run_id=$3, statistics=COALESCE($4::jsonb, statistics) WHERE id=$1`,
		string(j.ID), j.Status, rid, statistics)
	if err != nil {
		return mapPgErr(err)
	}
	if tag.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
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

// ClaimAccepted implements store.JobsStore.
func (s *jobsStore) ClaimAccepted(limit int, now time.Time, lease time.Duration, defaultCap int) ([]store.Job, error) {
	if limit <= 0 {
		return nil, nil
	}
	rows, err := s.pool.Query(context.Background(), claimAcceptedSQL,
		now, defaultCap, limit, lease.Seconds())
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
	if err := rows.Err(); err != nil {
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

// ListByKind returns jobs whose kind matches exactly AND whose org_id
// matches orgID, newest-first, using the (org_id, kind, created_at) index
// (migration 010_jobs_org_id.sql) so the WHERE + ORDER BY are index-served.
func (s *jobsStore) ListByKind(orgID, kind string, limit int, cur string) ([]store.Job, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)

	q := `SELECT ` + jobColumns + ` FROM jobs WHERE org_id = $1 AND kind = $2`
	args := []any{orgID, kind}
	if c != nil {
		q += ` AND (created_at, id) < ($3, $4)`
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

func (s *schedulesStore) Create(v store.Schedule) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO schedules(id, org_id, cron, target_kind, target_ref, enabled, created_at, created_by)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
		string(v.ID), v.OrgID, v.Cron, v.TargetKind, v.TargetRef, v.Enabled, v.CreatedAt, v.CreatedBy)
	return mapPgErr(err)
}

func (s *schedulesStore) Get(id quarrycontracts.ID) (store.Schedule, bool) {
	var v store.Schedule
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, org_id, cron, target_kind, target_ref, enabled, created_at, created_by
		 FROM schedules WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.OrgID, &v.Cron, &v.TargetKind, &v.TargetRef, &v.Enabled, &v.CreatedAt, &v.CreatedBy)
	if err != nil {
		return store.Schedule{}, false
	}
	return v, true
}

func (s *schedulesStore) List(limit int, cur string) ([]store.Schedule, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, org_id, cron, target_kind, target_ref, enabled, created_at, created_by FROM schedules`
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
	out := make([]store.Schedule, 0, limit)
	for rows.Next() {
		var v store.Schedule
		if err := rows.Scan(&v.ID, &v.OrgID, &v.Cron, &v.TargetKind, &v.TargetRef, &v.Enabled, &v.CreatedAt, &v.CreatedBy); err != nil {
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

func (s *schedulesStore) UpdateEnabled(id quarrycontracts.ID, enabled bool) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE schedules SET enabled=$2 WHERE id=$1`, string(id), enabled)
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

func (s *sourcesStore) Create(v store.Source) error {
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
		`INSERT INTO quarry_sources(source_id, org_id, name, url, kind, status, config, created_at, updated_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7, to_timestamp($8::double precision / 1000.0), to_timestamp($9::double precision / 1000.0))`,
		string(v.ID), v.OrgID, v.Name, v.URL, v.Kind, status, configJSON, v.CreatedAt, v.UpdatedAt)
	return mapPgErr(err)
}

// UpsertByOrgAndURL is the idempotent counterpart to Create: a live row for
// the same (org_id, url) already existing is refreshed (updated_at bumped)
// and returned as-is rather than erroring, so repeat registrations for the
// same website — one per crawled page, or a re-run of the same crawl —
// never duplicate. The `quarry_sources_org_url_uniq` partial unique index
// (migration 011) backs the ON CONFLICT target; `xmax = 0` is the standard
// Postgres trick for telling an upsert's INSERT branch apart from its
// UPDATE branch within a single round trip.
func (s *sourcesStore) UpsertByOrgAndURL(v store.Source) (store.Source, bool, error) {
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
		`INSERT INTO quarry_sources(source_id, org_id, name, url, kind, status, config, created_at, updated_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7, to_timestamp($8::double precision / 1000.0), to_timestamp($9::double precision / 1000.0))
		 ON CONFLICT (org_id, url) WHERE deleted_at IS NULL
		 DO UPDATE SET updated_at = to_timestamp($9::double precision / 1000.0)
		 RETURNING source_id, org_id, name, url, kind, status, config,
		           (extract(epoch from created_at) * 1000)::bigint,
		           (extract(epoch from updated_at) * 1000)::bigint,
		           (xmax = 0)`,
		string(v.ID), v.OrgID, v.Name, v.URL, v.Kind, status, configJSON, v.CreatedAt, v.UpdatedAt,
	).Scan(&out.ID, &out.OrgID, &out.Name, &out.URL, &out.Kind, &out.Status, &outConfig,
		&out.CreatedAt, &out.UpdatedAt, &inserted)
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
		`SELECT source_id, org_id, name, url, kind, status, config,
		        (extract(epoch from created_at) * 1000)::bigint,
		        (extract(epoch from updated_at) * 1000)::bigint
		   FROM quarry_sources
		  WHERE source_id = $1 AND org_id = $2 AND deleted_at IS NULL`,
		string(id), orgID,
	).Scan(&v.ID, &v.OrgID, &v.Name, &v.URL, &v.Kind, &v.Status, &configJSON, &v.CreatedAt, &v.UpdatedAt)
	if err != nil {
		return store.Source{}, false
	}
	_ = json.Unmarshal(configJSON, &v.Config)
	return v, true
}

func (s *sourcesStore) ListByOrg(orgID string, limit int, cur string) ([]store.Source, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT source_id, org_id, name, url, kind, status, config,
	             (extract(epoch from created_at) * 1000)::bigint,
	             (extract(epoch from updated_at) * 1000)::bigint
	        FROM quarry_sources
	       WHERE org_id = $1 AND deleted_at IS NULL`
	args := []any{orgID}
	if c != nil {
		q += ` AND (extract(epoch from created_at) * 1000, source_id) < ($2, $3)`
		args = append(args, c.CreatedAt, c.ID)
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
		if err := rows.Scan(&v.ID, &v.OrgID, &v.Name, &v.URL, &v.Kind, &v.Status, &configJSON, &v.CreatedAt, &v.UpdatedAt); err != nil {
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

// ---- events ---------------------------------------------------------------

type eventLog struct{ pool *pgxpool.Pool }

func (l *eventLog) Append(evt quarrycontracts.Event) error {
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
	_, err := l.pool.Exec(context.Background(),
		`INSERT INTO events(event_id, run_id, job_id, type, ts, seq, payload, idempotency_key)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,NULLIF($8,''))`,
		string(evt.EventID), runID, jobID, string(evt.Type), evt.Timestamp,
		int64(evt.Seq), payload, evt.IdempotencyKey)
	return mapPgErr(err)
}

// NextSeq queries `MAX(seq) + 1` for the given run_id. Returns 1 when
// no events exist yet. The handler uses this to assign seq server-side
// when callers don't number their own events, so the unique index on
// (run_id, seq) doesn't collide on default-zero sequences from
// activity-driven workflow emits.
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
