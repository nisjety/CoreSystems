package pg

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ---- webhooks -------------------------------------------------------------

type webhooksStore struct{ pool *pgxpool.Pool }

func (s *webhooksStore) Create(v store.Webhook) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO webhooks(id, url, secret, events, active, created_at)
		 VALUES ($1,$2,$3,$4,$5,$6)`,
		string(v.ID), v.URL, v.Secret, v.Events, v.Active, v.CreatedAt)
	return mapPgErr(err)
}

func (s *webhooksStore) Get(id quarrycontracts.ID) (store.Webhook, bool) {
	var v store.Webhook
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, url, secret, events, active, created_at
		 FROM webhooks WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.URL, &v.Secret, &v.Events, &v.Active, &v.CreatedAt)
	if err != nil {
		return store.Webhook{}, false
	}
	return v, true
}

func (s *webhooksStore) List(limit int, cur string) ([]store.Webhook, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, url, secret, events, active, created_at FROM webhooks`
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
	out := make([]store.Webhook, 0, limit)
	for rows.Next() {
		var v store.Webhook
		if err := rows.Scan(&v.ID, &v.URL, &v.Secret, &v.Events, &v.Active, &v.CreatedAt); err != nil {
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

// ---- webhook deliveries ---------------------------------------------------

type webhookDeliveriesStore struct{ pool *pgxpool.Pool }

func (s *webhookDeliveriesStore) Create(v store.WebhookDelivery) error {
	_, err := s.pool.Exec(context.Background(),
		`INSERT INTO webhook_deliveries(id, webhook_id, event_id, payload, attempt, status, last_error, next_attempt_at, created_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)`,
		string(v.ID), string(v.WebhookID), string(v.EventID), v.Payload,
		v.Attempt, v.Status, v.LastError, v.NextAttemptAt, v.CreatedAt)
	return mapPgErr(err)
}

func (s *webhookDeliveriesStore) Get(id quarrycontracts.ID) (store.WebhookDelivery, bool) {
	var v store.WebhookDelivery
	err := s.pool.QueryRow(context.Background(),
		`SELECT id, webhook_id, event_id, payload, attempt, status, last_error, next_attempt_at, created_at
		 FROM webhook_deliveries WHERE id=$1`, string(id),
	).Scan(&v.ID, &v.WebhookID, &v.EventID, &v.Payload, &v.Attempt, &v.Status, &v.LastError, &v.NextAttemptAt, &v.CreatedAt)
	if err != nil {
		return store.WebhookDelivery{}, false
	}
	return v, true
}

func (s *webhookDeliveriesStore) List(limit int, cur string) ([]store.WebhookDelivery, string) {
	limit = pageLimit(limit, defaultMaxPage)
	c, _ := decodeCursor(cur)
	q := `SELECT id, webhook_id, event_id, payload, attempt, status, last_error, next_attempt_at, created_at FROM webhook_deliveries`
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
	out := make([]store.WebhookDelivery, 0, limit)
	for rows.Next() {
		var v store.WebhookDelivery
		if err := rows.Scan(&v.ID, &v.WebhookID, &v.EventID, &v.Payload, &v.Attempt, &v.Status, &v.LastError, &v.NextAttemptAt, &v.CreatedAt); err != nil {
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
		 RETURNING id, webhook_id, event_id, payload, attempt, status, last_error, next_attempt_at, created_at`,
		now, limit)
	if err != nil {
		return nil, mapPgErr(err)
	}
	defer rows.Close()
	var out []store.WebhookDelivery
	for rows.Next() {
		var v store.WebhookDelivery
		if err := rows.Scan(&v.ID, &v.WebhookID, &v.EventID, &v.Payload, &v.Attempt, &v.Status, &v.LastError, &v.NextAttemptAt, &v.CreatedAt); err != nil {
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
