package store

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/metrics"
)

// PruneMode says what Purge may do to chained rows.
type PruneMode int

const (
	// PruneOff keeps every chained row: only unchained rows age out. It is the
	// default, so switching the chain on can never lose data.
	PruneOff PruneMode = iota
	// PruneDryRun verifies the prefix it would delete and logs the plan, and
	// deletes and writes nothing.
	PruneDryRun
	// PruneOn deletes the verified expired prefix and signs a checkpoint.
	PruneOn
)

const (
	checkpointReasonRetention = "retention"

	// defaultPruneBatchSize bounds one purge transaction: how many chained rows it
	// verifies and deletes, and so how long it runs and how much it holds.
	defaultPruneBatchSize = 5000
)

// Incident kinds, a closed vocabulary: they are stored and alerted on, and
// must never carry row content.
const (
	incidentChainBroken           = "chain_marked_broken"
	incidentCheckpointKey         = "checkpoint_key_unknown"
	incidentCheckpointSignature   = "checkpoint_signature_invalid"
	incidentSequenceGap           = "sequence_gap"
	incidentPrevHashMismatch      = "prev_hash_mismatch"
	incidentRowHashMismatch       = "row_hash_mismatch"
	incidentUnsupportedVersion    = "unsupported_chain_version"
	incidentUnhashableRow         = "unhashable_row"
	incidentCheckpointHashLinkage = "checkpoint_link_mismatch"
)

// WithChainKeys gives the store the key ring that signs attestations and
// checkpoints and verifies earlier ones.
func WithChainKeys(ring *chain.KeyRing) Option {
	return func(s *Store) { s.keys = ring }
}

// WithChainPrune sets what Purge may do to chained rows (AUDIT_CHAIN_PRUNE_*).
func WithChainPrune(mode PruneMode) Option {
	return func(s *Store) { s.prune = mode }
}

// Attest signs the current head of every healthy org chain that has not been
// attested at its present sequence, and returns how many it wrote. A head that
// is marked broken is never attested: a signature must not vouch for a chain
// the store already doubts. The signature is what an attacker with database
// access cannot forge, so a tail truncated later falls below the newest
// attested sequence.
func (s *Store) Attest(ctx context.Context) (int, error) {
	if !s.keys.CanSign() {
		return 0, chain.ErrNoSigningKey
	}
	rows, err := s.pool.Query(ctx, `
		SELECT h.org_id, h.head_seq, h.head_hash
		FROM audit_chain_heads h
		WHERE h.state = $1
		  AND h.head_seq >= 1
		  AND NOT EXISTS (
		    SELECT 1 FROM audit_chain_attestations a
		    WHERE a.org_id = h.org_id AND a.chain_seq = h.head_seq)
		ORDER BY h.org_id
	`, chainStateOK)
	if err != nil {
		return 0, fmt.Errorf("list unattested chain heads: %w", err)
	}
	type head struct {
		org  string
		seq  int64
		hash []byte
	}
	var heads []head
	for rows.Next() {
		var h head
		if err := rows.Scan(&h.org, &h.seq, &h.hash); err != nil {
			rows.Close()
			return 0, err
		}
		heads = append(heads, h)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, err
	}

	written := 0
	for _, h := range heads {
		signedAt := time.Now().UTC().Truncate(time.Microsecond)
		keyID, sig, err := s.keys.SignAttestation(h.org, h.seq, h.hash, signedAt)
		if err != nil {
			return written, fmt.Errorf("sign attestation for org %s: %w", h.org, err)
		}
		// Another replica may have attested this sequence first; the first
		// signature stands.
		tag, err := s.pool.Exec(ctx, `
			INSERT INTO audit_chain_attestations (org_id, chain_seq, row_hash, key_id, signature, signed_at)
			VALUES ($1, $2, $3, $4, $5, $6)
			ON CONFLICT (org_id, chain_seq) DO NOTHING
		`, h.org, h.seq, h.hash, keyID, sig, signedAt)
		if err != nil {
			return written, fmt.Errorf("store attestation for org %s: %w", h.org, err)
		}
		written += int(tag.RowsAffected())
	}
	return written, nil
}

// pruneBase is where the next purge batch starts: the newest signed checkpoint
// (or genesis) that the rows still present must link to.
type pruneBase struct {
	seq   int64
	hash  []byte
	total int64
}

// pruneOutcome is how one batch ended.
type pruneOutcome int

const (
	pruneProgress pruneOutcome = iota // a batch ran; there may be more
	pruneDone                         // nothing (more) is expired
	pruneBlocked                      // verification failed; an incident was written
	pruneBusy                         // another purger holds this org
)

type pruneBatch struct {
	outcome pruneOutcome
	rows    int64     // rows deleted (or, in a dry run, that would be)
	next    pruneBase // the base after this batch, used to carry a dry run forward
	written bool      // a checkpoint was written
}

// purgeChained is the chain-safe half of Purge. Per org it removes the
// contiguous expired prefix of the chain in bounded batches: each batch first
// verifies the rows it is about to delete against the previous signed
// checkpoint, and only then deletes them and signs a new checkpoint in the same
// transaction. A prefix that does not verify is never deleted: the org is
// blocked and an incident is written instead.
func (s *Store) purgeChained(ctx context.Context, retentionDays int, res *PurgeResult) error {
	if s.prune == PruneOff {
		return nil
	}
	if s.prune == PruneOn && !s.keys.CanSign() {
		return fmt.Errorf("chained purge needs a signing key: %w", chain.ErrNoSigningKey)
	}
	orgRows, err := s.pool.Query(ctx, `SELECT org_id FROM audit_chain_heads ORDER BY org_id`)
	if err != nil {
		return fmt.Errorf("list chain heads: %w", err)
	}
	var orgs []string
	for orgRows.Next() {
		var org string
		if err := orgRows.Scan(&org); err != nil {
			orgRows.Close()
			return err
		}
		orgs = append(orgs, org)
	}
	orgRows.Close()
	if err := orgRows.Err(); err != nil {
		return err
	}

	for _, org := range orgs {
		var carried *pruneBase // a dry run persists nothing, so it carries its own cursor
		for {
			batch, err := s.pruneBatchFor(ctx, org, retentionDays, carried)
			if err != nil {
				return fmt.Errorf("chained purge for org %s: %w", org, err)
			}
			if s.prune == PruneDryRun {
				res.ChainedPlanned += batch.rows
				next := batch.next
				carried = &next
			} else {
				res.AuditDeleted += batch.rows
			}
			if batch.written {
				res.CheckpointsWritten++
			}
			if batch.outcome == pruneBlocked {
				res.OrgsBlocked++
			}
			if batch.outcome != pruneProgress {
				break
			}
		}
	}
	return nil
}

func (s *Store) pruneBatchFor(ctx context.Context, orgID string, retentionDays int, carried *pruneBase) (pruneBatch, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return pruneBatch{}, err
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	// Two purgers (replicas, or a sweep overlapping a slow one) must not
	// both cut the same prefix: the loser skips this org until the next sweep.
	var locked bool
	if err := tx.QueryRow(ctx,
		`SELECT pg_try_advisory_xact_lock(hashtextextended('audit_chain_purge:' || $1, 0))`, orgID,
	).Scan(&locked); err != nil {
		return pruneBatch{}, err
	}
	if !locked {
		return pruneBatch{outcome: pruneBusy}, nil
	}

	var state string
	if err := tx.QueryRow(ctx, `SELECT state FROM audit_chain_heads WHERE org_id = $1`, orgID).Scan(&state); err != nil {
		return pruneBatch{}, err
	}
	if state != chainStateOK {
		return s.blockPurge(ctx, orgID, incidentChainBroken, 0)
	}

	base := carried
	if base == nil {
		loaded, kind, err := s.loadPruneBase(ctx, tx, orgID)
		if err != nil {
			return pruneBatch{}, err
		}
		if kind != "" {
			return s.blockPurge(ctx, orgID, kind, 0)
		}
		base = &loaded
	}

	// The expired prefix ends just before the first row that is still inside
	// the window (ingested_at is not commit-ordered, so the cut is by sequence,
	// never by time); with no such row, everything present has expired.
	var through *int64
	if err := tx.QueryRow(ctx, `
		SELECT COALESCE(MIN(chain_seq) FILTER (WHERE ingested_at >= NOW() - ($3 * INTERVAL '1 day')) - 1, MAX(chain_seq))
		FROM audit_events
		WHERE org_id = $1 AND chain_seq IS NOT NULL AND chain_seq > $2
	`, orgID, base.seq, retentionDays).Scan(&through); err != nil {
		return pruneBatch{}, err
	}
	if through == nil || *through <= base.seq {
		return pruneBatch{outcome: pruneDone, next: *base}, nil
	}
	to := *through
	if to-base.seq > s.pruneBatchSize() {
		to = base.seq + s.pruneBatchSize()
	}

	lastHash, kind, badSeq, err := verifyPrefix(ctx, tx, orgID, *base, to)
	if err != nil {
		return pruneBatch{}, err
	}
	if kind != "" {
		return s.blockPurge(ctx, orgID, kind, badSeq)
	}
	count := to - base.seq
	next := pruneBase{seq: to, hash: lastHash, total: base.total + count}

	if s.prune == PruneDryRun {
		log.Info().Str("org_id", orgID).Int64("from_seq", base.seq+1).Int64("through_seq", to).
			Int64("rows", count).Msg("audit chain purge dry run: prefix verified, nothing deleted")
		return pruneBatch{outcome: pruneProgress, rows: count, next: next}, nil
	}

	if _, err := tx.Exec(ctx, `SELECT set_config('audit.maintenance', 'on', true)`); err != nil {
		return pruneBatch{}, err
	}
	tag, err := tx.Exec(ctx,
		`DELETE FROM audit_events WHERE org_id = $1 AND chain_seq > $2 AND chain_seq <= $3`,
		orgID, base.seq, to)
	if err != nil {
		return pruneBatch{}, err
	}
	if tag.RowsAffected() != count {
		// Verification just read exactly count rows under this lock.
		return pruneBatch{}, fmt.Errorf("deleted %d rows, verified %d", tag.RowsAffected(), count)
	}
	createdAt := time.Now().UTC().Truncate(time.Microsecond)
	keyID, sig, err := s.keys.SignCheckpoint(orgID, to, lastHash, next.total, createdAt)
	if err != nil {
		return pruneBatch{}, err
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO audit_chain_checkpoints
		  (org_id, through_seq, through_hash, pruned_rows_total, reason, key_id, signature, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
	`, orgID, to, lastHash, next.total, checkpointReasonRetention, keyID, sig, createdAt); err != nil {
		return pruneBatch{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return pruneBatch{}, err
	}
	return pruneBatch{outcome: pruneProgress, rows: count, next: next, written: true}, nil
}

func (s *Store) pruneBatchSize() int64 {
	if s.pruneBatchOverride > 0 {
		return int64(s.pruneBatchOverride)
	}
	return defaultPruneBatchSize
}

// blockPurge records why nothing was deleted for this org. It runs on the pool,
// outside the purge transaction, so rolling that transaction back cannot take
// the incident with it.
func (s *Store) blockPurge(ctx context.Context, orgID, kind string, firstBadSeq int64) (pruneBatch, error) {
	var bad any
	if firstBadSeq > 0 {
		bad = firstBadSeq
	}
	if _, err := s.pool.Exec(ctx, `
		INSERT INTO audit_chain_incidents (org_id, kind, first_bad_seq, detail)
		VALUES ($1, $2, $3, $4)
	`, orgID, kind, bad, "purge refused; nothing was deleted"); err != nil {
		return pruneBatch{}, fmt.Errorf("record chain incident: %w", err)
	}
	// Counted once the row is in, so the counter never runs ahead of the
	// incident table it mirrors. Dry-run mode counts too, as it writes the row.
	metrics.RecordChainPurgeBlocked(kind)
	log.Error().Str("org_id", orgID).Str("kind", kind).Int64("first_bad_seq", firstBadSeq).
		Msg("audit chain purge blocked: the prefix does not verify, nothing was deleted")
	return pruneBatch{outcome: pruneBlocked}, nil
}

// loadPruneBase reads the newest checkpoint and verifies its signature. kind is
// non-empty (and the base unusable) when the checkpoint cannot be trusted.
func (s *Store) loadPruneBase(ctx context.Context, tx pgx.Tx, orgID string) (pruneBase, string, error) {
	var (
		base      pruneBase
		keyID     string
		signature []byte
		createdAt time.Time
	)
	err := tx.QueryRow(ctx, `
		SELECT through_seq, through_hash, pruned_rows_total, key_id, signature, created_at
		FROM audit_chain_checkpoints
		WHERE org_id = $1
		ORDER BY through_seq DESC
		LIMIT 1
	`, orgID).Scan(&base.seq, &base.hash, &base.total, &keyID, &signature, &createdAt)
	if errors.Is(err, pgx.ErrNoRows) {
		genesis, gerr := chain.GenesisHash(orgID)
		return pruneBase{hash: genesis}, "", gerr
	}
	if err != nil {
		return pruneBase{}, "", err
	}
	switch verr := s.keys.VerifyCheckpoint(orgID, base.seq, base.hash, base.total, createdAt, keyID, signature); {
	case errors.Is(verr, chain.ErrUnknownKeyID):
		return pruneBase{}, incidentCheckpointKey, nil
	case verr != nil:
		return pruneBase{}, incidentCheckpointSignature, nil
	}
	return base, "", nil
}

// prefixScanner lets scanChainRow read the stored chain columns that precede
// the hashed ones in the verification query.
type prefixScanner struct {
	rows   pgx.Rows
	prefix []any
}

func (p prefixScanner) Scan(dest ...any) error {
	return p.rows.Scan(append(append([]any{}, p.prefix...), dest...)...)
}

// verifyPrefix recomputes the chain over (base.seq, to] and returns the hash of
// the row at to. A non-empty kind names the first defect and badSeq where it
// is; the caller deletes nothing in that case. Rows are streamed, so memory
// stays bounded by one row.
func verifyPrefix(ctx context.Context, tx pgx.Tx, orgID string, base pruneBase, to int64) (last []byte, kind string, badSeq int64, err error) {
	rows, err := tx.Query(ctx, `
		SELECT chain_seq, prev_hash, row_hash, COALESCE(chain_version, 0), `+chainRowColumns+`
		FROM audit_events
		WHERE org_id = $1 AND chain_seq > $2 AND chain_seq <= $3
		ORDER BY chain_seq
	`, orgID, base.seq, to)
	if err != nil {
		return nil, "", 0, err
	}
	defer rows.Close()

	expected, prev := base.seq+1, base.hash
	for rows.Next() {
		var (
			seq     int64
			stored  []byte
			version int16
			link    []byte
		)
		_, row, err := scanChainRow(prefixScanner{rows: rows, prefix: []any{&seq, &link, &stored, &version}})
		if err != nil {
			return nil, "", 0, err
		}
		switch {
		case seq != expected:
			return nil, incidentSequenceGap, expected, nil
		case !bytes.Equal(link, prev):
			if expected == base.seq+1 && base.seq > 0 {
				return nil, incidentCheckpointHashLinkage, seq, nil
			}
			return nil, incidentPrevHashMismatch, seq, nil
		case version != chain.Version:
			return nil, incidentUnsupportedVersion, seq, nil
		}
		row.ChainSeq = seq
		want, herr := chain.RowHash(prev, row)
		if herr != nil {
			return nil, incidentUnhashableRow, seq, nil
		}
		if !bytes.Equal(want, stored) {
			return nil, incidentRowHashMismatch, seq, nil
		}
		prev, expected = stored, expected+1
	}
	if err := rows.Err(); err != nil {
		return nil, "", 0, err
	}
	if expected-1 != to {
		return nil, incidentSequenceGap, expected, nil // the tail of the prefix is missing
	}
	return prev, "", 0, nil
}
