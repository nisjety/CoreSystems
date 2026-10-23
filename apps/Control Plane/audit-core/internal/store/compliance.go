package store

import (
	"context"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

const (
	manifestMagic = "VAM1\n"

	// complianceMaxPageRows is the store's own ceiling on one page. The API
	// applies the documented limit (default 200, max 1000) before calling.
	complianceMaxPageRows = 1000

	KeyStatusActive      = "active"
	KeyStatusRetired     = "retired"
	KeyStatusUnavailable = "unavailable"
)

// PayloadSigner signs the manifest payload with the active chain key. The
// production signer is chain.KeyRing once it exposes SignPayload; until then a
// manifest is served unsigned (it still carries the signed checkpoint and
// attestation) rather than invent a second key path here.
type PayloadSigner interface {
	SignPayload(payload []byte) (keyID string, signature []byte, err error)
}

// WithManifestSigner overrides the signer taken from the key ring.
func WithManifestSigner(signer PayloadSigner) Option {
	return func(s *Store) { s.manifestSigner = signer }
}

// ComplianceFilter selects chained rows of one org. Since and Until bound the
// range by ingested_at but resolve to one contiguous chain_seq range, so an
// unfiltered export of a range is gap-free and verifies offline; the field
// filters (Event, UserID, Plane, Outcome) only thin it out and are for reading.
type ComplianceFilter struct {
	OrgID    string
	Event    string
	UserID   string
	Plane    string
	Outcome  string
	Since    time.Time
	Until    time.Time
	AfterSeq int64 // keyset cursor: rows with chain_seq greater than this
	Limit    int
}

// ComplianceEvent is one chained row with every field its hash covers plus the
// stored link and hash, in the shape cmd/audit-verify reads. Hashes are hex,
// ip_address is the host part only.
type ComplianceEvent struct {
	OrgID          string          `json:"org_id"`
	ChainSeq       int64           `json:"chain_seq"`
	OccurredAt     time.Time       `json:"occurred_at"`
	IngestedAt     time.Time       `json:"ingested_at"`
	UserID         string          `json:"user_id"`
	ActorRole      string          `json:"actor_role"`
	Plane          string          `json:"plane"`
	Event          string          `json:"event"`
	Subject        string          `json:"subject"`
	ResourceID     string          `json:"resource_id"`
	Outcome        string          `json:"outcome"`
	EventID        string          `json:"event_id"`
	RequestID      string          `json:"request_id"`
	IPAddress      string          `json:"ip_address"`
	UserAgent      string          `json:"user_agent"`
	SourceProducer string          `json:"source_producer"`
	SourceBus      string          `json:"source_bus"`
	PayloadHash    verifier.Hash   `json:"payload_hash"`
	Details        json.RawMessage `json:"details"`
	ChainVersion   int16           `json:"chain_version"`
	PrevHash       verifier.Hash   `json:"prev_hash"`
	RowHash        verifier.Hash   `json:"row_hash"`
}

// CompliancePage is one keyset page. NextSeq is the cursor to continue from, or
// zero when the range is exhausted. HeadSeq is the org's chain head when the
// page was read.
type CompliancePage struct {
	Events  []ComplianceEvent
	NextSeq int64
	HeadSeq int64
}

// ComplianceEvents reads one page of an org's chained rows in chain_seq order.
// Rows written before the org's chain was activated are not here (they have no
// position); the manifest counts them.
func (s *Store) ComplianceEvents(ctx context.Context, f ComplianceFilter) (CompliancePage, error) {
	if f.OrgID == "" {
		return CompliancePage{}, errors.New("compliance events: org id is required")
	}
	limit := f.Limit
	if limit < 1 || limit > complianceMaxPageRows {
		limit = complianceMaxPageRows
	}
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return CompliancePage{}, fmt.Errorf("begin compliance read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // read-only: nothing to commit

	var page CompliancePage
	page.HeadSeq, err = readHeadSeq(ctx, tx, f.OrgID)
	if err != nil {
		return CompliancePage{}, err
	}
	lo, hi, found, err := complianceSeqRange(ctx, tx, f.OrgID, f.Since, f.Until)
	if err != nil {
		return CompliancePage{}, err
	}
	page.Events = []ComplianceEvent{}
	if !found {
		return page, nil
	}

	rows, err := tx.Query(ctx, `
		SELECT chain_seq, prev_hash, row_hash, COALESCE(chain_version, 0), `+chainRowColumns+`
		FROM audit_events
		WHERE org_id = $1 AND chain_seq > $2 AND chain_seq >= $3 AND chain_seq <= $4
		  AND ($5 = '' OR event = $5)
		  AND ($6 = '' OR user_id = $6)
		  AND ($7 = '' OR plane = $7)
		  AND ($8 = '' OR outcome = $8)
		ORDER BY chain_seq
		LIMIT $9
	`, f.OrgID, f.AfterSeq, lo, hi, f.Event, f.UserID, f.Plane, f.Outcome, limit+1)
	if err != nil {
		return CompliancePage{}, fmt.Errorf("read compliance events: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var (
			seq     int64
			prev    []byte
			stored  []byte
			version int16
		)
		_, row, err := scanChainRow(prefixScanner{rows: rows, prefix: []any{&seq, &prev, &stored, &version}})
		if err != nil {
			return CompliancePage{}, fmt.Errorf("scan compliance event: %w", err)
		}
		page.Events = append(page.Events, ComplianceEvent{
			OrgID: row.OrgID, ChainSeq: seq,
			OccurredAt: row.OccurredAt.UTC(), IngestedAt: row.IngestedAt.UTC(),
			UserID: row.UserID, ActorRole: row.ActorRole, Plane: row.Plane, Event: row.Event,
			Subject: row.Subject, ResourceID: row.ResourceID, Outcome: row.Outcome,
			EventID: row.EventID, RequestID: row.RequestID, IPAddress: row.Host,
			UserAgent: row.UserAgent, SourceProducer: row.SourceProducer, SourceBus: row.SourceBus,
			PayloadHash: row.PayloadHash, Details: json.RawMessage(row.Details),
			ChainVersion: version, PrevHash: prev, RowHash: stored,
		})
	}
	if err := rows.Err(); err != nil {
		return CompliancePage{}, fmt.Errorf("read compliance events: %w", err)
	}
	if len(page.Events) > limit {
		page.Events = page.Events[:limit]
		page.NextSeq = page.Events[limit-1].ChainSeq
	}
	return page, nil
}

// rowQuerier is what the read helpers need from a pool or a transaction.
type rowQuerier interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

func readHeadSeq(ctx context.Context, q rowQuerier, orgID string) (int64, error) {
	var head int64
	err := q.QueryRow(ctx, `SELECT head_seq FROM audit_chain_heads WHERE org_id = $1`, orgID).Scan(&head)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, nil
	}
	if err != nil {
		return 0, fmt.Errorf("read chain head: %w", err)
	}
	return head, nil
}

// complianceSeqRange turns an ingested_at window into the chain_seq range it
// touches: from the first row ingested at or after since to the last row
// ingested at or before until. ingested_at is not commit-ordered, so the window
// is only used to find the ends; everything between them is returned, which is
// what keeps the range contiguous. ok is false when no row falls in the window.
func complianceSeqRange(ctx context.Context, q rowQuerier, orgID string, since, until time.Time) (lo, hi int64, ok bool, err error) {
	lo, hi = 1, math.MaxInt64
	if !since.IsZero() {
		var first *int64
		if err := q.QueryRow(ctx, `
			SELECT MIN(chain_seq) FROM audit_events
			WHERE org_id = $1 AND chain_seq IS NOT NULL AND ingested_at >= $2
		`, orgID, since).Scan(&first); err != nil {
			return 0, 0, false, fmt.Errorf("resolve range start: %w", err)
		}
		if first == nil {
			return 0, 0, false, nil
		}
		lo = *first
	}
	if !until.IsZero() {
		var last *int64
		if err := q.QueryRow(ctx, `
			SELECT MAX(chain_seq) FROM audit_events
			WHERE org_id = $1 AND chain_seq IS NOT NULL AND ingested_at <= $2
		`, orgID, until).Scan(&last); err != nil {
			return 0, 0, false, fmt.Errorf("resolve range end: %w", err)
		}
		if last == nil {
			return 0, 0, false, nil
		}
		hi = *last
	}
	return lo, hi, lo <= hi, nil
}

// ComplianceManifest describes a range of an org's chain: its ends, its size,
// the current head and the signed anchors. The field names that cmd/audit-verify
// reads (org_id, last_seq, last_row_hash, checkpoint, attestation,
// legacy_unchained_rows) are the same, so a saved manifest feeds it directly
// when the range reaches the head. Nothing in it is trusted beyond its
// signatures: the verifier still recomputes every row.
type ComplianceManifest struct {
	OrgID               string                `json:"org_id"`
	GeneratedAt         time.Time             `json:"generated_at"`
	FirstSeq            int64                 `json:"first_seq"`
	LastSeq             int64                 `json:"last_seq"`
	Count               int64                 `json:"count"`
	FirstPrevHash       verifier.Hash         `json:"first_prev_hash"`
	LastRowHash         verifier.Hash         `json:"last_row_hash"`
	HeadSeq             int64                 `json:"head_seq"`
	HeadHash            verifier.Hash         `json:"head_hash"`
	LegacyUnchainedRows int64                 `json:"legacy_unchained_rows"`
	Checkpoint          *verifier.Checkpoint  `json:"checkpoint"`
	Attestation         *verifier.Attestation `json:"attestation"`
	KeyID               string                `json:"key_id,omitempty"`
	Signature           verifier.Hash         `json:"signature,omitempty"`
}

// ManifestPayload is the exact byte string a manifest signature covers:
// "VAM1\n"+org+"\n"+first_seq+"\n"+last_seq+"\n"+count+"\n"+hex(first_prev_hash)+
// "\n"+hex(last_row_hash)+"\n"+head_seq+"\n"+hex(head_hash)+"\n"+legacy+"\n"+generated_at.
// The hash fields are empty for an empty range or an org without a chain.
func ManifestPayload(m ComplianceManifest) ([]byte, error) {
	switch {
	case m.OrgID == "":
		return nil, chain.ErrOrgRequired
	case strings.ContainsRune(m.OrgID, '\n'):
		return nil, chain.ErrOrgNewline
	}
	for _, h := range []verifier.Hash{m.FirstPrevHash, m.LastRowHash, m.HeadHash} {
		if len(h) != 0 && len(h) != chain.HashSize {
			return nil, chain.ErrBadHash
		}
	}
	return []byte(manifestMagic + m.OrgID + "\n" +
		strconv.FormatInt(m.FirstSeq, 10) + "\n" + strconv.FormatInt(m.LastSeq, 10) + "\n" +
		strconv.FormatInt(m.Count, 10) + "\n" +
		hex.EncodeToString(m.FirstPrevHash) + "\n" + hex.EncodeToString(m.LastRowHash) + "\n" +
		strconv.FormatInt(m.HeadSeq, 10) + "\n" + hex.EncodeToString(m.HeadHash) + "\n" +
		strconv.FormatInt(m.LegacyUnchainedRows, 10) + "\n" +
		m.GeneratedAt.UTC().Format(chain.SignedTimeLayout)), nil
}

// SignManifest signs m in place. A signer that holds no key leaves the manifest
// unsigned (key_id and signature empty) instead of failing the read: the
// signed checkpoint and attestation it carries are still the trust anchors.
func SignManifest(m *ComplianceManifest, signer PayloadSigner) error {
	if signer == nil {
		return nil
	}
	payload, err := ManifestPayload(*m)
	if err != nil {
		return err
	}
	keyID, signature, err := signer.SignPayload(payload)
	if errors.Is(err, chain.ErrNoSigningKey) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("sign manifest: %w", err)
	}
	m.KeyID, m.Signature = keyID, signature
	return nil
}

func (s *Store) payloadSigner() PayloadSigner {
	if s.manifestSigner != nil {
		return s.manifestSigner
	}
	if signer, ok := any(s.keys).(PayloadSigner); ok {
		return signer
	}
	return nil
}

// ComplianceManifest reads the manifest of an org's range in one snapshot and
// signs it. A zero since or until leaves that end open.
func (s *Store) ComplianceManifest(ctx context.Context, orgID string, since, until time.Time) (ComplianceManifest, error) {
	if orgID == "" {
		return ComplianceManifest{}, errors.New("compliance manifest: org id is required")
	}
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return ComplianceManifest{}, fmt.Errorf("begin compliance read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // read-only: nothing to commit

	m := ComplianceManifest{OrgID: orgID, GeneratedAt: time.Now().UTC().Truncate(time.Microsecond)}
	if err := tx.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`, orgID,
	).Scan(&m.LegacyUnchainedRows); err != nil {
		return ComplianceManifest{}, fmt.Errorf("count legacy rows: %w", err)
	}

	var headHash []byte
	switch err := tx.QueryRow(ctx,
		`SELECT head_seq, head_hash FROM audit_chain_heads WHERE org_id = $1`, orgID,
	).Scan(&m.HeadSeq, &headHash); {
	case errors.Is(err, pgx.ErrNoRows):
		return s.finishManifest(m)
	case err != nil:
		return ComplianceManifest{}, fmt.Errorf("read chain head: %w", err)
	}
	m.HeadHash = headHash

	lo, hi, found, err := complianceSeqRange(ctx, tx, orgID, since, until)
	if err != nil {
		return ComplianceManifest{}, err
	}
	if found {
		var first, last *int64
		if err := tx.QueryRow(ctx, `
			SELECT MIN(chain_seq), MAX(chain_seq), COUNT(*) FROM audit_events
			WHERE org_id = $1 AND chain_seq >= $2 AND chain_seq <= $3
		`, orgID, lo, hi).Scan(&first, &last, &m.Count); err != nil {
			return ComplianceManifest{}, fmt.Errorf("read range ends: %w", err)
		}
		if first != nil && last != nil {
			m.FirstSeq, m.LastSeq = *first, *last
			var firstPrev, lastHash []byte
			if err := tx.QueryRow(ctx,
				`SELECT prev_hash FROM audit_events WHERE org_id = $1 AND chain_seq = $2`, orgID, m.FirstSeq,
			).Scan(&firstPrev); err != nil {
				return ComplianceManifest{}, fmt.Errorf("read first link: %w", err)
			}
			if err := tx.QueryRow(ctx,
				`SELECT row_hash FROM audit_events WHERE org_id = $1 AND chain_seq = $2`, orgID, m.LastSeq,
			).Scan(&lastHash); err != nil {
				return ComplianceManifest{}, fmt.Errorf("read last hash: %w", err)
			}
			m.FirstPrevHash, m.LastRowHash = firstPrev, lastHash
		}
	}

	var cp verifier.Checkpoint
	var cpHash, cpSignature []byte
	switch err := tx.QueryRow(ctx, `
		SELECT through_seq, through_hash, pruned_rows_total, key_id, signature, created_at
		FROM audit_chain_checkpoints
		WHERE org_id = $1
		ORDER BY through_seq DESC
		LIMIT 1
	`, orgID).Scan(&cp.Seq, &cpHash, &cp.PrunedTotal, &cp.KeyID, &cpSignature, &cp.CreatedAt); {
	case err == nil:
		cp.Hash, cp.Signature, cp.CreatedAt = cpHash, cpSignature, cp.CreatedAt.UTC()
		m.Checkpoint = &cp
	case !errors.Is(err, pgx.ErrNoRows):
		return ComplianceManifest{}, fmt.Errorf("read newest checkpoint: %w", err)
	}

	var att verifier.Attestation
	var attHash, attSignature []byte
	switch err := tx.QueryRow(ctx, `
		SELECT chain_seq, row_hash, key_id, signature, signed_at
		FROM audit_chain_attestations
		WHERE org_id = $1
		ORDER BY chain_seq DESC
		LIMIT 1
	`, orgID).Scan(&att.Seq, &attHash, &att.KeyID, &attSignature, &att.SignedAt); {
	case err == nil:
		att.Hash, att.Signature, att.SignedAt = attHash, attSignature, att.SignedAt.UTC()
		m.Attestation = &att
	case !errors.Is(err, pgx.ErrNoRows):
		return ComplianceManifest{}, fmt.Errorf("read newest attestation: %w", err)
	}
	return s.finishManifest(m)
}

func (s *Store) finishManifest(m ComplianceManifest) (ComplianceManifest, error) {
	if err := SignManifest(&m, s.payloadSigner()); err != nil {
		return ComplianceManifest{}, err
	}
	return m, nil
}

// ComplianceKey is one public key a signature of the org may name.
type ComplianceKey struct {
	KeyID     string `json:"key_id"`
	Algorithm string `json:"algorithm"`
	// PublicKey is base64url without padding, the encoding of
	// AUDIT_CHAIN_PUBLIC_KEY_BASE64. Empty when this server does not hold it.
	PublicKey string `json:"public_key,omitempty"`
	Status    string `json:"status"`
}

// ComplianceKeys lists the keys needed to verify the org's anchors offline: the
// active key plus the keys that signed its newest checkpoint and attestation. A
// key a signature names but the ring does not hold is listed as unavailable, so
// a verifier learns it cannot check that anchor instead of finding nothing.
func (s *Store) ComplianceKeys(ctx context.Context, orgID string) ([]ComplianceKey, error) {
	ids := map[string]struct{}{}
	if active := s.keys.ActiveKeyID(); active != "" {
		ids[active] = struct{}{}
	}
	for _, query := range []string{
		`SELECT key_id FROM audit_chain_checkpoints WHERE org_id = $1 ORDER BY through_seq DESC LIMIT 1`,
		`SELECT key_id FROM audit_chain_attestations WHERE org_id = $1 ORDER BY chain_seq DESC LIMIT 1`,
	} {
		var id string
		switch err := s.pool.QueryRow(ctx, query, orgID).Scan(&id); {
		case err == nil:
			ids[id] = struct{}{}
		case !errors.Is(err, pgx.ErrNoRows):
			return nil, fmt.Errorf("read signing key ids: %w", err)
		}
	}

	keys := make([]ComplianceKey, 0, len(ids))
	for id := range ids {
		key := ComplianceKey{KeyID: id, Algorithm: "ed25519", Status: KeyStatusUnavailable}
		if pub, ok := s.keys.PublicKey(id); ok {
			key.PublicKey = base64.RawURLEncoding.EncodeToString(pub)
			key.Status = KeyStatusRetired
			if id == s.keys.ActiveKeyID() {
				key.Status = KeyStatusActive
			}
		}
		keys = append(keys, key)
	}
	sort.Slice(keys, func(i, j int) bool { return keys[i].KeyID < keys[j].KeyID })
	return keys, nil
}
