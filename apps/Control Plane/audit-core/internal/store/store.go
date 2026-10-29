// Package store wraps the Postgres pool with two simple insert helpers
// and a paginated read query for each table. Read queries enforce
// `org_id` filtering — never optional — so the verevon-side caller
// cannot accidentally fan a query across tenants.
package store

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

// Store is the single read/write entry point for audit-core. Concrete
// implementation is the pgx pool; tests can swap in an in-memory mock
// that implements the same interface.
type Store struct {
	pool *pgxpool.Pool
}

var (
	ErrAuditEventConflict = errors.New("audit event identity conflicts with an existing payload")
	ErrUsageEventConflict = errors.New("usage event identity conflicts with an existing payload")
)

func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool}
}

// InsertAudit appends a single audit event. Returns the assigned ID so
// callers (e.g. integration tests) can chain reads against the freshly
// inserted row.
func (s *Store) InsertAudit(ctx context.Context, ev *events.AuditEvent) (int64, error) {
	if ev == nil || strings.TrimSpace(ev.Plane) == "" {
		return 0, fmt.Errorf("audit event plane is required for direct source identity")
	}
	plane := strings.TrimSpace(ev.Plane)
	id, _, err := s.insertAudit(ctx, ev, "direct:"+plane, "http:"+plane, 0)
	return id, err
}

func (s *Store) InsertAuditFromSource(ctx context.Context, ev *events.AuditEvent, source string) (bool, error) {
	source = strings.TrimSpace(source)
	if source == "" || len(source) > 160 {
		return false, fmt.Errorf("invalid direct audit source identity")
	}
	if ev == nil || strings.TrimSpace(ev.Plane) == "" {
		return false, fmt.Errorf("audit event plane is required for direct source identity")
	}
	_, inserted, err := s.insertAudit(ctx, ev, source, "http:"+strings.TrimSpace(ev.Plane), 0)
	return inserted, err
}

func (s *Store) InsertAuditFromStream(ctx context.Context, ev *events.AuditEvent, sourceBus, sourceSubject string, streamSequence uint64) (bool, error) {
	sequence, err := validateStreamIdentity(sourceBus, streamSequence)
	if err != nil {
		return false, err
	}
	sourceSubject = strings.TrimSpace(sourceSubject)
	if sourceSubject == "" || len(sourceSubject) > 256 {
		return false, fmt.Errorf("invalid JetStream source subject")
	}
	_, inserted, err := s.insertAudit(ctx, ev, sourceBus, sourceSubject, sequence)
	return inserted, err
}

func (s *Store) insertAudit(ctx context.Context, ev *events.AuditEvent, sourceBus, sourceSubject string, streamSequence int64) (int64, bool, error) {
	if ev == nil || strings.TrimSpace(sourceBus) == "" || strings.TrimSpace(sourceSubject) == "" {
		return 0, false, fmt.Errorf("audit event and logical source identity are required")
	}
	if err := ev.Validate(); err != nil {
		return 0, false, err
	}
	details, err := marshalJSON(ev.Details)
	if err != nil {
		return 0, false, err
	}
	payloadBytes, err := json.Marshal(ev)
	if err != nil {
		return 0, false, err
	}
	payloadHash := sha256.Sum256(payloadBytes)
	var id int64
	err = s.pool.QueryRow(ctx, `
		INSERT INTO audit_events (
			occurred_at, org_id, user_id, actor_role, plane, event, subject,
			resource_id, outcome, details, event_id, request_id, ip_address, user_agent,
			source_bus, source_subject, source_producer, source_stream_sequence, payload_hash
		) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NULLIF($13, '')::inet, $14,
			NULLIF($15, ''), NULLIF($16, ''), NULLIF($17, ''), NULLIF($18, 0), $19)
		ON CONFLICT DO NOTHING
		RETURNING id
	`, ev.OccurredAt, ev.OrgID, nilIfEmpty(ev.UserID), nilIfEmpty(ev.ActorRole),
		ev.Plane, ev.Event, nilIfEmpty(ev.Subject), nilIfEmpty(ev.ResourceID),
		ev.Outcome, details, nilIfEmpty(ev.EventID), nilIfEmpty(ev.RequestID), ev.IPAddress,
		nilIfEmpty(ev.UserAgent), sourceBus, sourceSubject, ev.Producer, streamSequence, payloadHash[:]).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return s.resolveAuditConflict(ctx, ev, sourceBus, sourceSubject, streamSequence, payloadHash[:])
	}
	return id, err == nil, err
}

func (s *Store) resolveAuditConflict(
	ctx context.Context,
	ev *events.AuditEvent,
	sourceBus, sourceSubject string,
	streamSequence int64,
	payloadHash []byte,
) (int64, bool, error) {
	var id int64
	var existingHash []byte
	var err error
	if ev.EventID != "" {
		err = s.pool.QueryRow(ctx, `
			SELECT id, payload_hash FROM audit_events
			WHERE source_bus = $1 AND source_producer = $2 AND event_id = $3
		`, sourceBus, ev.Producer, ev.EventID).Scan(&id, &existingHash)
	}
	if ev.EventID == "" || errors.Is(err, pgx.ErrNoRows) {
		err = s.pool.QueryRow(ctx, `
			SELECT id, payload_hash FROM audit_events
			WHERE source_bus = $1 AND source_stream_sequence = $2
		`, sourceBus, streamSequence).Scan(&id, &existingHash)
	}
	if err != nil || !bytes.Equal(existingHash, payloadHash) {
		return 0, false, ErrAuditEventConflict
	}
	return id, false, nil
}

// InsertUsage appends a single usage event.
func (s *Store) InsertUsage(ctx context.Context, ev *events.UsageEvent) (int64, error) {
	if ev == nil || strings.TrimSpace(ev.Plane) == "" {
		return 0, fmt.Errorf("usage event plane is required for direct source identity")
	}
	plane := strings.TrimSpace(ev.Plane)
	id, _, err := s.insertUsage(ctx, ev, "direct:"+plane, "direct:"+plane, 0)
	return id, err
}

func (s *Store) InsertUsageFromSource(ctx context.Context, ev *events.UsageEvent, source string) (bool, error) {
	source = strings.TrimSpace(source)
	if source == "" || len(source) > 160 {
		return false, fmt.Errorf("invalid direct usage source identity")
	}
	if ev == nil || strings.TrimSpace(ev.Plane) == "" {
		return false, fmt.Errorf("usage event plane is required for direct source identity")
	}
	_, inserted, err := s.insertUsage(ctx, ev, source, "http:"+strings.TrimSpace(ev.Plane), 0)
	return inserted, err
}

func (s *Store) InsertUsageFromStream(ctx context.Context, ev *events.UsageEvent, sourceBus, sourceSubject string, streamSequence uint64) (bool, error) {
	sequence, err := validateStreamIdentity(sourceBus, streamSequence)
	if err != nil {
		return false, err
	}
	sourceSubject = strings.TrimSpace(sourceSubject)
	if sourceSubject == "" || len(sourceSubject) > 256 {
		return false, fmt.Errorf("invalid JetStream source subject")
	}
	_, inserted, err := s.insertUsage(ctx, ev, sourceBus, sourceSubject, sequence)
	return inserted, err
}

func (s *Store) insertUsage(ctx context.Context, ev *events.UsageEvent, sourceBus, sourceSubject string, streamSequence int64) (int64, bool, error) {
	if ev == nil || strings.TrimSpace(ev.EventID) == "" || strings.TrimSpace(sourceBus) == "" || strings.TrimSpace(sourceSubject) == "" {
		return 0, false, fmt.Errorf("usage event and logical source identity are required")
	}
	if err := ev.Validate(); err != nil {
		return 0, false, err
	}
	metadata, err := marshalJSON(ev.Metadata)
	if err != nil {
		return 0, false, err
	}
	payloadBytes, err := json.Marshal(ev)
	if err != nil {
		return 0, false, err
	}
	payloadHash := sha256.Sum256(payloadBytes)
	var id int64
	err = s.pool.QueryRow(ctx, `
		INSERT INTO usage_events (
			event_id, occurred_at, org_id, user_id, plane, op, tokens_in, tokens_out,
			bytes_in, bytes_out, cost_cents, request_id, metadata,
			source_bus, source_subject, source_producer, source_stream_sequence, payload_hash
		) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
			NULLIF($14, ''), NULLIF($15, ''), NULLIF($16, ''), NULLIF($17, 0), $18)
		ON CONFLICT DO NOTHING
		RETURNING id
	`, nilIfEmpty(ev.EventID), ev.OccurredAt, ev.OrgID, nilIfEmpty(ev.UserID), ev.Plane, ev.Op,
		ev.TokensIn, ev.TokensOut, ev.BytesIn, ev.BytesOut, ev.CostCents,
		nilIfEmpty(ev.RequestID), metadata, sourceBus, sourceSubject, ev.Producer, streamSequence, payloadHash[:]).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		var existingHash []byte
		if lookupErr := s.pool.QueryRow(ctx, `
			SELECT id, payload_hash FROM usage_events
			WHERE source_bus = $1 AND source_producer = $2 AND event_id = $3
		`, sourceBus, ev.Producer, ev.EventID).Scan(&id, &existingHash); lookupErr != nil {
			return 0, false, ErrUsageEventConflict
		}
		if !bytes.Equal(existingHash, payloadHash[:]) {
			return 0, false, ErrUsageEventConflict
		}
		return id, false, nil
	}
	return id, err == nil, err
}

func validateStreamIdentity(sourceBus string, streamSequence uint64) (int64, error) {
	if sourceBus == "" || streamSequence == 0 || streamSequence > math.MaxInt64 {
		return 0, fmt.Errorf("invalid JetStream source identity")
	}
	return int64(streamSequence), nil
}

// PurgeResult reports how many rows each append-only table shed during a
// single retention sweep.
type PurgeResult struct {
	AuditDeleted int64
	UsageDeleted int64
}

// Purge enforces data retention by deleting events older than retentionDays
// from both append-only tables. This is the ONLY delete the store performs —
// the tables are otherwise insert-only.
//
// The cutoff is parameterized (`NOW() - ($1 * INTERVAL '1 day')`) so the
// retention window is never interpolated into the SQL string. retentionDays
// must be >= 1; a non-positive value is a programming error and is rejected
// rather than silently purging everything.
func (s *Store) Purge(ctx context.Context, retentionDays int) (PurgeResult, error) {
	if retentionDays < 1 {
		return PurgeResult{}, fmt.Errorf("retentionDays must be >= 1, got %d", retentionDays)
	}

	var res PurgeResult

	auditTag, err := s.pool.Exec(ctx, `
		DELETE FROM audit_events
		WHERE ingested_at < NOW() - ($1 * INTERVAL '1 day')
	`, retentionDays)
	if err != nil {
		return PurgeResult{}, fmt.Errorf("purge audit_events: %w", err)
	}
	res.AuditDeleted = auditTag.RowsAffected()

	usageTag, err := s.pool.Exec(ctx, `
		DELETE FROM usage_events
		WHERE ingested_at < NOW() - ($1 * INTERVAL '1 day')
	`, retentionDays)
	if err != nil {
		return PurgeResult{}, fmt.Errorf("purge usage_events: %w", err)
	}
	res.UsageDeleted = usageTag.RowsAffected()

	return res, nil
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
	ID         int64          `json:"id"`
	IngestedAt time.Time      `json:"ingested_at"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Plane      string         `json:"plane"`
	Event      string         `json:"event"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome"`
	Details    map[string]any `json:"details,omitempty"`
	EventID    string         `json:"event_id,omitempty"`
	RequestID  string         `json:"request_id,omitempty"`
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
		       details, COALESCE(event_id, ''), COALESCE(request_id, '')
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
			&r.ResourceID, &r.Outcome, &detailsBytes, &r.EventID, &r.RequestID); err != nil {
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
	ID         int64          `json:"id"`
	EventID    string         `json:"event_id,omitempty"`
	IngestedAt time.Time      `json:"ingested_at"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	Plane      string         `json:"plane"`
	Op         string         `json:"op"`
	TokensIn   int64          `json:"tokens_in"`
	TokensOut  int64          `json:"tokens_out"`
	BytesIn    int64          `json:"bytes_in"`
	BytesOut   int64          `json:"bytes_out"`
	CostCents  float64        `json:"cost_cents"`
	RequestID  string         `json:"request_id,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
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
		SELECT id, COALESCE(event_id, ''), ingested_at, occurred_at, org_id,
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
		if err := rows.Scan(&r.ID, &r.EventID, &r.IngestedAt, &r.OccurredAt, &r.OrgID,
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

// UsageSummary is the rolled-up shape the verevon `/settings/usage`
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
		       COUNT(*)            AS event_count,
		       COALESCE(SUM(tokens_in),  0),
		       COALESCE(SUM(tokens_out), 0),
		       COALESCE(SUM(bytes_in),   0),
		       COALESCE(SUM(bytes_out),  0),
		       COALESCE(SUM(cost_cents), 0) AS total_cost_cents
		FROM usage_events
		WHERE org_id = $1
		  AND ingested_at >= $2
		  AND ingested_at <= $3
		GROUP BY plane, op
		ORDER BY total_cost_cents DESC, event_count DESC
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
