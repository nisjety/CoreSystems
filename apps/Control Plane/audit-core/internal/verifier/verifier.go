// Package verifier decides whether an org's audit chain is intact. Verify is
// the pure decision: it consumes stored rows in chain order together with the
// signed anchors (newest checkpoint, newest attestation, head) and reports one
// of five statuses, never a bare "ok". The store feeds it rows from Postgres
// (store.VerifyOrg) and cmd/audit-verify feeds it rows from an export, so the
// database check and the offline check cannot disagree about the rules. Run is
// the daily incremental and weekly full loop around the store.
//
// What a verdict means is bounded on purpose. The hash chain alone proves
// internal consistency; anyone who can write the database can rewrite a whole
// chain and recompute it. What they cannot do without the signing key is
// forge an attestation or a checkpoint, so those signatures (and the SIEM or
// customer copy, outside this package) are what pins the chain. Completeness
// is not claimed: rows that never reached audit-core are not in the chain.
package verifier

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
)

// Status is the outcome of one verification. Only Verified and
// VerifiedWithLegacy mean the checked range is intact and anchored; Partial and
// KeyUnavailable mean nothing was found wrong and nothing can be vouched for.
type Status string

const (
	// StatusVerified: the chain from its trust base to the head recomputes, the
	// head is the last row, and a valid signed attestation anchors it.
	StatusVerified Status = "verified"
	// StatusVerifiedWithLegacy: as verified, and the org also has rows written
	// before its chain was activated. Those rows are outside the chain and are
	// not covered by this verdict.
	StatusVerifiedWithLegacy Status = "verified_with_legacy"
	// StatusBroken: a defect was found; FirstBrokenSeq says where.
	StatusBroken Status = "broken"
	// StatusPartial: no defect in the rows read, but the run did not cover the
	// whole chain or lacks an anchor (see Reason).
	StatusPartial Status = "partial"
	// StatusKeyUnavailable: a signature names a key this verifier does not hold,
	// so the trust anchor cannot be checked. The chain may be fine.
	StatusKeyUnavailable Status = "key_unavailable"
)

// Reasons are a closed vocabulary: they are stored, alerted on and printed, and
// never carry row content.
const (
	ReasonSequenceGap          = "sequence_gap"
	ReasonPrefixMissing        = "prefix_missing"
	ReasonDuplicateSequence    = "duplicate_sequence"
	ReasonOrgMismatch          = "org_mismatch"
	ReasonPrevHashMismatch     = "prev_hash_mismatch"
	ReasonCheckpointLink       = "checkpoint_link_mismatch"
	ReasonRowHashMismatch      = "row_hash_mismatch"
	ReasonUnsupportedVersion   = "unsupported_chain_version"
	ReasonUnhashableRow        = "unhashable_row"
	ReasonResumeAnchor         = "resume_anchor_mismatch"
	ReasonTailMissing          = "tail_missing"
	ReasonHeadBehindChain      = "head_behind_chain"
	ReasonHeadHashMismatch     = "head_hash_mismatch"
	ReasonCheckpointSignature  = "checkpoint_signature_invalid"
	ReasonAttestationSignature = "attestation_signature_invalid"
	ReasonAttestationMismatch  = "attestation_hash_mismatch"
	ReasonTruncated            = "truncated_below_attestation"

	ReasonKeyUnavailable = "signing_key_unavailable"
	ReasonNoAttestation  = "no_attestation"
	ReasonNoHead         = "no_head_claim"
	ReasonUnanchored     = "unanchored_start"
	ReasonRowLimit       = "row_limit_reached"
	ReasonNoChain        = "no_chain"
)

// Hash is a byte string that renders as lowercase hex in JSON.
type Hash []byte

func (h Hash) MarshalText() ([]byte, error) { return []byte(hex.EncodeToString(h)), nil }

func (h *Hash) UnmarshalText(text []byte) error {
	if len(text) == 0 {
		*h = nil
		return nil
	}
	decoded, err := hex.DecodeString(string(text))
	if err != nil {
		return fmt.Errorf("hash is not hex: %w", err)
	}
	*h = decoded
	return nil
}

// Checkpoint is a signed retention checkpoint: rows 1..Seq of the chain were
// removed on purpose and the row at Seq had Hash.
type Checkpoint struct {
	Seq         int64     `json:"through_seq"`
	Hash        Hash      `json:"through_hash"`
	PrunedTotal int64     `json:"pruned_rows_total"`
	KeyID       string    `json:"key_id"`
	Signature   Hash      `json:"signature"`
	CreatedAt   time.Time `json:"created_at"`
}

// Attestation is a signed statement of the chain head at one moment.
type Attestation struct {
	Seq       int64     `json:"chain_seq"`
	Hash      Hash      `json:"row_hash"`
	KeyID     string    `json:"key_id"`
	Signature Hash      `json:"signature"`
	SignedAt  time.Time `json:"signed_at"`
}

// Head is the chain head as the store (or an export manifest) claims it.
type Head struct {
	Seq  int64 `json:"seq"`
	Hash Hash  `json:"hash"`
}

// StoredRow is one chained row exactly as stored: the hashed fields, the stored
// link and hash, and the attestation signed at this very sequence, if any.
type StoredRow struct {
	chain.Row
	PrevHash    []byte
	RowHash     []byte
	Version     int16
	Attestation *Attestation
}

// RowSource yields stored rows in ascending chain_seq order, starting at
// Params.StartSeq. Next reports false at the end; an error is an I/O failure,
// not a verdict.
type RowSource interface {
	Next() (StoredRow, bool, error)
}

// Params is everything Verify needs besides the rows.
type Params struct {
	OrgID string
	Keys  *chain.KeyRing
	// Checkpoint is the newest signed checkpoint, nil when the chain was never
	// pruned. Rows then start at Checkpoint.Seq+1.
	Checkpoint *Checkpoint
	// Head is the claimed head. Without it completeness cannot be judged.
	Head *Head
	// Newest is the newest attestation of the org (any sequence). An attestation
	// at a sequence the rows cover must also be attached to that row.
	Newest *Attestation
	// Legacy counts the org's rows that were never chained.
	Legacy int64
	// FromSeq and FromHash resume a run: row FromSeq must hash to FromHash,
	// which was verified earlier. FromSeq at or below the trust base starts at
	// the base; FromSeq above it without FromHash starts unanchored.
	FromSeq  int64
	FromHash []byte
	// MaxRows caps the rows examined; zero means no cap.
	MaxRows int64
}

// Options is what a caller may choose per run; the store fills the rest of
// Params from the database.
type Options struct {
	FromSeq  int64
	FromHash []byte
	MaxRows  int64
}

// StartSeq is the first sequence the row source must deliver.
func (p Params) StartSeq() int64 {
	base := int64(0)
	if p.Checkpoint != nil {
		base = p.Checkpoint.Seq
	}
	if p.FromSeq > base+1 {
		return p.FromSeq
	}
	return base + 1
}

// CheckpointInfo, HeadInfo and AttestedInfo are the report's view of the
// anchors: what was found and whether its signature held.
type CheckpointInfo struct {
	ThroughSeq      int64  `json:"through_seq"`
	PrunedRowsTotal int64  `json:"pruned_rows_total"`
	KeyID           string `json:"key_id"`
	Verified        bool   `json:"verified"`
}

type HeadInfo struct {
	Seq  int64 `json:"seq"`
	Hash Hash  `json:"hash"`
}

type AttestedInfo struct {
	Seq      int64     `json:"seq"`
	KeyID    string    `json:"key_id"`
	SignedAt time.Time `json:"signed_at"`
	Verified bool      `json:"verified"`
}

// Report is the verdict. FirstBrokenSeq is the first sequence that cannot be
// trusted (zero unless Status is broken); for a defect only a signature can
// reveal (a rewritten range under an attestation) it is the attested
// sequence, which is at or after the real change. ThroughSeq and ThroughHash
// are the last row that verified in this run, the cursor an incremental run
// resumes from.
type Report struct {
	OrgID               string          `json:"org_id"`
	Status              Status          `json:"status"`
	Reason              string          `json:"reason,omitempty"`
	FirstBrokenSeq      int64           `json:"first_broken_seq,omitempty"`
	FromSeq             int64           `json:"from_seq"`
	ThroughSeq          int64           `json:"through_seq"`
	ThroughHash         Hash            `json:"through_hash,omitempty"`
	RowsVerified        int64           `json:"rows_verified"`
	Complete            bool            `json:"complete"`
	LegacyUnchainedRows int64           `json:"legacy_unchained_rows"`
	Checkpoint          *CheckpointInfo `json:"checkpoint,omitempty"`
	Head                *HeadInfo       `json:"head,omitempty"`
	Attested            *AttestedInfo   `json:"attested,omitempty"`
}

// OK reports whether the status vouches for the range it covered.
func (r Report) OK() bool {
	return r.Status == StatusVerified || r.Status == StatusVerifiedWithLegacy
}

// NoChain is the report for an org that has no chain head.
func NoChain(orgID string, legacy int64) Report {
	return Report{OrgID: orgID, Status: StatusPartial, Reason: ReasonNoChain, Complete: true, LegacyUnchainedRows: legacy}
}

// Verify checks one org's chain. The returned error is an I/O failure of the
// row source or an unusable Params; a defect in the data is a Report.
func Verify(p Params, src RowSource) (Report, error) {
	if p.OrgID == "" {
		return Report{}, chain.ErrOrgRequired
	}
	baseSeq, basePrev := int64(0), []byte(nil)
	if p.Checkpoint != nil {
		baseSeq, basePrev = p.Checkpoint.Seq, p.Checkpoint.Hash
	} else {
		genesis, err := chain.GenesisHash(p.OrgID)
		if err != nil {
			return Report{}, err
		}
		basePrev = genesis
	}
	start := p.StartSeq()
	resumed := start > baseSeq+1
	anchored := !resumed || len(p.FromHash) > 0

	rep := Report{OrgID: p.OrgID, FromSeq: start, Complete: true, LegacyUnchainedRows: p.Legacy}
	broken := func(seq int64, reason string) (Report, error) {
		if seq < 1 {
			seq = 1
		}
		rep.Status, rep.Reason, rep.FirstBrokenSeq = StatusBroken, reason, seq
		return rep, nil
	}
	keyUnavailable := false
	attestationKeyUnavailable := false

	if cp := p.Checkpoint; cp != nil {
		rep.Checkpoint = &CheckpointInfo{ThroughSeq: cp.Seq, PrunedRowsTotal: cp.PrunedTotal, KeyID: cp.KeyID}
		switch err := p.Keys.VerifyCheckpoint(p.OrgID, cp.Seq, cp.Hash, cp.PrunedTotal, cp.CreatedAt, cp.KeyID, cp.Signature); {
		case err == nil:
			rep.Checkpoint.Verified = true
		case errors.Is(err, chain.ErrUnknownKeyID):
			keyUnavailable = true
		default:
			return broken(cp.Seq, ReasonCheckpointSignature)
		}
	}

	// checkAttestation returns a non-empty reason when the signature is bad.
	checkAttestation := func(a *Attestation) string {
		switch err := p.Keys.VerifyAttestation(p.OrgID, a.Seq, a.Hash, a.SignedAt, a.KeyID, a.Signature); {
		case err == nil:
			return ""
		case errors.Is(err, chain.ErrUnknownKeyID):
			keyUnavailable, attestationKeyUnavailable = true, true
			return ""
		default:
			return ReasonAttestationSignature
		}
	}

	var (
		prev        = basePrev
		expected    = start
		rangeEnd    = start - 1
		count       int64
		first       = true
		lastChecked int64 // newest attestation sequence checked on a row
	)
	for {
		if p.MaxRows > 0 && count >= p.MaxRows {
			rep.Complete = false
			break
		}
		row, ok, err := src.Next()
		if err != nil {
			return Report{}, err
		}
		if !ok {
			break
		}
		seq := row.ChainSeq
		switch {
		case row.OrgID != p.OrgID:
			return broken(seq, ReasonOrgMismatch)
		case seq < expected:
			return broken(seq, ReasonDuplicateSequence)
		case seq > expected && first && !resumed:
			// Rows before the first present one are neither here nor covered by
			// a checkpoint: a dropped or replaced checkpoint looks like this.
			return broken(expected, ReasonPrefixMissing)
		case seq > expected:
			return broken(expected, ReasonSequenceGap)
		}
		if first && resumed {
			prev = row.PrevHash // bound by the row hash below, which must equal FromHash
		}
		switch {
		case !bytes.Equal(row.PrevHash, prev):
			if first && !resumed && p.Checkpoint != nil {
				return broken(seq, ReasonCheckpointLink)
			}
			return broken(seq, ReasonPrevHashMismatch)
		case row.Version != chain.Version:
			return broken(seq, ReasonUnsupportedVersion)
		}
		want, err := chain.RowHash(prev, row.Row)
		if err != nil {
			return broken(seq, ReasonUnhashableRow)
		}
		if !bytes.Equal(want, row.RowHash) {
			return broken(seq, ReasonRowHashMismatch)
		}
		if first && resumed && anchored && !bytes.Equal(want, p.FromHash) {
			return broken(seq, ReasonResumeAnchor)
		}
		if a := row.Attestation; a != nil {
			if reason := checkAttestation(a); reason != "" {
				return broken(a.Seq, reason)
			}
			if !bytes.Equal(a.Hash, want) {
				return broken(seq, ReasonAttestationMismatch)
			}
			lastChecked = a.Seq
		}
		prev, rangeEnd, expected = want, seq, seq+1
		count++
		first = false
	}
	rep.RowsVerified, rep.ThroughSeq, rep.ThroughHash = count, rangeEnd, prev
	if first && resumed {
		rep.ThroughHash = nil // no row was read, so there is no verified hash to resume from
	}

	if !rep.Complete {
		rep.Status, rep.Reason = StatusPartial, ReasonRowLimit
		return rep, nil
	}

	if p.Head != nil {
		rep.Head = &HeadInfo{Seq: p.Head.Seq, Hash: p.Head.Hash}
		switch {
		case rangeEnd < p.Head.Seq:
			return broken(rangeEnd+1, ReasonTailMissing)
		case rangeEnd > p.Head.Seq:
			return broken(p.Head.Seq+1, ReasonHeadBehindChain)
		case !bytes.Equal(prev, p.Head.Hash):
			return broken(p.Head.Seq, ReasonHeadHashMismatch)
		}
	}

	if n := p.Newest; n != nil {
		rep.Attested = &AttestedInfo{Seq: n.Seq, KeyID: n.KeyID, SignedAt: n.SignedAt}
		if n.Seq != lastChecked {
			if reason := checkAttestation(n); reason != "" {
				return broken(n.Seq, reason)
			}
		}
		// A head below an attested sequence means rows were cut off after the
		// signature was made; the signature is what the attacker cannot undo.
		if n.Seq > rangeEnd {
			return broken(rangeEnd+1, ReasonTruncated)
		}
		rep.Attested.Verified = !attestationKeyUnavailable
	}

	switch {
	case keyUnavailable:
		rep.Status, rep.Reason = StatusKeyUnavailable, ReasonKeyUnavailable
	case !anchored:
		rep.Status, rep.Reason = StatusPartial, ReasonUnanchored
	case p.Head == nil:
		rep.Status, rep.Reason = StatusPartial, ReasonNoHead
	case p.Newest == nil:
		rep.Status, rep.Reason = StatusPartial, ReasonNoAttestation
	case p.Legacy > 0:
		rep.Status = StatusVerifiedWithLegacy
	default:
		rep.Status = StatusVerified
	}
	return rep, nil
}

// State is what the loop remembers per org between runs: the last verdict, how
// far the chain is verified (the cursor an incremental run resumes from) and
// the progress of a full run that is spread over several passes.
type State struct {
	Status              Status
	Reason              string
	FirstBrokenSeq      int64
	ThroughSeq          int64
	ThroughHash         []byte
	LegacyUnchainedRows int64
	LastRunAt           time.Time
	LastFullAt          time.Time
	FullCursorSeq       int64
	FullCursorHash      []byte
}

// Source is the store as the loop sees it.
type Source interface {
	ChainOrgs(ctx context.Context) ([]string, error)
	VerifyOrg(ctx context.Context, orgID string, opts Options) (Report, error)
	LoadVerifierState(ctx context.Context, orgID string) (State, error)
	SaveVerifierState(ctx context.Context, orgID string, st State) error
}

const (
	// DefaultInterval is how often an incremental pass runs.
	DefaultInterval = 24 * time.Hour
	// FullEveryPasses is how many incremental passes pass between full ones: a
	// daily interval gives the weekly full verification.
	FullEveryPasses = 7
	// DefaultMaxRows bounds the rows one org contributes to one pass; a larger
	// chain is verified over several passes through the stored cursor.
	DefaultMaxRows = 250_000
)

var (
	verifyFailures = prometheus.NewCounter(prometheus.CounterOpts{
		Name: "audit_core_chain_verify_failures_total",
		Help: "Verification passes that found an org chain broken.",
	})
	lastVerify = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "audit_core_chain_last_verify_timestamp_seconds",
		Help: "Unix time the verifier last finished a pass over every org without an error.",
	})
)

var registerLastVerifyOnce sync.Once

func init() {
	prometheus.MustRegister(verifyFailures)
}

// registerLastVerify exposes the last-verify gauge only once a loop runs: this
// package is linked into every audit-core binary, and a gauge stuck at 0 with
// the chain off would make the stale-verifier alert fire for a feature that is
// not running. The failures counter is harmless at 0, so it registers at init.
func registerLastVerify() {
	registerLastVerifyOnce.Do(func() { prometheus.MustRegister(lastVerify) })
}

// Enabled reads AUDIT_CHAIN_VERIFY_ENABLED. Verification is read-only, so
// unlike the other chain switches it follows the chain: on whenever the chain
// is on, off only for the literal "0" (and always off with the chain off).
func Enabled(raw string, chainEnabled bool) bool {
	return chainEnabled && raw != "0"
}

// ParseInterval reads AUDIT_CHAIN_VERIFY_INTERVAL, the spacing of incremental
// passes. Anything unparsable or shorter than a second keeps the default.
func ParseInterval(raw string) time.Duration {
	if raw == "" {
		return DefaultInterval
	}
	d, err := time.ParseDuration(raw)
	if err != nil || d < time.Second {
		log.Warn().Str("AUDIT_CHAIN_VERIFY_INTERVAL", raw).Dur("default", DefaultInterval).
			Msg("invalid AUDIT_CHAIN_VERIFY_INTERVAL; using default")
		return DefaultInterval
	}
	return d
}

// Config tunes the loop. The zero value gives the defaults.
type Config struct {
	Interval time.Duration
	MaxRows  int64
	Now      func() time.Time // tests only
}

func (c Config) interval() time.Duration {
	if c.Interval <= 0 {
		return DefaultInterval
	}
	return c.Interval
}

func (c Config) maxRows() int64 {
	if c.MaxRows <= 0 {
		return DefaultMaxRows
	}
	return c.MaxRows
}

func (c Config) now() time.Time {
	if c.Now != nil {
		return c.Now().UTC()
	}
	return time.Now().UTC()
}

// Run verifies every org chain once at start and then every interval until ctx
// ends. Each pass resumes from the stored cursor; one pass in FullEveryPasses
// intervals starts over from the trust base so a change inside a range that was
// verified earlier is still found.
func Run(ctx context.Context, src Source, cfg Config) {
	registerLastVerify()
	lastVerify.Set(float64(cfg.now().Unix())) // a loop that never finishes goes stale from here
	pass := func() {
		passCtx, cancel := context.WithTimeout(ctx, cfg.interval())
		defer cancel()
		if err := Pass(passCtx, src, cfg); err != nil {
			log.Error().Err(err).Msg("audit chain verification pass incomplete")
		}
	}
	pass()
	ticker := time.NewTicker(cfg.interval())
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Info().Msg("chain verifier stopping")
			return
		case <-ticker.C:
			pass()
		}
	}
}

// Pass verifies every org once. It keeps going after an org fails so one bad
// chain cannot hide the others, and returns the first error; the last-verify
// timestamp only advances when every org was reached.
func Pass(ctx context.Context, src Source, cfg Config) error {
	orgs, err := src.ChainOrgs(ctx)
	if err != nil {
		return fmt.Errorf("list chain orgs: %w", err)
	}
	var first error
	for _, org := range orgs {
		if err := verifyOrg(ctx, src, cfg, org); err != nil {
			log.Error().Err(err).Str("org_id", org).Msg("audit chain verification failed to run")
			if first == nil {
				first = fmt.Errorf("verify org %s: %w", org, err)
			}
		}
	}
	if first == nil {
		lastVerify.Set(float64(cfg.now().Unix()))
	}
	return first
}

func verifyOrg(ctx context.Context, src Source, cfg Config, org string) error {
	st, err := src.LoadVerifierState(ctx, org)
	if err != nil {
		return err
	}
	now := cfg.now()
	fullEvery := cfg.interval() * FullEveryPasses
	full := st.FullCursorSeq > 0 || st.LastFullAt.IsZero() || now.Sub(st.LastFullAt) >= fullEvery

	opts := Options{MaxRows: cfg.maxRows()}
	switch {
	case full && st.FullCursorSeq > 0:
		opts.FromSeq, opts.FromHash = st.FullCursorSeq, st.FullCursorHash
	case !full && st.ThroughSeq > 0:
		opts.FromSeq, opts.FromHash = st.ThroughSeq, st.ThroughHash
	}
	rep, err := src.VerifyOrg(ctx, org, opts)
	if err != nil {
		return err
	}

	next := st
	next.Status, next.Reason, next.FirstBrokenSeq = rep.Status, rep.Reason, rep.FirstBrokenSeq
	next.LegacyUnchainedRows, next.LastRunAt = rep.LegacyUnchainedRows, now
	switch {
	case rep.Status == StatusBroken:
		// The cursor stays put: the next pass re-reads the same range and finds
		// the same defect until the chain is repaired, instead of resuming
		// past it. A full run restarts from the trust base.
		next.FullCursorSeq, next.FullCursorHash = 0, nil
		verifyFailures.Inc()
		log.Error().Str("org_id", org).Str("reason", rep.Reason).Int64("first_broken_seq", rep.FirstBrokenSeq).
			Msg("audit chain verification found the chain broken")
	default:
		if rep.ThroughSeq > 0 && rep.ThroughHash != nil {
			if full && !rep.Complete {
				next.FullCursorSeq, next.FullCursorHash = rep.ThroughSeq, rep.ThroughHash
			} else {
				next.FullCursorSeq, next.FullCursorHash = 0, nil
			}
			if rep.ThroughSeq > st.ThroughSeq {
				next.ThroughSeq, next.ThroughHash = rep.ThroughSeq, rep.ThroughHash
			}
		}
		if full && rep.Complete {
			next.LastFullAt = now
		}
	}
	return src.SaveVerifierState(ctx, org, next)
}
