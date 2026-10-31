package store

import (
	"context"
	_ "embed"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// auditChainVerificationStateSQL is migration 009. It is embedded here and
// registered in the migrations list in migrate.go.
//
//go:embed migrations/009_audit_chain_verification_state.sql
var auditChainVerificationStateSQL string

var _ verifier.Source = (*Store)(nil)

// ChainOrgs lists the orgs that have a chain head, the ones a verifier pass
// walks.
func (s *Store) ChainOrgs(ctx context.Context) ([]string, error) {
	rows, err := s.pool.Query(ctx, `SELECT org_id FROM audit_chain_heads ORDER BY org_id`)
	if err != nil {
		return nil, fmt.Errorf("list chain heads: %w", err)
	}
	defer rows.Close()
	var orgs []string
	for rows.Next() {
		var org string
		if err := rows.Scan(&org); err != nil {
			return nil, err
		}
		orgs = append(orgs, org)
	}
	return orgs, rows.Err()
}

// VerifyOrg checks one org's chain against the stored rows, the newest signed
// checkpoint and the newest signed attestation (see package verifier for the
// verdicts). Everything is read in one repeatable-read snapshot, so appends
// that land while a long run streams the rows cannot look like a head that
// disagrees with the chain. The rows are streamed, never collected: memory is
// bounded by one row however long the chain is.
func (s *Store) VerifyOrg(ctx context.Context, orgID string, opts verifier.Options) (verifier.Report, error) {
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return verifier.Report{}, fmt.Errorf("begin verify: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // read-only: nothing to commit

	var legacy int64
	if err := tx.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`, orgID,
	).Scan(&legacy); err != nil {
		return verifier.Report{}, fmt.Errorf("count legacy rows: %w", err)
	}

	var head verifier.Head
	err = tx.QueryRow(ctx,
		`SELECT head_seq, head_hash FROM audit_chain_heads WHERE org_id = $1`, orgID,
	).Scan(&head.Seq, &head.Hash)
	if errors.Is(err, pgx.ErrNoRows) {
		return verifier.NoChain(orgID, legacy), nil
	}
	if err != nil {
		return verifier.Report{}, fmt.Errorf("read chain head: %w", err)
	}

	params := verifier.Params{
		OrgID: orgID, Keys: s.keys, Head: &head, Legacy: legacy,
		FromSeq: opts.FromSeq, FromHash: opts.FromHash, MaxRows: opts.MaxRows,
	}

	var cp verifier.Checkpoint
	switch err := tx.QueryRow(ctx, `
		SELECT through_seq, through_hash, pruned_rows_total, key_id, signature, created_at
		FROM audit_chain_checkpoints
		WHERE org_id = $1
		ORDER BY through_seq DESC
		LIMIT 1
	`, orgID).Scan(&cp.Seq, &cp.Hash, &cp.PrunedTotal, &cp.KeyID, &cp.Signature, &cp.CreatedAt); {
	case err == nil:
		params.Checkpoint = &cp
	case !errors.Is(err, pgx.ErrNoRows):
		return verifier.Report{}, fmt.Errorf("read newest checkpoint: %w", err)
	}

	var newest verifier.Attestation
	switch err := tx.QueryRow(ctx, `
		SELECT chain_seq, row_hash, key_id, signature, signed_at
		FROM audit_chain_attestations
		WHERE org_id = $1
		ORDER BY chain_seq DESC
		LIMIT 1
	`, orgID).Scan(&newest.Seq, &newest.Hash, &newest.KeyID, &newest.Signature, &newest.SignedAt); {
	case err == nil:
		params.Newest = &newest
	case !errors.Is(err, pgx.ErrNoRows):
		return verifier.Report{}, fmt.Errorf("read newest attestation: %w", err)
	}

	// Each row arrives with the attestation signed at its sequence (if any), so
	// the attestations are checked against the chain without a second stream.
	// The derived table renames its columns: chainRowColumns is unqualified.
	rows, err := tx.Query(ctx, `
		SELECT chain_seq, prev_hash, row_hash, COALESCE(chain_version, 0), `+chainRowColumns+`,
		       att.att_seq, att.att_hash, att.att_key, att.att_sig, att.att_at
		FROM audit_events
		LEFT JOIN (
		  SELECT chain_seq AS att_seq, row_hash AS att_hash, key_id AS att_key,
		         signature AS att_sig, signed_at AS att_at
		  FROM audit_chain_attestations
		  WHERE org_id = $1 AND chain_seq >= $2
		) att ON att.att_seq = audit_events.chain_seq
		WHERE audit_events.org_id = $1 AND audit_events.chain_seq >= $2
		ORDER BY audit_events.chain_seq
	`, orgID, params.StartSeq())
	if err != nil {
		return verifier.Report{}, fmt.Errorf("read chained rows: %w", err)
	}
	defer rows.Close()

	return verifier.Verify(params, &chainRowSource{rows: rows})
}

// chainRowSource adapts the verification query to verifier.RowSource.
type chainRowSource struct {
	rows pgx.Rows
}

// affixScanner lets scanChainRow read the chain columns that precede the hashed
// ones in the verification query and the attestation columns that follow them.
type affixScanner struct {
	rows           pgx.Rows
	prefix, suffix []any
}

func (a affixScanner) Scan(dest ...any) error {
	all := append(append(append([]any{}, a.prefix...), dest...), a.suffix...)
	return a.rows.Scan(all...)
}

func (c *chainRowSource) Next() (verifier.StoredRow, bool, error) {
	if !c.rows.Next() {
		return verifier.StoredRow{}, false, c.rows.Err()
	}
	var (
		stored  verifier.StoredRow
		seq     int64
		version int16
		attSeq  *int64
		attHash []byte
		attKey  *string
		attSig  []byte
		attAt   *time.Time
	)
	_, row, err := scanChainRow(affixScanner{
		rows:   c.rows,
		prefix: []any{&seq, &stored.PrevHash, &stored.RowHash, &version},
		suffix: []any{&attSeq, &attHash, &attKey, &attSig, &attAt},
	})
	if err != nil {
		return verifier.StoredRow{}, false, err
	}
	row.ChainSeq = seq
	stored.Row = row
	stored.Version = version
	if attSeq != nil {
		a := &verifier.Attestation{Seq: *attSeq, Hash: attHash, Signature: attSig}
		if attKey != nil {
			a.KeyID = *attKey
		}
		if attAt != nil {
			a.SignedAt = *attAt
		}
		stored.Attestation = a
	}
	return stored, true, nil
}

// LoadVerifierState returns what the verifier loop remembers for an org, or the
// zero State when it has not run for it yet.
func (s *Store) LoadVerifierState(ctx context.Context, orgID string) (verifier.State, error) {
	var (
		st          verifier.State
		status      string
		firstBroken *int64
		lastFull    *time.Time
	)
	err := s.pool.QueryRow(ctx, `
		SELECT last_status, last_reason, first_broken_seq, verified_through_seq, verified_through_hash,
		       legacy_unchained_rows, last_run_at, last_full_at, full_cursor_seq, full_cursor_hash
		FROM audit_chain_verifications
		WHERE org_id = $1
	`, orgID).Scan(&status, &st.Reason, &firstBroken, &st.ThroughSeq, &st.ThroughHash,
		&st.LegacyUnchainedRows, &st.LastRunAt, &lastFull, &st.FullCursorSeq, &st.FullCursorHash)
	if errors.Is(err, pgx.ErrNoRows) {
		return verifier.State{}, nil
	}
	if err != nil {
		return verifier.State{}, fmt.Errorf("read verifier state: %w", err)
	}
	st.Status = verifier.Status(status)
	if firstBroken != nil {
		st.FirstBrokenSeq = *firstBroken
	}
	if lastFull != nil {
		st.LastFullAt = *lastFull
	}
	return st, nil
}

// SaveVerifierState records the verifier loop's state for an org.
func (s *Store) SaveVerifierState(ctx context.Context, orgID string, st verifier.State) error {
	var firstBroken, lastFull any
	if st.FirstBrokenSeq > 0 {
		firstBroken = st.FirstBrokenSeq
	}
	if !st.LastFullAt.IsZero() {
		lastFull = st.LastFullAt
	}
	_, err := s.pool.Exec(ctx, `
		INSERT INTO audit_chain_verifications
		  (org_id, last_status, last_reason, first_broken_seq, verified_through_seq, verified_through_hash,
		   legacy_unchained_rows, last_run_at, last_full_at, full_cursor_seq, full_cursor_hash)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
		ON CONFLICT (org_id) DO UPDATE SET
		  last_status = EXCLUDED.last_status,
		  last_reason = EXCLUDED.last_reason,
		  first_broken_seq = EXCLUDED.first_broken_seq,
		  verified_through_seq = EXCLUDED.verified_through_seq,
		  verified_through_hash = EXCLUDED.verified_through_hash,
		  legacy_unchained_rows = EXCLUDED.legacy_unchained_rows,
		  last_run_at = EXCLUDED.last_run_at,
		  last_full_at = EXCLUDED.last_full_at,
		  full_cursor_seq = EXCLUDED.full_cursor_seq,
		  full_cursor_hash = EXCLUDED.full_cursor_hash
	`, orgID, string(st.Status), st.Reason, firstBroken, st.ThroughSeq, st.ThroughHash,
		st.LegacyUnchainedRows, st.LastRunAt, lastFull, st.FullCursorSeq, st.FullCursorHash)
	if err != nil {
		return fmt.Errorf("save verifier state: %w", err)
	}
	return nil
}
