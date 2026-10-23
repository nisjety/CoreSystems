package verifier

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
)

// These tests exercise the decision without a database: rows are built with the
// real chain.RowHash, anchors are signed with a real key ring, and each attack
// below edits the in-memory copy the way a database writer could.

const testOrg = "org-verifier"

func seedOf(b byte) []byte {
	seed := make([]byte, ed25519.SeedSize)
	for i := range seed {
		seed[i] = b + byte(i)
	}
	return seed
}

func ringFor(t *testing.T, keyID string, seed byte, verifyKeys string) *chain.KeyRing {
	t.Helper()
	ring, err := chain.NewKeyRing(chain.KeyConfig{
		KeyID:      keyID,
		Private:    base64.RawURLEncoding.EncodeToString(seedOf(seed)),
		VerifyKeys: verifyKeys,
	})
	if err != nil {
		t.Fatal(err)
	}
	return ring
}

func publicOf(seed byte) string {
	pub := ed25519.NewKeyFromSeed(seedOf(seed)).Public().(ed25519.PublicKey)
	return base64.RawURLEncoding.EncodeToString(pub)
}

// fixture is one org's chain with its anchors, ready to be edited and verified.
type fixture struct {
	rows       []StoredRow
	head       Head
	newest     *Attestation
	checkpoint *Checkpoint
	ring       *chain.KeyRing
	legacy     int64
}

var epoch = time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)

// hashRow recomputes a row hash the way the inserter does.
func hashRow(t *testing.T, prev []byte, r chain.Row) []byte {
	t.Helper()
	h, err := chain.RowHash(prev, r)
	if err != nil {
		t.Fatal(err)
	}
	return h
}

func buildRows(t *testing.T, startSeq, n int64, prev []byte) []StoredRow {
	t.Helper()
	var rows []StoredRow
	for seq := startSeq; seq < startSeq+n; seq++ {
		r := chain.Row{
			OrgID:      testOrg,
			ChainSeq:   seq,
			OccurredAt: epoch.Add(time.Duration(seq) * time.Second),
			IngestedAt: epoch.Add(time.Duration(seq)*time.Second + time.Millisecond),
			UserID:     "user-1",
			Plane:      "control",
			Event:      "verifier_test",
			Outcome:    "ok",
			EventID:    fmt.Sprintf("evt-%d", seq),
			Details:    []byte(fmt.Sprintf(`{"n": %d, "tool": "knowledge_search"}`, seq)),
		}
		hash := hashRow(t, prev, r)
		rows = append(rows, StoredRow{Row: r, PrevHash: prev, RowHash: hash, Version: chain.Version})
		prev = hash
	}
	return rows
}

func attest(t *testing.T, ring *chain.KeyRing, rows []StoredRow, seq int64) *Attestation {
	t.Helper()
	row := rows[seq-rows[0].ChainSeq]
	signedAt := epoch.Add(time.Hour + time.Duration(seq)*time.Second)
	keyID, sig, err := ring.SignAttestation(testOrg, seq, row.RowHash, signedAt)
	if err != nil {
		t.Fatal(err)
	}
	return &Attestation{Seq: seq, Hash: row.RowHash, KeyID: keyID, Signature: sig, SignedAt: signedAt}
}

// newFixture builds rows 1..n attested at each of attestAt (the last one is the
// newest attestation).
func newFixture(t *testing.T, n int64, attestAt ...int64) *fixture {
	t.Helper()
	genesis, err := chain.GenesisHash(testOrg)
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{ring: ringFor(t, "key-1", 1, ""), rows: buildRows(t, 1, n, genesis)}
	for _, seq := range attestAt {
		a := attest(t, f.ring, f.rows, seq)
		f.rows[seq-1].Attestation = a
		f.newest = a
	}
	last := f.rows[len(f.rows)-1]
	f.head = Head{Seq: last.ChainSeq, Hash: last.RowHash}
	return f
}

// prune turns rows 1..through into a signed checkpoint, as the purge does.
func (f *fixture) prune(t *testing.T, through int64) {
	t.Helper()
	cutoff := f.rows[through-1]
	createdAt := epoch.Add(2 * time.Hour)
	keyID, sig, err := f.ring.SignCheckpoint(testOrg, through, cutoff.RowHash, through, createdAt)
	if err != nil {
		t.Fatal(err)
	}
	f.checkpoint = &Checkpoint{Seq: through, Hash: cutoff.RowHash, PrunedTotal: through, KeyID: keyID, Signature: sig, CreatedAt: createdAt}
	f.rows = f.rows[through:]
}

// rechain recomputes the hashes of rows[from:] after an edit, and the head, the
// way an attacker without the signing key rewrites a chain.
func (f *fixture) rechain(t *testing.T, from int) {
	t.Helper()
	prev := f.rows[from].PrevHash
	for i := from; i < len(f.rows); i++ {
		f.rows[i].PrevHash = prev
		f.rows[i].RowHash = hashRow(t, prev, f.rows[i].Row)
		prev = f.rows[i].RowHash
	}
	last := f.rows[len(f.rows)-1]
	f.head = Head{Seq: last.ChainSeq, Hash: last.RowHash}
}

type sliceSource struct {
	rows []StoredRow
	next int
	err  error
}

func (s *sliceSource) Next() (StoredRow, bool, error) {
	if s.err != nil && s.next >= len(s.rows) {
		return StoredRow{}, false, s.err
	}
	if s.next >= len(s.rows) {
		return StoredRow{}, false, nil
	}
	row := s.rows[s.next]
	s.next++
	return row, true, nil
}

func (f *fixture) params() Params {
	head := f.head
	return Params{OrgID: testOrg, Keys: f.ring, Checkpoint: f.checkpoint, Head: &head, Newest: f.newest, Legacy: f.legacy}
}

func (f *fixture) verify(t *testing.T) Report {
	t.Helper()
	return f.verifyWith(t, f.params())
}

func (f *fixture) verifyWith(t *testing.T, p Params) Report {
	t.Helper()
	var rows []StoredRow
	for _, r := range f.rows {
		if r.ChainSeq >= p.StartSeq() {
			rows = append(rows, r)
		}
	}
	rep, err := Verify(p, &sliceSource{rows: rows})
	if err != nil {
		t.Fatal(err)
	}
	return rep
}

func assertBroken(t *testing.T, rep Report, seq int64, reason string) {
	t.Helper()
	if rep.Status != StatusBroken || rep.FirstBrokenSeq != seq || rep.Reason != reason {
		t.Fatalf("report = %s/%s first_broken_seq=%d, want broken/%s at %d", rep.Status, rep.Reason, rep.FirstBrokenSeq, reason, seq)
	}
}

func assertStatus(t *testing.T, rep Report, status Status, reason string) {
	t.Helper()
	if rep.Status != status || rep.Reason != reason {
		t.Fatalf("report = %s/%s (first_broken_seq=%d), want %s/%q", rep.Status, rep.Reason, rep.FirstBrokenSeq, status, reason)
	}
}

func TestCleanChainIsVerified(t *testing.T) {
	f := newFixture(t, 10, 5, 10)
	rep := f.verify(t)
	assertStatus(t, rep, StatusVerified, "")
	if rep.RowsVerified != 10 || rep.ThroughSeq != 10 || !rep.Complete {
		t.Fatalf("report coverage = %+v", rep)
	}
	if rep.Attested == nil || !rep.Attested.Verified || rep.Attested.Seq != 10 {
		t.Fatalf("attested = %+v", rep.Attested)
	}
}

func TestLegacyRowsAreReportedNotHidden(t *testing.T) {
	f := newFixture(t, 4, 4)
	f.legacy = 7
	rep := f.verify(t)
	assertStatus(t, rep, StatusVerifiedWithLegacy, "")
	if rep.LegacyUnchainedRows != 7 {
		t.Fatalf("legacy = %d", rep.LegacyUnchainedRows)
	}
}

// The attack table of the plan: each tamper is detected and the first broken
// sequence is the first one that cannot be trusted.
func TestAttackTable(t *testing.T) {
	cases := []struct {
		name   string
		build  func(t *testing.T) *fixture
		seq    int64
		reason string
	}{
		{
			name: "edit a field",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.rows[3].Outcome = "denied"
				return f
			},
			seq: 4, reason: ReasonRowHashMismatch,
		},
		{
			name: "edit details",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.rows[5].Details = []byte(`{"n": 99}`)
				return f
			},
			seq: 6, reason: ReasonRowHashMismatch,
		},
		{
			name: "delete a middle row",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.rows = append(f.rows[:4:4], f.rows[5:]...)
				return f
			},
			seq: 5, reason: ReasonSequenceGap,
		},
		{
			name: "delete the tail",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.rows = f.rows[:8]
				return f
			},
			seq: 9, reason: ReasonTailMissing,
		},
		{
			name: "delete the tail and lower the head",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 5, 10)
				f.rows = f.rows[:7]
				f.head = Head{Seq: 7, Hash: f.rows[6].RowHash}
				return f
			},
			seq: 8, reason: ReasonTruncated,
		},
		{
			name: "swap two chain_seq values",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.rows[2].ChainSeq, f.rows[3].ChainSeq = 4, 3
				f.rows[2], f.rows[3] = f.rows[3], f.rows[2]
				return f
			},
			seq: 3, reason: ReasonPrevHashMismatch,
		},
		{
			name: "forge the head hash",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.head.Hash = bytes.Repeat([]byte{0xab}, chain.HashSize)
				return f
			},
			seq: 10, reason: ReasonHeadHashMismatch,
		},
		{
			name: "raise the head past the rows",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.head.Seq = 12
				return f
			},
			seq: 11, reason: ReasonTailMissing,
		},
		{
			name: "rewrite a row and recompute the tail, keeping the attestations",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 5, 10)
				f.rows[6].Outcome = "denied"
				f.rechain(t, 6)
				return f
			},
			// The rewritten range is self-consistent, so only the signature at 10
			// contradicts it; the attestation at 5 still matches.
			seq: 10, reason: ReasonAttestationMismatch,
		},
		{
			name: "rewrite the whole chain without the key",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 5, 10)
				f.rows[1].Outcome = "denied"
				f.rechain(t, 1)
				return f
			},
			seq: 5, reason: ReasonAttestationMismatch,
		},
		{
			name: "forge an attestation with another key under the real key id",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				rogue := ringFor(t, "key-1", 9, "")
				_, sig, err := rogue.SignAttestation(testOrg, 10, f.rows[9].RowHash, f.newest.SignedAt)
				if err != nil {
					t.Fatal(err)
				}
				forged := *f.newest
				forged.Signature = sig
				f.rows[9].Attestation = &forged
				f.newest = &forged
				return f
			},
			seq: 10, reason: ReasonAttestationSignature,
		},
		{
			name: "drop the checkpoint after a prune",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.prune(t, 4)
				f.checkpoint = nil
				return f
			},
			seq: 1, reason: ReasonPrefixMissing,
		},
		{
			name: "replace the checkpoint with an older one",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 12, 12)
				older := newFixture(t, 12, 12)
				older.prune(t, 4)
				f.prune(t, 8)
				f.checkpoint = older.checkpoint
				return f
			},
			seq: 5, reason: ReasonPrefixMissing,
		},
		{
			name: "forge a checkpoint with another key under the real key id",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.prune(t, 4)
				rogue := ringFor(t, "key-1", 9, "")
				_, sig, err := rogue.SignCheckpoint(testOrg, 4, f.checkpoint.Hash, 4, f.checkpoint.CreatedAt)
				if err != nil {
					t.Fatal(err)
				}
				f.checkpoint.Signature = sig
				return f
			},
			seq: 4, reason: ReasonCheckpointSignature,
		},
		{
			name: "rewrite the rows that follow a checkpoint",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 10, 10)
				f.prune(t, 4)
				f.rows[0].Outcome = "denied"
				f.rechain(t, 0)
				return f
			},
			seq: 10, reason: ReasonAttestationMismatch,
		},
		{
			name: "unsupported chain version",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 6, 6)
				f.rows[2].Version = 2
				return f
			},
			seq: 3, reason: ReasonUnsupportedVersion,
		},
		{
			name: "row from another org",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 6, 6)
				f.rows[2].OrgID = "org-other"
				return f
			},
			seq: 3, reason: ReasonOrgMismatch,
		},
		{
			name: "duplicated sequence",
			build: func(t *testing.T) *fixture {
				f := newFixture(t, 6, 6)
				f.rows = append(f.rows[:3:3], f.rows[2:]...)
				return f
			},
			seq: 3, reason: ReasonDuplicateSequence,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			assertBroken(t, tc.build(t).verify(t), tc.seq, tc.reason)
		})
	}
}

func TestPrunedChainWithValidCheckpointIsVerified(t *testing.T) {
	f := newFixture(t, 12, 12)
	f.prune(t, 5)
	rep := f.verify(t)
	assertStatus(t, rep, StatusVerified, "")
	if rep.Checkpoint == nil || !rep.Checkpoint.Verified || rep.Checkpoint.ThroughSeq != 5 || rep.FromSeq != 6 {
		t.Fatalf("report = %+v", rep)
	}
}

func TestCheckpointWithUnknownKeyIsKeyUnavailableNotBroken(t *testing.T) {
	f := newFixture(t, 10, 10)
	f.prune(t, 4)
	rogue := ringFor(t, "rogue", 9, "")
	keyID, sig, err := rogue.SignCheckpoint(testOrg, 4, f.checkpoint.Hash, 4, f.checkpoint.CreatedAt)
	if err != nil {
		t.Fatal(err)
	}
	f.checkpoint.KeyID, f.checkpoint.Signature = keyID, sig
	assertStatus(t, f.verify(t), StatusKeyUnavailable, ReasonKeyUnavailable)
}

func TestRetiredKeyStillVerifiesAfterRotation(t *testing.T) {
	f := newFixture(t, 10, 10) // signed by key-1
	f.ring = ringFor(t, "key-2", 2, "key-1:"+publicOf(1))
	assertStatus(t, f.verify(t), StatusVerified, "")
}

func TestVerifierWithoutKeysCannotVouch(t *testing.T) {
	f := newFixture(t, 10, 10)
	f.ring = nil
	assertStatus(t, f.verify(t), StatusKeyUnavailable, ReasonKeyUnavailable)
}

func TestHashBreakIsFoundEvenWithoutKeys(t *testing.T) {
	f := newFixture(t, 10, 10)
	f.ring = nil
	f.rows[6].Outcome = "denied"
	assertBroken(t, f.verify(t), 7, ReasonRowHashMismatch)
}

func TestChainWithoutAttestationIsPartialNeverVerified(t *testing.T) {
	f := newFixture(t, 10)
	assertStatus(t, f.verify(t), StatusPartial, ReasonNoAttestation)
}

// Deleting every attestation together with the truncated rows and the head
// leaves nothing signed to contradict the shorter chain. This is the stated
// limit of the design: only an external witness (SIEM or customer export)
// closes it. What the verifier must still do is refuse to say "verified".
func TestTruncationWithAttestationsDeletedIsPartialNeverVerified(t *testing.T) {
	f := newFixture(t, 10, 5, 10)
	f.rows = f.rows[:5]
	f.rows[4].Attestation = nil
	f.head = Head{Seq: 5, Hash: f.rows[4].RowHash}
	f.newest = nil
	assertStatus(t, f.verify(t), StatusPartial, ReasonNoAttestation)
}

func TestRowLimitStopsPartialAndResumeCompletes(t *testing.T) {
	f := newFixture(t, 10, 5, 10)
	p := f.params()
	p.MaxRows = 4
	rep := f.verifyWith(t, p)
	assertStatus(t, rep, StatusPartial, ReasonRowLimit)
	if rep.ThroughSeq != 4 || rep.RowsVerified != 4 || rep.Complete || len(rep.ThroughHash) != chain.HashSize {
		t.Fatalf("partial report = %+v", rep)
	}

	// Resume at the last verified row: it must hash to the stored cursor.
	p = f.params()
	p.FromSeq, p.FromHash = rep.ThroughSeq, rep.ThroughHash
	resumed := f.verifyWith(t, p)
	assertStatus(t, resumed, StatusVerified, "")
	if resumed.FromSeq != 4 || resumed.RowsVerified != 7 || resumed.ThroughSeq != 10 {
		t.Fatalf("resumed report = %+v", resumed)
	}
}

func TestResumeFindsAChangeInsideTheAlreadyVerifiedRange(t *testing.T) {
	f := newFixture(t, 10, 10)
	cursorSeq, cursorHash := int64(4), append([]byte(nil), f.rows[3].RowHash...)
	f.rows[3].Outcome = "denied"
	f.rechain(t, 3)
	p := f.params()
	p.FromSeq, p.FromHash = cursorSeq, cursorHash
	assertBroken(t, f.verifyWith(t, p), 4, ReasonResumeAnchor)
}

func TestResumeWithoutAnAnchorIsPartial(t *testing.T) {
	f := newFixture(t, 10, 10)
	p := f.params()
	p.FromSeq = 6
	assertStatus(t, f.verifyWith(t, p), StatusPartial, ReasonUnanchored)
}

func TestResumeBelowTheCheckpointStartsAtTheCheckpoint(t *testing.T) {
	f := newFixture(t, 12, 12)
	f.prune(t, 8)
	p := f.params()
	p.FromSeq, p.FromHash = 4, bytes.Repeat([]byte{1}, chain.HashSize) // pruned since the cursor was saved
	rep := f.verifyWith(t, p)
	assertStatus(t, rep, StatusVerified, "")
	if rep.FromSeq != 9 {
		t.Fatalf("from_seq = %d, want 9", rep.FromSeq)
	}
}

func TestMissingHeadClaimIsPartial(t *testing.T) {
	f := newFixture(t, 6, 6)
	p := f.params()
	p.Head = nil
	assertStatus(t, f.verifyWith(t, p), StatusPartial, ReasonNoHead)
}

func TestSourceErrorIsNotAVerdict(t *testing.T) {
	f := newFixture(t, 4, 4)
	boom := errors.New("connection reset")
	_, err := Verify(f.params(), &sliceSource{rows: f.rows[:2], err: boom})
	if !errors.Is(err, boom) {
		t.Fatalf("err = %v, want the source error", err)
	}
}

func TestNoChainReportIsPartial(t *testing.T) {
	rep := NoChain("org-x", 3)
	assertStatus(t, rep, StatusPartial, ReasonNoChain)
	if rep.OK() || rep.LegacyUnchainedRows != 3 {
		t.Fatalf("report = %+v", rep)
	}
}

func TestHashRendersAsHexJSON(t *testing.T) {
	var h Hash
	if err := h.UnmarshalText([]byte("0aff")); err != nil || !bytes.Equal(h, []byte{0x0a, 0xff}) {
		t.Fatalf("hash = %x err = %v", []byte(h), err)
	}
	out, _ := h.MarshalText()
	if string(out) != "0aff" {
		t.Fatalf("hex = %s", out)
	}
	if err := h.UnmarshalText([]byte("zz")); err == nil {
		t.Fatal("non-hex text must be rejected")
	}
}

func TestEnabledFollowsTheChainUnlessSwitchedOff(t *testing.T) {
	cases := []struct {
		raw   string
		chain bool
		want  bool
	}{
		{"", true, true}, {"1", true, true}, {"0", true, false},
		{"", false, false}, {"1", false, false}, {"garbage", true, true},
	}
	for _, tc := range cases {
		if got := Enabled(tc.raw, tc.chain); got != tc.want {
			t.Errorf("Enabled(%q, %v) = %v, want %v", tc.raw, tc.chain, got, tc.want)
		}
	}
}

func TestParseIntervalKeepsTheDefaultForBadValues(t *testing.T) {
	for raw, want := range map[string]time.Duration{
		"": DefaultInterval, "6h": 6 * time.Hour, "2s": 2 * time.Second,
		"0s": DefaultInterval, "-1h": DefaultInterval, "soon": DefaultInterval, "500ms": DefaultInterval,
	} {
		if got := ParseInterval(raw); got != want {
			t.Errorf("ParseInterval(%q) = %v, want %v", raw, got, want)
		}
	}
}

// fakeSource is the store as the loop sees it: it runs the real Verify over an
// in-memory chain and keeps the saved state.
type fakeSource struct {
	t      *testing.T
	fix    *fixture
	state  State
	saved  []State
	opts   []Options
	failOn error
}

func (s *fakeSource) ChainOrgs(context.Context) ([]string, error) { return []string{testOrg}, nil }

func (s *fakeSource) VerifyOrg(_ context.Context, _ string, opts Options) (Report, error) {
	if s.failOn != nil {
		return Report{}, s.failOn
	}
	s.opts = append(s.opts, opts)
	p := s.fix.params()
	p.FromSeq, p.FromHash, p.MaxRows = opts.FromSeq, opts.FromHash, opts.MaxRows
	return s.fix.verifyWith(s.t, p), nil
}

func (s *fakeSource) LoadVerifierState(context.Context, string) (State, error) { return s.state, nil }

func (s *fakeSource) SaveVerifierState(_ context.Context, _ string, st State) error {
	s.state = st
	s.saved = append(s.saved, st)
	return nil
}

func TestLoopRunsIncrementallyThenFullOnTheWeeklyCadence(t *testing.T) {
	f := newFixture(t, 10, 10)
	src := &fakeSource{t: t, fix: f}
	now := epoch
	cfg := Config{Interval: 24 * time.Hour, Now: func() time.Time { return now }}

	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	if got := src.state; got.Status != StatusVerified || got.ThroughSeq != 10 || got.LastFullAt.IsZero() {
		t.Fatalf("first pass state = %+v (the first pass is a full run)", got)
	}

	// New rows arrive: the next day's pass resumes from the saved cursor.
	grown := newFixture(t, 14, 14)
	src.fix = grown
	now = now.Add(24 * time.Hour)
	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	last := src.opts[len(src.opts)-1]
	if last.FromSeq != 10 || len(last.FromHash) != chain.HashSize {
		t.Fatalf("incremental pass options = %+v, want resume at 10", last)
	}
	if src.state.ThroughSeq != 14 {
		t.Fatalf("through = %d, want 14", src.state.ThroughSeq)
	}

	// A week after the last full run the pass starts over from the base.
	now = now.Add(7 * 24 * time.Hour)
	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	if last := src.opts[len(src.opts)-1]; last.FromSeq != 0 {
		t.Fatalf("weekly pass options = %+v, want a full run", last)
	}
}

func TestLoopSpreadsAFullRunOverPassesWithTheRowCap(t *testing.T) {
	f := newFixture(t, 10, 10)
	src := &fakeSource{t: t, fix: f}
	cfg := Config{Interval: time.Hour, MaxRows: 4, Now: func() time.Time { return epoch }}

	var cursors []int64
	for i := 0; i < 6 && src.state.LastFullAt.IsZero(); i++ {
		if err := Pass(context.Background(), src, cfg); err != nil {
			t.Fatal(err)
		}
		cursors = append(cursors, src.state.FullCursorSeq)
	}
	if src.state.LastFullAt.IsZero() || src.state.FullCursorSeq != 0 || src.state.Status != StatusVerified {
		t.Fatalf("state after the capped passes = %+v (cursors %v)", src.state, cursors)
	}
	if len(cursors) < 3 || cursors[0] != 4 || cursors[1] != 7 {
		t.Fatalf("full run cursors = %v, want it to advance 4, 7, ...", cursors)
	}
}

func TestLoopKeepsTheCursorOnABrokenChainAndCounts(t *testing.T) {
	f := newFixture(t, 10, 10)
	src := &fakeSource{t: t, fix: f}
	cfg := Config{Interval: time.Hour, Now: func() time.Time { return epoch }}
	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	through := src.state.ThroughSeq

	// An incremental pass resumes past row 8, so the weekly full pass is what
	// finds a change inside the range verified earlier.
	f.rows[7].Outcome = "denied"
	src.state.LastFullAt = time.Time{}
	before := metricValue(t, "audit_core_chain_verify_failures_total")
	cfg.Now = func() time.Time { return epoch.Add(time.Hour) }
	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	if src.state.Status != StatusBroken || src.state.FirstBrokenSeq != 8 || src.state.ThroughSeq != through {
		t.Fatalf("state = %+v, want broken at 8 with the cursor unchanged at %d", src.state, through)
	}
	if after := metricValue(t, "audit_core_chain_verify_failures_total"); after != before+1 {
		t.Fatalf("failures counter %v -> %v, want +1", before, after)
	}
}

func TestLoopErrorLeavesTheLastVerifyTimestampAlone(t *testing.T) {
	f := newFixture(t, 4, 4)
	src := &fakeSource{t: t, fix: f}
	cfg := Config{Interval: time.Hour, Now: func() time.Time { return epoch.Add(5 * time.Hour) }}
	registerLastVerify()
	lastVerify.Set(123)
	src.failOn = errors.New("database down")
	if err := Pass(context.Background(), src, cfg); err == nil {
		t.Fatal("a pass that could not verify an org must report an error")
	}
	if got := metricValue(t, "audit_core_chain_last_verify_timestamp_seconds"); got != 123 {
		t.Fatalf("last verify timestamp moved to %v after a failed pass", got)
	}

	src.failOn = nil
	if err := Pass(context.Background(), src, cfg); err != nil {
		t.Fatal(err)
	}
	if got := metricValue(t, "audit_core_chain_last_verify_timestamp_seconds"); got != float64(cfg.Now().Unix()) {
		t.Fatalf("last verify timestamp = %v after a clean pass", got)
	}
}

// metricValue reads an unlabelled counter or gauge from the default registry.
func metricValue(t *testing.T, name string) float64 {
	t.Helper()
	families, err := prometheus.DefaultGatherer.Gather()
	if err != nil {
		t.Fatal(err)
	}
	for _, family := range families {
		if family.GetName() != name || len(family.GetMetric()) == 0 {
			continue
		}
		m := family.GetMetric()[0]
		if m.GetCounter() != nil {
			return m.GetCounter().GetValue()
		}
		return m.GetGauge().GetValue()
	}
	t.Fatalf("metric %s is not registered", name)
	return 0
}
