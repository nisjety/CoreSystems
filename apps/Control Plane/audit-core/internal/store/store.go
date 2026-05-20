// Package store wraps the Postgres pool with two simple insert helpers
// and a paginated read query for each table. Read queries enforce
// `org_id` filtering — never optional — so the velion-side caller
// cannot accidentally fan a query across tenants.
package store

import (
	"context"
	"encoding/json"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

// Store is the single read/write entry point for audit-core. Concrete
// implementation is the pgx pool; tests can swap in an in-memory mock
// that implements the same interface.
type Store struct {
	pool *pgxpool.Pool
}

func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool}
}

// InsertAudit appends a single audit event. Returns the assigned ID so
// callers (e.g. integration tests) can chain reads against the freshly
// inserted row.
func (s *Store) InsertAudit(ctx context.Context, ev *events.AuditEvent) (int64, error) {
	details, err := marshalJSON(ev.Details)
	if err != nil {
		return 0, err
	}
	var id int64
	err = s.pool.QueryRow(ctx, `
		INSERT INTO audit_events (
			occurred_at, org_id, user_id, actor_role, plane, event, subject,
			resource_id, outcome, details, request_id, ip_address, user_agent
		) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULLIF($12, '')::inet, $13)
		RETURNING id
	`, ev.OccurredAt, ev.OrgID, nilIfEmpty(ev.UserID), nilIfEmpty(ev.ActorRole),
		ev.Plane, ev.Event, nilIfEmpty(ev.Subject), nilIfEmpty(ev.ResourceID),
		ev.Outcome, details, nilIfEmpty(ev.RequestID), ev.IPAddress,
		nilIfEmpty(ev.UserAgent)).Scan(&id)
	return id, err
}

// InsertUsage appends a single usage event.
func (s *Store) InsertUsage(ctx context.Context, ev *events.UsageEvent) (int64, error) {
	metadata, err := marshalJSON(ev.Metadata)
	if err != nil {
		return 0, err
	}
	var id int64
	err = s.pool.QueryRow(ctx, `
		INSERT INTO usage_events (
			occurred_at, org_id, user_id, plane, op, tokens_in, tokens_out,
			bytes_in, bytes_out, cost_cents, request_id, metadata
		) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
		RETURNING id
	`, ev.OccurredAt, ev.OrgID, nilIfEmpty(ev.UserID), ev.Plane, ev.Op,
		ev.TokensIn, ev.TokensOut, ev.BytesIn, ev.BytesOut, ev.CostCents,
		nilIfEmpty(ev.RequestID), metadata).Scan(&id)
	return id, err
}

// AuditFilter constrains the audit-list query. Every field except OrgID
// is optional; OrgID is mandatory and enforced at the SQL layer too.
type AuditFilter struct {
	OrgID  string
	Since  time.Time
	Until  time.Time
	Event  string
	UserID string
	Limit  int
}

type AuditRow struct {
	ID         int64                  `json:"id"`
	IngestedAt time.Time              `json:"ingested_at"`
	OccurredAt time.Time              `json:"occurred_at"`
	OrgID      string                 `json:"org_id"`
	UserID     string                 `json:"user_id,omitempty"`
	ActorRole  string                 `json:"actor_role,omitempty"`
	Plane      string                 `json:"plane"`
	Event      string                 `json:"event"`
	Subject    string                 `json:"subject,omitempty"`
	ResourceID string                 `json:"resource_id,omitempty"`
	Outcome    string                 `json:"outcome"`
	Details    map[string]any         `json:"details,omitempty"`
	RequestID  string                 `json:"request_id,omitempty"`
}

func (s *Store) ListAudit(ctx context.Context, f AuditFilter) ([]AuditRow, error) {
	if f.Limit <= 0 || f.Limit > 500 {
		f.Limit = 100
	}
	since := f.Since
	if since.IsZero() {
		since = time.Now().Add(-7 * 24 * time.Hour)
	}
	until := f.Until
	if until.IsZero() {
		until = time.Now().Add(time.Minute)
	}

	rows, err := s.pool.Query(ctx, `
		SELECT id, ingested_at, occurred_at, org_id,
		       COALESCE(user_id, ''), COALESCE(actor_role, ''), plane, event,
		       COALESCE(subject, ''), COALESCE(resource_id, ''), outcome,
		       details, COALESCE(request_id, '')
		FROM audit_events
		WHERE org_id = $1
		  AND ingested_at >= $2
		  AND ingested_at <= $3
		  AND ($4 = '' OR event = $4)
		  AND ($5 = '' OR user_id = $5)
		ORDER BY ingested_at DESC
		LIMIT $6
	`, f.OrgID, since, until, f.Event, f.UserID, f.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := make([]AuditRow, 0, f.Limit)
	for rows.Next() {
		var r AuditRow
		var detailsBytes []byte
		if err := rows.Scan(&r.ID, &r.IngestedAt, &r.OccurredAt, &r.OrgID,
			&r.UserID, &r.ActorRole, &r.Plane, &r.Event, &r.Subject,
			&r.ResourceID, &r.Outcome, &detailsBytes, &r.RequestID); err != nil {
			return nil, err
		}
		if len(detailsBytes) > 0 {
			_ = json.Unmarshal(detailsBytes, &r.Details)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// UsageFilter scopes a usage-list query.
type UsageFilter struct {
	OrgID string
	Since time.Time
	Until time.Time
	Plane string
	Op    string
	Limit int
}

type UsageRow struct {
	ID         int64                  `json:"id"`
	IngestedAt time.Time              `json:"ingested_at"`
	OccurredAt time.Time              `json:"occurred_at"`
	OrgID      string                 `json:"org_id"`
	UserID     string                 `json:"user_id,omitempty"`
	Plane      string                 `json:"plane"`
	Op         string                 `json:"op"`
	TokensIn   int64                  `json:"tokens_in"`
	TokensOut  int64                  `json:"tokens_out"`
	BytesIn    int64                  `json:"bytes_in"`
	BytesOut   int64                  `json:"bytes_out"`
	CostCents  float64                `json:"cost_cents"`
	RequestID  string                 `json:"request_id,omitempty"`
	Metadata   map[string]any         `json:"metadata,omitempty"`
}

func (s *Store) ListUsage(ctx context.Context, f UsageFilter) ([]UsageRow, error) {
	if f.Limit <= 0 || f.Limit > 1000 {
		f.Limit = 200
	}
	since := f.Since
	if since.IsZero() {
		since = time.Now().Add(-30 * 24 * time.Hour)
	}
	until := f.Until
	if until.IsZero() {
		until = time.Now().Add(time.Minute)
	}

	rows, err := s.pool.Query(ctx, `
		SELECT id, ingested_at, occurred_at, org_id,
		       COALESCE(user_id, ''), plane, op, tokens_in, tokens_out,
		       bytes_in, bytes_out, cost_cents,
		       COALESCE(request_id, ''), metadata
		FROM usage_events
		WHERE org_id = $1
		  AND ingested_at >= $2
		  AND ingested_at <= $3
		  AND ($4 = '' OR plane = $4)
		  AND ($5 = '' OR op = $5)
		ORDER BY ingested_at DESC
		LIMIT $6
	`, f.OrgID, since, until, f.Plane, f.Op, f.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := make([]UsageRow, 0, f.Limit)
	for rows.Next() {
		var r UsageRow
		var metaBytes []byte
		if err := rows.Scan(&r.ID, &r.IngestedAt, &r.OccurredAt, &r.OrgID,
			&r.UserID, &r.Plane, &r.Op, &r.TokensIn, &r.TokensOut, &r.BytesIn,
			&r.BytesOut, &r.CostCents, &r.RequestID, &metaBytes); err != nil {
			return nil, err
		}
		if len(metaBytes) > 0 {
			_ = json.Unmarshal(metaBytes, &r.Metadata)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// UsageSummary is the rolled-up shape the velion `/settings/usage`
// dashboard renders directly. One row per (plane, op) for the given
// org + window, with totals across the window.
type UsageSummary struct {
	Plane     string  `json:"plane"`
	Op        string  `json:"op"`
	Events    int64   `json:"events"`
	TokensIn  int64   `json:"tokens_in"`
	TokensOut int64   `json:"tokens_out"`
	BytesIn   int64   `json:"bytes_in"`
	BytesOut  int64   `json:"bytes_out"`
	CostCents float64 `json:"cost_cents"`
}

func (s *Store) SummariseUsage(ctx context.Context, orgID string, since, until time.Time) ([]UsageSummary, error) {
	if since.IsZero() {
		since = time.Now().Add(-30 * 24 * time.Hour)
	}
	if until.IsZero() {
		until = time.Now().Add(time.Minute)
	}
	rows, err := s.pool.Query(ctx, `
		SELECT plane, op,
		       COUNT(*)            AS events,
		       COALESCE(SUM(tokens_in),  0),
		       COALESCE(SUM(tokens_out), 0),
		       COALESCE(SUM(bytes_in),   0),
		       COALESCE(SUM(bytes_out),  0),
		       COALESCE(SUM(cost_cents), 0)
		FROM usage_events
		WHERE org_id = $1
		  AND ingested_at >= $2
		  AND ingested_at <= $3
		GROUP BY plane, op
		ORDER BY cost_cents DESC NULLS LAST, events DESC
	`, orgID, since, until)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := make([]UsageSummary, 0, 16)
	for rows.Next() {
		var r UsageSummary
		if err := rows.Scan(&r.Plane, &r.Op, &r.Events, &r.TokensIn,
			&r.TokensOut, &r.BytesIn, &r.BytesOut, &r.CostCents); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

func nilIfEmpty(s string) any {
	if s == "" {
		return nil
	}
	return s
}

func marshalJSON(v map[string]any) ([]byte, error) {
	if v == nil {
		return []byte(`{}`), nil
	}
	return json.Marshal(v)
}
