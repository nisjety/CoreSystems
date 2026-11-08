package pg

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ---- webhooks -------------------------------------------------------------

type webhooksStore struct{ pool *pgxpool.Pool }

// webhookColumns is the one column list every webhooks SELECT uses, in the order
// scanWebhook reads it.
const webhookColumns = `id, org_id, url, secret, secret_hint, events, active, description, created_by, created_at`

func scanWebhook(row pgx.Row) (store.Webhook, error) {
	var v store.Webhook
	err := row.Scan(&v.ID, &v.OrgID, &v.URL, &v.Secret, &v.SecretHint, &v.Events, &v.Active, &v.Description, &v.CreatedBy, &v.CreatedAt)
	return v, err
}

func (s *webhooksStore) Create(v store.Webhook) error {
	if v.Events == nil {
		v.Events = []string{}
	}
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO webhooks(`+webhookColumns+`)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
		string(v.ID), v.OrgID, v.URL, v.Secret, v.SecretHint, v.Events, v.Active, v.Description, v.CreatedBy, v.CreatedAt)
	return mapPgErr(err)
}

// CreateCapped serialises the creators of one org behind a transaction-scoped advisory
// lock, so the count, the duplicate check and the insert are one atomic step.
func (s *webhooksStore) CreateCapped(v store.Webhook, maxPerOrg int) error {
	if v.OrgID == "" {
		return fmt.Errorf("pg: CreateCapped requires an org")
	}
	if v.Events == nil {
		v.Events = []string{}
	}
	ctx := context.Background()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtext($1))`, "webhooks:"+v.OrgID); err != nil {
		return err
	}
	var count int
	if err := tx.QueryRow(ctx, `SELECT count(*) FROM webhooks WHERE org_id=$1`, v.OrgID).Scan(&count); err != nil {
		return err
	}
	var dup bool
	if err := tx.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM webhooks WHERE org_id=$1 AND url=$2 AND events=$3)`,
		v.OrgID, v.URL, v.Events).Scan(&dup); err != nil {
		return err
	}
	if dup {
		return store.ErrConflict
	}
	if maxPerOrg > 0 && count >= maxPerOrg {
		return store.ErrQuotaExceeded
	}
	if _, err := tx.Exec(ctx,
		`INSERT INTO webhooks(`+webhookColumns+`)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
		string(v.ID), v.OrgID, v.URL, v.Secret, v.SecretHint, v.Events, v.Active, v.Description, v.CreatedBy, v.CreatedAt); err != nil {
		return mapPgErr(err)
	}
	return mapPgErr(tx.Commit(ctx))
}

func (s *webhooksStore) Get(id quarrycontracts.ID) (store.Webhook, bool) {
	v, err := scanWebhook(s.pool.QueryRow(context.Background(),
		`SELECT `+webhookColumns+` FROM webhooks WHERE id=$1`, string(id)))
	if err != nil {
		return store.Webhook{}, false
	}
	return v, true
}

func (s *webhooksStore) GetByOrg(id quarrycontracts.ID, orgID string) (store.Webhook, bool) {
	if orgID == "" {
		return store.Webhook{}, false
	}
	v, err := scanWebhook(s.pool.QueryRow(context.Background(),
		`SELECT `+webhookColumns+` FROM webhooks WHERE id=$1 AND org_id=$2`, string(id), orgID))
	if err != nil {
		return store.Webhook{}, false
	}
	return v, true
}

// List is the unscoped listing, kept for the ResourceStore contract and tests. No route
// and no fan-out reads it.
func (s *webhooksStore) List(limit int, cur string) ([]store.Webhook, string) {
	return s.list("", false, limit, cur)
}

func (s *webhooksStore) ListByOrg(orgID string, limit int, cur string) ([]store.Webhook, string) {
	if orgID == "" {
		return nil, ""
	}
	return s.list(orgID, true, limit, cur)
}

func (s *webhooksStore) list(orgID string, scoped bool, limit int, cur string) ([]store.Webhook, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT ` + webhookColumns + ` FROM webhooks`
	args := []any{}
	var where []string
	if scoped {
		args = append(args, orgID)
		where = append(where, fmt.Sprintf(`org_id = $%d`, len(args)))
	}
	if c != nil {
		args = append(args, c.CreatedAt, c.ID)
		where = append(where, fmt.Sprintf(`(created_at, id) < ($%d, $%d)`, len(args)-1, len(args)))
	}
	for i, w := range where {
		if i == 0 {
			q += ` WHERE ` + w
		} else {
			q += ` AND ` + w
		}
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.Webhook, 0, limit)
	for rows.Next() {
		v, err := scanWebhook(rows)
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

func (s *webhooksStore) CountByOrg(orgID string) int {
	if orgID == "" {
		return 0
	}
	var n int
	if err := s.pool.QueryRow(context.Background(), `SELECT count(*) FROM webhooks WHERE org_id=$1`, orgID).Scan(&n); err != nil {
		return 0
	}
	return n
}

func (s *webhooksStore) ListActiveByOrg(orgID string, limit int) []store.Webhook {
	if orgID == "" {
		return nil
	}
	limit = pageLimit(limit, defaultMaxPage)
	rows, err := s.pool.Query(context.Background(),
		`SELECT `+webhookColumns+` FROM webhooks WHERE org_id=$1 AND active ORDER BY created_at, id LIMIT $2`, orgID, limit)
	if err != nil {
		return nil
	}
	defer rows.Close()
	var out []store.Webhook
	for rows.Next() {
		v, err := scanWebhook(rows)
		if err != nil {
			return nil
		}
		out = append(out, v)
	}
	return out
}

func (s *webhooksStore) UpdateByOrg(v store.Webhook) error {
	if v.OrgID == "" {
		return store.ErrNotFound
	}
	if v.Events == nil {
		v.Events = []string{}
	}
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE webhooks SET events=$3, active=$4, secret=$5, secret_hint=$6, description=$7
		 WHERE id=$1 AND org_id=$2`,
		string(v.ID), v.OrgID, v.Events, v.Active, v.Secret, v.SecretHint, v.Description)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func (s *webhooksStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM webhooks WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// DeleteByOrg removes the subscription in one statement; its deliveries go with it
// (webhook_deliveries.webhook_id ON DELETE CASCADE).
func (s *webhooksStore) DeleteByOrg(id quarrycontracts.ID, orgID string) error {
	if orgID == "" {
		return store.ErrNotFound
	}
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM webhooks WHERE id=$1 AND org_id=$2`, string(id), orgID)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// ---- webhook deliveries ---------------------------------------------------

type webhookDeliveriesStore struct{ pool *pgxpool.Pool }

// deliveryColumns is the one column list every webhook_deliveries SELECT uses, in the
// order scanDelivery reads it.
const deliveryColumns = `id, webhook_id, org_id, event_id, event_type, dedupe_key, payload, attempt, status, last_error, next_attempt_at, created_at`

func scanDelivery(row pgx.Row) (store.WebhookDelivery, error) {
	var v store.WebhookDelivery
	err := row.Scan(&v.ID, &v.WebhookID, &v.OrgID, &v.EventID, &v.EventType, &v.DedupeKey, &v.Payload, &v.Attempt, &v.Status, &v.LastError, &v.NextAttemptAt, &v.CreatedAt)
	return v, err
}

func (s *webhookDeliveriesStore) Create(v store.WebhookDelivery) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO webhook_deliveries(`+deliveryColumns+`)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)`,
		string(v.ID), string(v.WebhookID), v.OrgID, string(v.EventID), v.EventType, v.DedupeKey, v.Payload,
		v.Attempt, v.Status, v.LastError, v.NextAttemptAt, v.CreatedAt)
	return mapPgErr(err)
}

func (s *webhookDeliveriesStore) Get(id quarrycontracts.ID) (store.WebhookDelivery, bool) {
	v, err := scanDelivery(s.pool.QueryRow(context.Background(),
		`SELECT `+deliveryColumns+` FROM webhook_deliveries WHERE id=$1`, string(id)))
	if err != nil {
		return store.WebhookDelivery{}, false
	}
	return v, true
}

func (s *webhookDeliveriesStore) GetByOrg(id quarrycontracts.ID, orgID string) (store.WebhookDelivery, bool) {
	if orgID == "" {
		return store.WebhookDelivery{}, false
	}
	v, err := scanDelivery(s.pool.QueryRow(context.Background(),
		`SELECT `+deliveryColumns+` FROM webhook_deliveries WHERE id=$1 AND org_id=$2`, string(id), orgID))
	if err != nil {
		return store.WebhookDelivery{}, false
	}
	return v, true
}

func (s *webhookDeliveriesStore) List(limit int, cur string) ([]store.WebhookDelivery, string) {
	return s.list(``, nil, limit, cur)
}

func (s *webhookDeliveriesStore) ListByWebhook(orgID string, webhookID quarrycontracts.ID, status string, limit int, cur string) ([]store.WebhookDelivery, string) {
	if orgID == "" {
		return nil, ""
	}
	where := `org_id = $1 AND webhook_id = $2`
	args := []any{orgID, string(webhookID)}
	if status != "" {
		args = append(args, status)
		where += fmt.Sprintf(` AND status = $%d`, len(args))
	}
	return s.list(where, args, limit, cur)
}

// list pages deliveries newest first. where (with $1.. placeholders for args) narrows it.
func (s *webhookDeliveriesStore) list(where string, args []any, limit int, cur string) ([]store.WebhookDelivery, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT ` + deliveryColumns + ` FROM webhook_deliveries`
	args = append([]any(nil), args...)
	if c != nil {
		args = append(args, c.CreatedAt, c.ID)
		cond := fmt.Sprintf(`(created_at, id) < ($%d, $%d)`, len(args)-1, len(args))
		if where != "" {
			where += ` AND ` + cond
		} else {
			where = cond
		}
	}
	if where != "" {
		q += ` WHERE ` + where
	}
	q += fmt.Sprintf(` ORDER BY created_at DESC, id DESC LIMIT %d`, limit+1)
	rows, err := s.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, ""
	}
	defer rows.Close()
	out := make([]store.WebhookDelivery, 0, limit)
	for rows.Next() {
		v, err := scanDelivery(rows)
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

func (s *webhookDeliveriesStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM webhook_deliveries WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// DeleteTerminalBefore is the delivery retention sweep: terminal rows older than cutoff.
func (s *webhookDeliveriesStore) DeleteTerminalBefore(cutoff int64) (int64, error) {
	ct, err := s.pool.Exec(context.Background(),
		`DELETE FROM webhook_deliveries WHERE status IN ('success','failed','dlq') AND created_at < $1`, cutoff)
	if err != nil {
		return 0, mapPgErr(err)
	}
	return ct.RowsAffected(), nil
}

func (s *webhookDeliveriesStore) Update(v store.WebhookDelivery) error {
	ct, err := s.pool.Exec(context.Background(),
		`UPDATE webhook_deliveries SET attempt=$2, status=$3, last_error=$4, next_attempt_at=$5 WHERE id=$1`,
		string(v.ID), v.Attempt, v.Status, v.LastError, v.NextAttemptAt)
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

func (s *webhookDeliveriesStore) ClaimDue(now int64, limit int) ([]store.WebhookDelivery, error) {
	rows, err := s.pool.Query(context.Background(),
		`UPDATE webhook_deliveries SET status='in_flight'
		 WHERE id IN (
		   SELECT id FROM webhook_deliveries
		   WHERE status='pending' AND next_attempt_at<=$1
		   ORDER BY next_attempt_at ASC LIMIT $2
		   FOR UPDATE SKIP LOCKED)
		 RETURNING `+deliveryColumns,
		now, limit)
	if err != nil {
		return nil, mapPgErr(err)
	}
	defer rows.Close()
	var out []store.WebhookDelivery
	for rows.Next() {
		v, err := scanDelivery(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, v)
	}
	return out, rows.Err()
}

// ---- blocklists -----------------------------------------------------------

type blocklistsStore struct{ pool *pgxpool.Pool }

func (s *blocklistsStore) Create(v store.BlocklistEntry) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO blocklist_entries(id, pattern, is_regex, created_at)
		 VALUES ($1,$2,$3,$4)`,
		string(v.ID), v.Pattern, v.IsRegex, v.CreatedAt)
	return mapPgErr(err)
}

func (s *blocklistsStore) Get(id quarrycontracts.ID) (store.BlocklistEntry, bool) {
	var v store.BlocklistEntry
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, pattern, is_regex, created_at FROM blocklist_entries WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.Pattern, &v.IsRegex, &v.CreatedAt)
	if err != nil {
		return store.BlocklistEntry{}, false
	}
	return v, true
}

func (s *blocklistsStore) List(limit int, cur string) ([]store.BlocklistEntry, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, pattern, is_regex, created_at FROM blocklist_entries`
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
	out := make([]store.BlocklistEntry, 0, limit)
	for rows.Next() {
		var v store.BlocklistEntry
		if err := rows.Scan(&v.ID, &v.Pattern, &v.IsRegex, &v.CreatedAt); err != nil {
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

func (s *blocklistsStore) Delete(id quarrycontracts.ID) error {
	ct, err := s.pool.Exec(context.Background(), `DELETE FROM blocklist_entries WHERE id=$1`, string(id))
	if err != nil {
		return mapPgErr(err)
	}
	if ct.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}
