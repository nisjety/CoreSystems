// Cycle-24 Postgres implementations: the /v1/team/* aggregates, the
// activity feed, the enriched /v1/request-queues read model, and the
// org-scoped quarry_snapshots_v2 table. Same conventions as resources.go —
// unix-millis keyset cursors, org_id in every WHERE clause, honest zeros
// when an org has no rows.
package pg

import (
	"context"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ---- shared list cursors ---------------------------------------------------
//
// Identical encoding to encodeCursor/decodeCursor above ("<millis>|<id>",
// base64 raw URL), surfaced under store-facing names so both the store
// package's in-memory impl and this package paginate identically.

func encodeListCursor(createdAt int64, id string) string {
	return encodeCursor(createdAt, id)
}

// decodeListCursor is the forgiving decoder: a missing OR malformed cursor
// starts from the top instead of erroring, matching ListFilter's documented
// "bad cursor = fresh result window" semantics.
func decodeListCursor(s string) (createdAt int64, id string, ok bool) {
	if s == "" {
		return 0, "", false
	}
	c, err := decodeCursor(s)
	if err != nil || c == nil {
		return 0, "", false
	}
	return c.CreatedAt, c.ID, true
}

// ---- team usage ------------------------------------------------------------

type teamUsageStore struct{ pool *pgxpool.Pool }

func (s *teamUsageStore) CreditUsage(orgID, period string) (store.TeamCreditUsageRow, error) {
	since := periodWindowPG(period)
	var (
		used    *float64
		limit   *float64
		pctZero float64
	)
	err := s.pool.QueryRow(context.Background(),
		`SELECT
		    COALESCE((SELECT SUM(COALESCE((e->>'pages')::numeric, 1)) * $2::numeric
		              FROM events, LATERAL (SELECT payload AS e) x
		             WHERE job_id IS NOT NULL AND ts >= $3
		               AND EXISTS (SELECT 1 FROM jobs j WHERE j.id = events.job_id AND j.org_id = $1)),
		            0)::float8,
		    NULL,
		    0::float8`,
		orgID, creditsPerPage, since,
	).Scan(&used, &limit, &pctZero)
	if err != nil {
		return store.TeamCreditUsageRow{}, err
	}
	// No ceiling is modeled here (Control Plane owns entitlements): limit
	// stays nil → "uncapped" → utilization_percent stays 0.
	return store.TeamCreditUsageRow{CreditsUsed: *used}, nil
}

func (s *teamUsageStore) TokenUsage(orgID, period string) (store.TeamTokenUsageRow, error) {
	since := periodWindowPG(period)
	rows, err := s.pool.Query(context.Background(),
		`SELECT e.payload->'usage'->>'input_tokens', e.payload->'usage'->>'output_tokens'
		   FROM events e
		   JOIN jobs j ON j.id = e.job_id
		  WHERE j.org_id = $1 AND e.ts >= $2`, orgID, since)
	if err != nil {
		return store.TeamTokenUsageRow{}, err
	}
	defer rows.Close()
	var row store.TeamTokenUsageRow
	for rows.Next() {
		var inTok, outTok *string
		if err := rows.Scan(&inTok, &outTok); err != nil {
			return store.TeamTokenUsageRow{}, err
		}
		row.InputTokens += parseUint64(inTok)
		row.OutputTokens += parseUint64(outTok)
	}
	if err := rows.Err(); err != nil {
		return store.TeamTokenUsageRow{}, err
	}
	row.TotalTokens = row.InputTokens + row.OutputTokens
	return row, nil
}

func (s *teamUsageStore) Concurrency(orgID string) (store.TeamConcurrencyRow, error) {
	rows, err := s.pool.Query(context.Background(),
		`SELECT j.params->>'url', COUNT(*) OVER ()
		   FROM jobs j
		  WHERE j.org_id = $1 AND j.status = 'running'
		  ORDER BY created_at DESC`, orgID)
	if err != nil {
		return store.TeamConcurrencyRow{}, err
	}
	defer rows.Close()
	row := store.TeamConcurrencyRow{ByHost: []store.HostConcurrencyRow{}}
	counts := map[string]uint32{}
	var current uint32
	for rows.Next() {
		current++
		var url *string
		if err := rows.Scan(&url, &current); err != nil {
			return store.TeamConcurrencyRow{}, err
		}
		if url != nil {
			if h := hostOfURL(*url); h != "" {
				counts[h]++
			}
		}
	}
	if err := rows.Err(); err != nil {
		return store.TeamConcurrencyRow{}, err
	}
	row.Current = current
	for h, c := range counts {
		row.ByHost = append(row.ByHost, store.HostConcurrencyRow{Host: h, Current: c})
	}
	return row, nil
}

func (s *teamUsageStore) QueueStatus(orgID string) (store.TeamQueueStatusRow, error) {
	// Read-only projection over the Rust runtime's frontier tables — same
	// ownership boundary as ListRequestQueues above.
	rows, err := s.pool.Query(context.Background(),
		`SELECT q.queue_id::text, q.name,
		        COUNT(i.request_id) FILTER (WHERE i.status = 'queued'),
		        COUNT(i.request_id) FILTER (WHERE i.status = 'in_flight')
		   FROM quarry_request_queues q
		   LEFT JOIN quarry_queue_items i
		     ON i.queue_id = q.queue_id AND i.org_id = q.org_id
		  WHERE q.org_id = $1 AND q.deleted_at IS NULL
		  GROUP BY q.queue_id, q.name
		  ORDER BY 3 DESC, q.queue_id::text DESC
		  LIMIT 10`, orgID)
	if err != nil {
		return store.TeamQueueStatusRow{}, err
	}
	defer rows.Close()
	row := store.TeamQueueStatusRow{ByQueue: []store.QueueStatusEntryRow{}}
	for rows.Next() {
		var e store.QueueStatusEntryRow
		if err := rows.Scan(&e.QueueID, &e.Name, &e.Queued, &e.InFlight); err != nil {
			return store.TeamQueueStatusRow{}, err
		}
		row.QueuedTotal += e.Queued
		row.InFlightTotal += e.InFlight
		row.ByQueue = append(row.ByQueue, e)
	}
	if err := rows.Err(); err != nil {
		return store.TeamQueueStatusRow{}, err
	}
	return row, nil
}

func (s *teamUsageStore) Activity(orgID string, f store.ListFilter) ([]store.ActivityEntry, string) {
	limit := f.Limit
	if limit <= 0 {
		limit = 50
	}
	orderDir := "DESC"
	keyCmp := "<"
	if !f.SortDescending {
		orderDir = "ASC"
		keyCmp = ">"
	}

	q := `SELECT e.event_id, e.type, e.run_id, EXTRACT(EPOCH FROM e.ts) * 1000,
	             COALESCE(e.payload->>'url', '') , COALESCE(e.payload->>'query', '')
	        FROM events e
	        JOIN jobs j ON j.id = e.job_id
	       WHERE j.org_id = $1`
	args := []any{orgID}
	n := 2
	if f.Status != "" {
		q += fmt.Sprintf(` AND e.type = $%d`, n)
		args = append(args, f.Status)
		n++
	}
	if f.CreatedBefore != nil {
		q += fmt.Sprintf(` AND EXTRACT(EPOCH FROM e.ts) * 1000 <= $%d`, n)
		args = append(args, f.CreatedBefore.UnixMilli())
		n++
	}
	if f.CreatedAfter != nil {
		q += fmt.Sprintf(` AND EXTRACT(EPOCH FROM e.ts) * 1000 >= $%d`, n)
		args = append(args, f.CreatedAfter.UnixMilli())
		n++
	}
	if curTS, curID, ok := decodeListCursor(f.Cursor); ok {
		q += fmt.Sprintf(` AND (EXTRACT(EPOCH FROM e.ts) * 1000 %s $%d OR (EXTRACT(EPOCH FROM e.ts) * 1000 = $%d AND e.event_id %s $%d))`, keyCmp, n, n, keyCmp, n+1)
		args = append(args, curTS)
		args = append(args, curID)
		n += 2
	}
	q += ` ORDER BY EXTRACT(EPOCH FROM e.ts) * 1000 ` + orderDir + `, e.event_id ` + orderDir
	q += fmt.Sprintf(` LIMIT $%d`, n)
	args = append(args, limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()

	out := make([]store.ActivityEntry, 0, limit)
	for rows.Next() {
		var (
			e        store.ActivityEntry
			runID    *string
			url      string
			query    string
			tsMillis float64
		)
		if err := rows.Scan(&e.EventID, &e.EventType, &runID, &tsMillis, &url, &query); err != nil {
			return nil, ""
		}
		e.Ts = millisToTime(tsMillis)
		e.Summary = activitySummaryFor(string(e.EventType), url, query)
		if runID != nil && strings.HasPrefix(*runID, string(quarrycontracts.KindRun)) {
			id := quarrycontracts.ID(*runID)
			e.RunID = &id
		}
		out = append(out, e)
	}
	if err := rows.Err(); err != nil {
		return nil, ""
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeListCursor(last.Ts.UnixMilli(), string(last.EventID))
		out = out[:limit]
	}
	return out, next
}

// activitySummaryFor mirrors the in-memory activitySummary without needing
// a full event struct — the SQL above already projected just the fields
// the summaries use.
func activitySummaryFor(eventType, url, query string) string {
	switch eventType {
	case "page_fetched":
		if url != "" {
			return "fetched " + url
		}
		return "page fetched"
	case "page_failed":
		if url != "" {
			return "failed to fetch " + url
		}
		return "page failed"
	case "page_blocked":
		if url != "" {
			return "blocked while fetching " + url
		}
		return "page blocked"
	case "page_queued":
		if url != "" {
			return "queued " + url
		}
		return "page queued"
	case "search_issued":
		if query != "" {
			return "search issued: " + query
		}
		return "search issued"
	default:
		return eventType
	}
}

// ---- request queues (enriched read model) ----------------------------------

// ListRequestQueueSummaries upgrades ListRequestQueues to the full
// RequestQueueSummary wire shape: kind/status/created_at-as-RFC3339 inputs
// plus the four-counter stats block the Rust side requires.
func (d *postgresDB) ListRequestQueueSummaries(orgID string, f store.ListFilter) ([]store.RequestQueueSummaryV2, string, error) {
	limit := f.Limit
	if limit <= 0 {
		limit = 25
	}
	orderDir := "DESC"
	keyCmp := "<"
	if !f.SortDescending {
		orderDir = "ASC"
		keyCmp = ">"
	}

	q := `SELECT q.queue_id::text, q.name, q.kind,
	          CASE WHEN q.deleted_at IS NOT NULL THEN 'deleted' ELSE 'active' END,
	          (EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint,
	          COUNT(i.request_id) FILTER (WHERE i.status = 'queued'),
	          COUNT(i.request_id) FILTER (WHERE i.status = 'in_flight'),
	          COUNT(i.request_id) FILTER (WHERE i.status = 'acked'),
	          COUNT(i.request_id) FILTER (WHERE i.status = 'failed')
	     FROM quarry_request_queues q
	     LEFT JOIN quarry_queue_items i
	       ON i.queue_id = q.queue_id AND i.org_id = q.org_id
	    WHERE q.org_id = $1 AND q.deleted_at IS NULL`
	args := []any{orgID}
	n := 2
	if f.Status != "" {
		q += fmt.Sprintf(` AND (%s)`, queueStatusPredicate(f.Status))
		args = append(args, f.Status)
		n++
	}
	if f.CreatedBefore != nil {
		q += fmt.Sprintf(` AND (EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint <= $%d`, n)
		args = append(args, f.CreatedBefore.UnixMilli())
		n++
	}
	if f.CreatedAfter != nil {
		q += fmt.Sprintf(` AND (EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint >= $%d`, n)
		args = append(args, f.CreatedAfter.UnixMilli())
		n++
	}
	if curTS, curID, ok := decodeListCursor(f.Cursor); ok {
		q += fmt.Sprintf(` AND ((EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint %s $%d OR ((EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint = $%d AND q.queue_id::text %s $%d))`, keyCmp, n, n, keyCmp, n+1)
		args = append(args, curTS)
		args = append(args, curID)
		n += 2
	}
	q += ` GROUP BY q.queue_id, q.name, q.kind, q.deleted_at, q.created_at`
	q += ` ORDER BY (EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint ` + orderDir + `, q.queue_id::text ` + orderDir
	q += fmt.Sprintf(` LIMIT $%d`, n)
	args = append(args, limit+1)

	rows, err := d.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, "", err
	}
	defer rows.Close()
	out := make([]store.RequestQueueSummaryV2, 0, limit)
	for rows.Next() {
		var v store.RequestQueueSummaryV2
		if err := rows.Scan(&v.QueueID, &v.Name, &v.Kind, &v.Status, &v.CreatedAt,
			&v.Stats.Queued, &v.Stats.InFlight, &v.Stats.Acked, &v.Stats.Failed); err != nil {
			return nil, "", err
		}
		out = append(out, v)
	}
	if err := rows.Err(); err != nil {
		return nil, "", err
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeListCursor(last.CreatedAt, last.QueueID)
		out = out[:limit]
	}
	return out, next, nil
}

// queueStatusPredicate maps a wire status token onto SQL over the queue's
// soft-delete state. Unknown tokens match nothing rather than everything —
// filters fail closed.
func queueStatusPredicate(status string) string {
	switch status {
	case "active":
		return "q.deleted_at IS NULL"
	case "draining", "deleted":
		return "q.deleted_at IS NOT NULL"
	default:
		return "FALSE"
	}
}

// ---- snapshots v2 ----------------------------------------------------------

type snapshotsV2Store struct{ pool *pgxpool.Pool }

func (s *snapshotsV2Store) Create(v store.SnapshotV2) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO quarry_snapshots_v2
		       (snapshot_id, org_id, source_id, url, fingerprint, prev_fingerprint,
		        change_status, captured_at, artifact_id)
		 VALUES ($1,$2,NULLIF($3,''),$4,$5,NULLIF($6,''),$7,
		         to_timestamp($8::double precision / 1000.0),NULLIF($9,''))`,
		string(v.ID), v.OrgID, optID(v.SourceID), v.URL, v.Fingerprint,
		optStr(v.PrevFingerprint), v.ChangeStatus, v.CreatedAt, optID(v.ArtifactID))
	return mapPgErr(err)
}

func (s *snapshotsV2Store) GetByOrg(orgID string, id quarrycontracts.ID) (store.SnapshotV2, bool) {
	v, err := scanSnapshotV2Row(s.pool.QueryRow(context.Background(),
		snapshotSelect()+` WHERE snapshot_id = $1 AND org_id = $2`, string(id), orgID))
	if err != nil {
		return store.SnapshotV2{}, false
	}
	return v, true
}

func (s *snapshotsV2Store) ListByOrg(orgID string, f store.ListFilter) ([]store.SnapshotV2, string) {
	limit := f.Limit
	if limit <= 0 {
		limit = 50
	}
	orderDir := "DESC"
	keyCmp := "<"
	if !f.SortDescending {
		orderDir = "ASC"
		keyCmp = ">"
	}

	q := snapshotSelect() + ` WHERE org_id = $1`
	args := []any{orgID}
	n := 2
	if f.Status != "" {
		q += fmt.Sprintf(` AND change_status = $%d`, n)
		args = append(args, f.Status)
		n++
	}
	if f.CreatedBefore != nil {
		q += fmt.Sprintf(` AND captured_at <= to_timestamp($%d::double precision / 1000.0)`, n)
		args = append(args, f.CreatedBefore.UnixMilli())
		n++
	}
	if f.CreatedAfter != nil {
		q += fmt.Sprintf(` AND captured_at >= to_timestamp($%d::double precision / 1000.0)`, n)
		args = append(args, f.CreatedAfter.UnixMilli())
		n++
	}
	if curTS, curID, ok := decodeListCursor(f.Cursor); ok {
		q += fmt.Sprintf(` AND ((EXTRACT(EPOCH FROM captured_at) * 1000)::bigint %s $%d OR ((EXTRACT(EPOCH FROM captured_at) * 1000)::bigint = $%d AND snapshot_id %s $%d))`, keyCmp, n, n, keyCmp, n+1)
		args = append(args, curTS)
		args = append(args, curID)
		n += 2
	}
	q += ` ORDER BY captured_at ` + orderDir + `, snapshot_id ` + orderDir
	q += fmt.Sprintf(` LIMIT $%d`, n)
	args = append(args, limit+1)

	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.SnapshotV2, 0, limit)
	for rows.Next() {
		v, err := scanSnapshotV2Row(rows)
		if err != nil {
			return nil, ""
		}
		out = append(out, v)
	}
	if err := rows.Err(); err != nil {
		return nil, ""
	}
	next := ""
	if len(out) > limit {
		last := out[limit-1]
		next = encodeListCursor(last.CreatedAt, string(last.ID))
		out = out[:limit]
	}
	return out, next
}

func (s *snapshotsV2Store) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM quarry_snapshots_v2 WHERE snapshot_id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func snapshotSelect() string {
	return `SELECT snapshot_id, org_id, COALESCE(source_id,''), url, fingerprint,
	               COALESCE(prev_fingerprint,''), change_status,
	               (EXTRACT(EPOCH FROM captured_at) * 1000)::bigint,
	               COALESCE(artifact_id,'')
	          FROM quarry_snapshots_v2`
}

// scanSnapshotV2Row scans one snapshot row (works for both QueryRow and
// Rows since pgx shares the Scan signature).
func scanSnapshotV2Row(row interface {
	Scan(dest ...any) error
}) (store.SnapshotV2, error) {
	var (
		v            store.SnapshotV2
		sourceID     string
		prevFP       string
		artifactID   string
		tsMillis     float64
	)
	if err := row.Scan(&v.ID, &v.OrgID, &sourceID, &v.URL, &v.Fingerprint,
		&prevFP, &v.ChangeStatus, &tsMillis, &artifactID); err != nil {
		return store.SnapshotV2{}, err
	}
	v.CreatedAt = int64(tsMillis)
	if sourceID != "" {
		id := quarrycontracts.ID(sourceID)
		v.SourceID = &id
	}
	if prevFP != "" {
		v.PrevFingerprint = &prevFP
	}
	if artifactID != "" {
		id := quarrycontracts.ID(artifactID)
		v.ArtifactID = &id
	}
	return v, nil
}

// ---- small helpers ---------------------------------------------------------

// creditsPerPage is the credit price of one fetched page, mirroring
// quarry_core::credits::CREDIT_COST_PER_PAGE — the same constant the Rust
// side prices /v1/crawl budget checks with.
const creditsPerPage = 1.0

// periodWindowPG resolves the period token to an inclusive start bound as
// a timestamptz-ready time. Mirrors the in-memory periodWindow exactly —
// same tokens, same defaults, same "unknown degrades to 7d" contract.
func periodWindowPG(period string) time.Time {
	switch strings.TrimSpace(period) {
	case "today":
		now := time.Now()
		y, m, d := now.Date()
		return time.Date(y, m, d, 0, 0, 0, 0, now.Location())
	case "30d":
		return time.Now().AddDate(0, 0, -30)
	case "7d":
		fallthrough
	default:
		if day, err := time.ParseInLocation("2006-01-02", strings.TrimSpace(period), time.UTC); err == nil {
			return day
		}
		return time.Now().AddDate(0, 0, -7)
	}
}

func parseUint64(s *string) uint64 {
	if s == nil {
		return 0
	}
	n, _ := strconv.ParseUint(strings.TrimSpace(*s), 10, 64)
	return n
}

func hostOfURL(raw string) string {
	u := strings.TrimPrefix(strings.TrimPrefix(raw, "https://"), "http://")
	if i := strings.IndexAny(u, "/?#"); i >= 0 {
		u = u[:i]
	}
	return strings.ToLower(u)
}

func millisToTime(ms float64) time.Time {
	return time.UnixMilli(int64(ms)).UTC()
}

func optID(id *quarrycontracts.ID) any {
	if id == nil {
		return ""
	}
	return string(*id)
}

func optStr(s *string) any {
	if s == nil {
		return ""
	}
	return *s
}
