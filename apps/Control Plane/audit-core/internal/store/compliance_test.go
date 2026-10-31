package store_test

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// manifestTestSigner signs the manifest payload with a fixed Ed25519 key, the
// way chain.KeyRing is expected to once it exposes SignPayload.
type manifestTestSigner struct {
	id  string
	key ed25519.PrivateKey
	err error
}

func newManifestTestSigner(seed byte) manifestTestSigner {
	return manifestTestSigner{id: "audit-test-a", key: ed25519.NewKeyFromSeed(seedOf(seed))}
}

func (s manifestTestSigner) SignPayload(payload []byte) (string, []byte, error) {
	if s.err != nil {
		return "", nil, s.err
	}
	return s.id, ed25519.Sign(s.key, payload), nil
}

func (s manifestTestSigner) public() ed25519.PublicKey { return s.key.Public().(ed25519.PublicKey) }

func sampleManifest() store.ComplianceManifest {
	return store.ComplianceManifest{
		OrgID:         "org-1",
		GeneratedAt:   time.Date(2026, 10, 24, 12, 0, 0, 123456000, time.UTC),
		FirstSeq:      4,
		LastSeq:       9,
		Count:         6,
		FirstPrevHash: bytes.Repeat([]byte{0x11}, chain.HashSize),
		LastRowHash:   bytes.Repeat([]byte{0x22}, chain.HashSize),
		HeadSeq:       12,
		HeadHash:      bytes.Repeat([]byte{0x33}, chain.HashSize),
	}
}

func TestManifestPayloadIsTheDocumentedByteString(t *testing.T) {
	payload, err := store.ManifestPayload(sampleManifest())
	if err != nil {
		t.Fatal(err)
	}
	want := "VAM1\norg-1\n4\n9\n6\n" +
		strings.Repeat("11", 32) + "\n" + strings.Repeat("22", 32) + "\n12\n" + strings.Repeat("33", 32) +
		"\n0\n2026-10-24T12:00:00.123456Z"
	if string(payload) != want {
		t.Fatalf("payload = %q\nwant      %q", payload, want)
	}
}

func TestManifestPayloadCommitsToEveryClaimedField(t *testing.T) {
	base, err := store.ManifestPayload(sampleManifest())
	if err != nil {
		t.Fatal(err)
	}
	mutations := map[string]func(*store.ComplianceManifest){
		"org":     func(m *store.ComplianceManifest) { m.OrgID = "org-2" },
		"first":   func(m *store.ComplianceManifest) { m.FirstSeq++ },
		"last":    func(m *store.ComplianceManifest) { m.LastSeq++ },
		"count":   func(m *store.ComplianceManifest) { m.Count++ },
		"prev":    func(m *store.ComplianceManifest) { m.FirstPrevHash[0] ^= 1 },
		"hash":    func(m *store.ComplianceManifest) { m.LastRowHash[0] ^= 1 },
		"head":    func(m *store.ComplianceManifest) { m.HeadSeq++ },
		"headh":   func(m *store.ComplianceManifest) { m.HeadHash[0] ^= 1 },
		"legacy":  func(m *store.ComplianceManifest) { m.LegacyUnchainedRows++ },
		"signed":  func(m *store.ComplianceManifest) { m.GeneratedAt = m.GeneratedAt.Add(time.Microsecond) },
		"swapped": func(m *store.ComplianceManifest) { m.FirstSeq, m.LastSeq = m.LastSeq, m.FirstSeq },
	}
	for name, mutate := range mutations {
		m := sampleManifest()
		mutate(&m)
		got, err := store.ManifestPayload(m)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if bytes.Equal(got, base) {
			t.Fatalf("%s: the payload does not change when the field does", name)
		}
	}
}

func TestManifestPayloadRejectsWhatWouldBeAmbiguous(t *testing.T) {
	m := sampleManifest()
	m.OrgID = "org\n1"
	if _, err := store.ManifestPayload(m); !errors.Is(err, chain.ErrOrgNewline) {
		t.Fatalf("org with a newline: err = %v", err)
	}
	m = sampleManifest()
	m.OrgID = ""
	if _, err := store.ManifestPayload(m); !errors.Is(err, chain.ErrOrgRequired) {
		t.Fatalf("empty org: err = %v", err)
	}
	m = sampleManifest()
	m.LastRowHash = m.LastRowHash[:5]
	if _, err := store.ManifestPayload(m); !errors.Is(err, chain.ErrBadHash) {
		t.Fatalf("short hash: err = %v", err)
	}
	empty := store.ComplianceManifest{OrgID: "org-1", GeneratedAt: time.Now()}
	if _, err := store.ManifestPayload(empty); err != nil {
		t.Fatalf("an empty range has no hashes and must still be signable: %v", err)
	}
}

func TestSignManifestSignatureVerifiesAndTamperingDoesNot(t *testing.T) {
	signer := newManifestTestSigner(1)
	m := sampleManifest()
	if err := store.SignManifest(&m, signer); err != nil {
		t.Fatal(err)
	}
	if m.KeyID != "audit-test-a" || len(m.Signature) != ed25519.SignatureSize {
		t.Fatalf("manifest not signed: key=%q signature=%d bytes", m.KeyID, len(m.Signature))
	}
	payload, err := store.ManifestPayload(m)
	if err != nil {
		t.Fatal(err)
	}
	if !ed25519.Verify(signer.public(), payload, m.Signature) {
		t.Fatal("signature does not verify over the manifest payload")
	}
	m.Count++
	tampered, _ := store.ManifestPayload(m)
	if ed25519.Verify(signer.public(), tampered, m.Signature) {
		t.Fatal("a manifest with a changed count still verifies")
	}
}

func TestSignManifestWithoutAKeyLeavesItUnsignedInsteadOfFailing(t *testing.T) {
	m := sampleManifest()
	if err := store.SignManifest(&m, nil); err != nil || m.Signature != nil {
		t.Fatalf("nil signer: err=%v signature=%x", err, m.Signature)
	}
	if err := store.SignManifest(&m, manifestTestSigner{err: chain.ErrNoSigningKey}); err != nil || m.Signature != nil || m.KeyID != "" {
		t.Fatalf("verify-only ring: err=%v key=%q signature=%x", err, m.KeyID, m.Signature)
	}
	boom := errors.New("hsm unreachable")
	if err := store.SignManifest(&m, manifestTestSigner{err: boom}); !errors.Is(err, boom) {
		t.Fatalf("a real signing failure must surface, got %v", err)
	}
}

func chainRowOf(e store.ComplianceEvent) chain.Row {
	return chain.Row{
		OrgID: e.OrgID, ChainSeq: e.ChainSeq, OccurredAt: e.OccurredAt, IngestedAt: e.IngestedAt,
		UserID: e.UserID, ActorRole: e.ActorRole, Plane: e.Plane, Event: e.Event, Subject: e.Subject,
		ResourceID: e.ResourceID, Outcome: e.Outcome, EventID: e.EventID, RequestID: e.RequestID,
		Host: e.IPAddress, UserAgent: e.UserAgent, SourceProducer: e.SourceProducer, SourceBus: e.SourceBus,
		PayloadHash: e.PayloadHash, Details: []byte(e.Details),
	}
}

// pageAll follows the cursor to the end of the range.
func pageAll(t *testing.T, env verifyEnv, f store.ComplianceFilter) []store.ComplianceEvent {
	t.Helper()
	var all []store.ComplianceEvent
	for {
		page, err := env.st.ComplianceEvents(env.ctx, f)
		if err != nil {
			t.Fatalf("ComplianceEvents: %v", err)
		}
		all = append(all, page.Events...)
		if page.NextSeq == 0 {
			return all
		}
		f.AfterSeq = page.NextSeq
	}
}

func TestComplianceEventsPagesAreContiguousAndRecomputeToTheirHashes(t *testing.T) {
	env := newAttestedChain(t, "compl-pages")
	all := pageAll(t, env, store.ComplianceFilter{OrgID: env.org, Limit: 3})
	if len(all) != 10 {
		t.Fatalf("paged %d rows, want 10", len(all))
	}
	prev, err := chain.GenesisHash(env.org)
	if err != nil {
		t.Fatal(err)
	}
	for i, e := range all {
		if e.ChainSeq != int64(i+1) {
			t.Fatalf("row %d has chain_seq %d: pages skipped or repeated a row", i, e.ChainSeq)
		}
		if !bytes.Equal(e.PrevHash, prev) {
			t.Fatalf("seq %d: prev_hash does not link", e.ChainSeq)
		}
		want, err := chain.RowHash(prev, chainRowOf(e))
		if err != nil || !bytes.Equal(want, e.RowHash) {
			t.Fatalf("seq %d: the exported fields do not recompute to row_hash (err %v)", e.ChainSeq, err)
		}
		prev = e.RowHash
	}
}

func TestComplianceEventsCursorIsStableUnderConcurrentInserts(t *testing.T) {
	env := newVerifyEnv(t, "compl-cursor")
	insertChained(t, env.ctx, env.st, env.org, 1, 6)

	first, err := env.st.ComplianceEvents(env.ctx, store.ComplianceFilter{OrgID: env.org, Limit: 3})
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Events) != 3 || first.NextSeq != 3 || first.HeadSeq != 6 {
		t.Fatalf("first page: %d rows next=%d head=%d", len(first.Events), first.NextSeq, first.HeadSeq)
	}
	insertChained(t, env.ctx, env.st, env.org, 7, 10)

	rest := pageAll(t, env, store.ComplianceFilter{OrgID: env.org, Limit: 3, AfterSeq: first.NextSeq})
	for i, e := range rest {
		if e.ChainSeq != int64(4+i) {
			t.Fatalf("after concurrent inserts the continuation has seq %d at position %d", e.ChainSeq, i)
		}
	}
	if len(rest) != 7 {
		t.Fatalf("continuation returned %d rows, want 7 (4..10)", len(rest))
	}
}

func TestComplianceEventsWindowResolvesToOneContiguousSeqRange(t *testing.T) {
	env := newAttestedChain(t, "compl-window")
	all := pageAll(t, env, store.ComplianceFilter{OrgID: env.org})

	got := pageAll(t, env, store.ComplianceFilter{
		OrgID: env.org, Since: all[4].IngestedAt, Until: all[6].IngestedAt,
	})
	if len(got) != 3 || got[0].ChainSeq != 5 || got[2].ChainSeq != 7 {
		t.Fatalf("window rows 5..7 returned %d rows starting at %d", len(got), firstSeq(got))
	}
	if none := pageAll(t, env, store.ComplianceFilter{OrgID: env.org, Since: time.Now().Add(time.Hour)}); len(none) != 0 {
		t.Fatalf("a window after the last row returned %d rows", len(none))
	}
	if none := pageAll(t, env, store.ComplianceFilter{OrgID: env.org, Until: all[0].IngestedAt.Add(-time.Hour)}); len(none) != 0 {
		t.Fatalf("a window before the first row returned %d rows", len(none))
	}
}

func firstSeq(rows []store.ComplianceEvent) int64 {
	if len(rows) == 0 {
		return 0
	}
	return rows[0].ChainSeq
}

func TestComplianceEventsFiltersThinTheRangeAndNeverCrossOrgs(t *testing.T) {
	env := newVerifyEnv(t, "compl-filter")
	insertChained(t, env.ctx, env.st, env.org, 1, 4)
	other := chainEvent(env.org, 99)
	other.Event = "approval_decided"
	other.Outcome = "denied"
	if _, err := env.st.InsertAudit(env.ctx, other); err != nil {
		t.Fatal(err)
	}

	got := pageAll(t, env, store.ComplianceFilter{OrgID: env.org, Event: "approval_decided", Outcome: "denied"})
	if len(got) != 1 || got[0].ChainSeq != 5 {
		t.Fatalf("filtered rows = %d (first seq %d), want the one denied approval at seq 5", len(got), firstSeq(got))
	}

	elsewhere := verifyOrg(t, env.pool, "compl-filter-other")
	if rows := pageAll(t, env, store.ComplianceFilter{OrgID: elsewhere}); len(rows) != 0 {
		t.Fatalf("an org with no rows saw %d", len(rows))
	}
}

func TestComplianceManifestDescribesTheRangeAndIsSigned(t *testing.T) {
	env := newAttestedChain(t, "compl-manifest")
	signer := newManifestTestSigner(1)
	st := store.New(env.pool, store.WithChain(), store.WithChainKeys(env.ring), store.WithManifestSigner(signer))

	// One row written while the chain switch was off stays unchained and is
	// counted as legacy, never hidden.
	if _, err := store.New(env.pool).InsertAudit(env.ctx, chainEvent(env.org, 77)); err != nil {
		t.Fatal(err)
	}

	m, err := st.ComplianceManifest(env.ctx, env.org, time.Time{}, time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	genesis, _ := chain.GenesisHash(env.org)
	rows := loadChain(t, env.ctx, env.pool, env.org)
	if m.FirstSeq != 1 || m.LastSeq != 10 || m.Count != 10 || m.HeadSeq != 10 {
		t.Fatalf("manifest range = %d..%d count %d head %d", m.FirstSeq, m.LastSeq, m.Count, m.HeadSeq)
	}
	if !bytes.Equal(m.FirstPrevHash, genesis) || !bytes.Equal(m.LastRowHash, rows[9].hash) || !bytes.Equal(m.HeadHash, rows[9].hash) {
		t.Fatal("manifest hashes do not match the stored first link and last row")
	}
	if m.LegacyUnchainedRows != 1 {
		t.Fatalf("legacy rows = %d, want 1", m.LegacyUnchainedRows)
	}
	if m.Attestation == nil || m.Attestation.Seq != 10 || m.Attestation.KeyID != "audit-test-a" {
		t.Fatalf("attestation = %+v, want the one signed at seq 10", m.Attestation)
	}
	if m.Checkpoint != nil {
		t.Fatalf("an unpruned chain has no checkpoint, got %+v", m.Checkpoint)
	}
	payload, err := store.ManifestPayload(m)
	if err != nil {
		t.Fatal(err)
	}
	if !ed25519.Verify(signer.public(), payload, m.Signature) {
		t.Fatal("the manifest signature does not verify")
	}

	sub, err := st.ComplianceManifest(env.ctx, env.org, rows[3].row.IngestedAt, rows[5].row.IngestedAt)
	if err != nil {
		t.Fatal(err)
	}
	if sub.FirstSeq != 4 || sub.LastSeq != 6 || sub.Count != 3 || !bytes.Equal(sub.FirstPrevHash, rows[2].hash) {
		t.Fatalf("sub-range manifest = %d..%d count %d", sub.FirstSeq, sub.LastSeq, sub.Count)
	}
	if sub.HeadSeq != 10 {
		t.Fatalf("a sub-range manifest still names the real head, got %d", sub.HeadSeq)
	}
}

func TestComplianceKeysListActiveAndRetiredAndFlagUnavailable(t *testing.T) {
	env := newAttestedChain(t, "compl-keys") // attestations signed by audit-test-a

	keys, err := env.st.ComplianceKeys(env.ctx, env.org)
	if err != nil {
		t.Fatal(err)
	}
	if len(keys) != 1 || keys[0].KeyID != "audit-test-a" || keys[0].Status != store.KeyStatusActive ||
		keys[0].PublicKey != publicOf(1) || keys[0].Algorithm != "ed25519" {
		t.Fatalf("active key listing = %+v", keys)
	}

	rotated := pruneRing(t, "audit-test-b", 2, "audit-test-a:"+publicOf(1))
	keys, err = store.New(env.pool, store.WithChain(), store.WithChainKeys(rotated)).ComplianceKeys(env.ctx, env.org)
	if err != nil {
		t.Fatal(err)
	}
	if len(keys) != 2 || keys[0].KeyID != "audit-test-a" || keys[0].Status != store.KeyStatusRetired ||
		keys[1].KeyID != "audit-test-b" || keys[1].Status != store.KeyStatusActive {
		t.Fatalf("after rotation = %+v", keys)
	}

	noKey, err := store.New(env.pool, store.WithChain()).ComplianceKeys(env.ctx, env.org)
	if err != nil {
		t.Fatal(err)
	}
	if len(noKey) != 1 || noKey[0].Status != store.KeyStatusUnavailable || noKey[0].PublicKey != "" {
		t.Fatalf("a server without the key must say the key is unavailable: %+v", noKey)
	}
	if decoded, err := base64.RawURLEncoding.DecodeString(keys[1].PublicKey); err != nil || len(decoded) != ed25519.PublicKeySize {
		t.Fatalf("public key is not base64url Ed25519: %v", err)
	}
}
