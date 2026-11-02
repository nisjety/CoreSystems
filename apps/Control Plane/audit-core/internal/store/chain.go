package store

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/metrics"
)

const (
	chainModeInsert  = "insert"
	chainModeCatchup = "catchup"

	chainStateOK     = "ok"
	chainStateBroken = "broken"

	// catchUpBatchSize bounds one catch-up transaction, and with it how long
	// the org's head lock is held against live inserts.
	catchUpBatchSize = 500

	// detailsGuardMaxBytes is the observe-only size cap on audit details. The
	// documented content-free shape is a handful of short keys; anything near
	// this size is content riding in details.
	detailsGuardMaxBytes = 8 << 10
)

// detailsGuardDenyKeys are detail keys that name content or secrets. The
// guard only counts them (audit_core_details_guard_total); it never rejects.
var detailsGuardDenyKeys = map[string]struct{}{
	"content": {}, "prompt": {}, "completion": {}, "message": {}, "body": {}, "text": {},
	"password": {}, "secret": {}, "token": {}, "authorization": {}, "api_key": {},
}

// chainRowColumns is the one column list a row hash is computed from, read
// back from the stored row (never taken from the incoming event) so Postgres
// normalisation of jsonb, inet and timestamptz cannot make a later
// recomputation disagree. The order matches scanChainRow.
const chainRowColumns = `id, org_id, occurred_at, ingested_at,
	COALESCE(user_id, ''), COALESCE(actor_role, ''), plane, event,
	COALESCE(subject, ''), COALESCE(resource_id, ''), outcome,
	COALESCE(event_id, ''), COALESCE(request_id, ''), COALESCE(host(ip_address), ''),
	COALESCE(user_agent, ''), COALESCE(source_producer, ''), COALESCE(source_bus, ''),
	payload_hash, details::text`

type rowScanner interface {
	Scan(dest ...any) error
}

func scanChainRow(row rowScanner) (int64, chain.Row, error) {
	var id int64
	var r chain.Row
	err := row.Scan(&id, &r.OrgID, &r.OccurredAt, &r.IngestedAt,
		&r.UserID, &r.ActorRole, &r.Plane, &r.Event,
		&r.Subject, &r.ResourceID, &r.Outcome,
		&r.EventID, &r.RequestID, &r.Host,
		&r.UserAgent, &r.SourceProducer, &r.SourceBus,
		&r.PayloadHash, &r.Details)
	return id, r, err
}

// chainHead is one org's locked head row.
type chainHead struct {
	seq         int64
	hash        []byte
	state       string
	activatedAt time.Time
}

// lockChainHead creates the org's head on first use and returns it locked
// FOR UPDATE until the transaction ends. This lock is what serialises an org's
// appends; the unique (org_id, chain_seq) index is the backstop behind it.
func lockChainHead(ctx context.Context, tx pgx.Tx, orgID string) (chainHead, error) {
	genesis, err := chain.GenesisHash(orgID)
	if err != nil {
		return chainHead{}, err
	}
	start := time.Now()
	defer func() { metrics.ObserveChainHeadLockWait(time.Since(start)) }()

	if _, err := tx.Exec(ctx, `
		INSERT INTO audit_chain_heads (org_id, head_seq, head_hash)
		VALUES ($1, 0, $2)
		ON CONFLICT (org_id) DO NOTHING
	`, orgID, genesis); err != nil {
		return chainHead{}, fmt.Errorf("create chain head: %w", err)
	}
	var head chainHead
	if err := tx.QueryRow(ctx, `
		SELECT head_seq, head_hash, state, activated_at
		FROM audit_chain_heads
		WHERE org_id = $1
		FOR UPDATE
	`, orgID).Scan(&head.seq, &head.hash, &head.state, &head.activatedAt); err != nil {
		return chainHead{}, fmt.Errorf("lock chain head: %w", err)
	}
	return head, nil
}

// checkChainTip compares the stored hash of the row at the head's sequence
// with the head's hash. A mismatch means the chain was altered outside this
// code: the org is marked broken, counted once and logged, and the caller
// still appends (an audit sink must not stop recording; the verifier reports
// where the chain broke). A tip row that is absent is not a mismatch: retention
// may have pruned through the head.
func checkChainTip(ctx context.Context, tx pgx.Tx, orgID string, head *chainHead) error {
	if head.state != chainStateOK || head.seq == 0 {
		return nil
	}
	var tip []byte
	err := tx.QueryRow(ctx,
		`SELECT row_hash FROM audit_events WHERE org_id = $1 AND chain_seq = $2`,
		orgID, head.seq).Scan(&tip)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("read chain tip: %w", err)
	}
	if bytes.Equal(tip, head.hash) {
		return nil
	}
	if _, err := tx.Exec(ctx, `
		UPDATE audit_chain_heads SET state = $2, updated_at = NOW()
		WHERE org_id = $1 AND state <> $2
	`, orgID, chainStateBroken); err != nil {
		return fmt.Errorf("mark chain broken: %w", err)
	}
	head.state = chainStateBroken
	metrics.RecordChainBroken()
	log.Error().Str("org_id", orgID).Int64("head_seq", head.seq).
		Msg("audit chain head does not match the stored tip row; chain marked broken")
	return nil
}

// appendChained gives the already inserted row id the next position in the
// org's chain and advances the head, both in the caller's transaction. row is
// the stored row as read back by scanChainRow.
func appendChained(ctx context.Context, tx pgx.Tx, head *chainHead, id int64, row chain.Row, mode string) error {
	row.ChainSeq = head.seq + 1
	hash, err := chain.RowHash(head.hash, row)
	if err != nil {
		return fmt.Errorf("hash audit row: %w", err)
	}
	batch := &pgx.Batch{}
	batch.Queue(`
		UPDATE audit_events
		SET chain_seq = $2, prev_hash = $3, row_hash = $4, chain_version = $5, chain_mode = $6
		WHERE id = $1
	`, id, row.ChainSeq, head.hash, hash, int16(chain.Version), mode)
	batch.Queue(`
		UPDATE audit_chain_heads SET head_seq = $2, head_hash = $3, updated_at = NOW()
		WHERE org_id = $1
	`, row.OrgID, row.ChainSeq, hash)

	results := tx.SendBatch(ctx, batch)
	for step := 0; step < 2; step++ {
		tag, err := results.Exec()
		if err == nil && tag.RowsAffected() != 1 {
			err = fmt.Errorf("affected %d rows, want 1", tag.RowsAffected())
		}
		if err != nil {
			_ = results.Close()
			return fmt.Errorf("chain append step %d: %w", step+1, err)
		}
	}
	if err := results.Close(); err != nil {
		return fmt.Errorf("chain append: %w", err)
	}
	head.seq, head.hash = row.ChainSeq, hash
	return nil
}

// insertAuditChained is insertAudit with the chain on: one transaction takes
// the org's head lock, inserts the row, then chains it. A replay (the insert
// conflicts) commits without touching the head and is resolved exactly as on
// the unchained path.
func (s *Store) insertAuditChained(
	ctx context.Context,
	ev *events.AuditEvent,
	args []any,
	details []byte,
	sourceBus, sourceSubject string,
	streamSequence int64,
	payloadHash []byte,
) (int64, bool, error) {
	id, duplicate, err := s.appendNewAudit(ctx, ev.OrgID, args)
	switch {
	case err != nil:
		metrics.RecordChainAppend("error")
		return 0, false, err
	case duplicate:
		metrics.RecordChainAppend("duplicate")
		return s.resolveAuditConflict(ctx, ev, sourceBus, sourceSubject, streamSequence, payloadHash)
	}
	metrics.RecordChainAppend("appended")
	metrics.RecordDetailsGuard(detailsGuardResult(ev.Details, details))
	return id, true, nil
}

func (s *Store) appendNewAudit(ctx context.Context, orgID string, args []any) (id int64, duplicate bool, err error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, false, err
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	head, err := lockChainHead(ctx, tx, orgID)
	if err != nil {
		return 0, false, err
	}
	if err := checkChainTip(ctx, tx, orgID, &head); err != nil {
		return 0, false, err
	}
	id, row, err := scanChainRow(tx.QueryRow(ctx, insertAuditSQL+" RETURNING "+chainRowColumns, args...))
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, true, tx.Commit(ctx)
	}
	if err != nil {
		return 0, false, err
	}
	if err := appendChained(ctx, tx, &head, id, row, chainModeInsert); err != nil {
		return 0, false, err
	}
	return id, false, tx.Commit(ctx)
}

// CatchUp chains rows that were inserted without a chain position after their
// org's chain was activated (the chain switch was off for a while). Rows older
// than the org's activation stay legacy and unchained: there is no backfill.
// It returns how many rows it chained. Orgs are processed one bounded
// transaction at a time so live inserts are never starved of the head lock.
func (s *Store) CatchUp(ctx context.Context) (int64, error) {
	orgRows, err := s.pool.Query(ctx, `SELECT org_id FROM audit_chain_heads ORDER BY org_id`)
	if err != nil {
		return 0, fmt.Errorf("list chain heads: %w", err)
	}
	var orgs []string
	for orgRows.Next() {
		var org string
		if err := orgRows.Scan(&org); err != nil {
			orgRows.Close()
			return 0, err
		}
		orgs = append(orgs, org)
	}
	orgRows.Close()
	if err := orgRows.Err(); err != nil {
		return 0, err
	}

	var total int64
	for _, org := range orgs {
		for {
			n, err := s.catchUpOrg(ctx, org)
			total += n
			if err != nil {
				return total, fmt.Errorf("chain catch-up for org %s: %w", org, err)
			}
			if n < catchUpBatchSize {
				break
			}
		}
	}
	return total, nil
}

func (s *Store) catchUpOrg(ctx context.Context, orgID string) (int64, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	head, err := lockChainHead(ctx, tx, orgID)
	if err != nil {
		return 0, err
	}
	if err := checkChainTip(ctx, tx, orgID, &head); err != nil {
		return 0, err
	}
	pending, err := tx.Query(ctx, `
		SELECT `+chainRowColumns+`
		FROM audit_events
		WHERE org_id = $1 AND chain_seq IS NULL AND ingested_at >= $2
		ORDER BY ingested_at, id
		LIMIT $3
	`, orgID, head.activatedAt, catchUpBatchSize)
	if err != nil {
		return 0, err
	}
	type pendingRow struct {
		id  int64
		row chain.Row
	}
	var batch []pendingRow
	for pending.Next() {
		id, row, err := scanChainRow(pending)
		if err != nil {
			pending.Close()
			return 0, err
		}
		batch = append(batch, pendingRow{id: id, row: row})
	}
	pending.Close()
	if err := pending.Err(); err != nil {
		return 0, err
	}

	for _, p := range batch {
		if err := appendChained(ctx, tx, &head, p.id, p.row, chainModeCatchup); err != nil {
			return 0, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	for range batch {
		metrics.RecordChainAppend("catchup")
	}
	return int64(len(batch)), nil
}

// detailsGuardResult is the D7 observe-mode check: it classifies the details
// of a freshly stored event as ok, oversize or denied_key. It never rejects
// anything; the counter shows whether enforcing it later would drop traffic.
func detailsGuardResult(details map[string]any, encoded []byte) string {
	if hasDeniedKey(details) {
		return "denied_key"
	}
	if len(encoded) > detailsGuardMaxBytes {
		return "oversize"
	}
	return "ok"
}

func hasDeniedKey(v any) bool {
	switch t := v.(type) {
	case map[string]any:
		for k, child := range t {
			if _, denied := detailsGuardDenyKeys[strings.ToLower(k)]; denied || hasDeniedKey(child) {
				return true
			}
		}
	case []any:
		for _, child := range t {
			if hasDeniedKey(child) {
				return true
			}
		}
	}
	return false
}
