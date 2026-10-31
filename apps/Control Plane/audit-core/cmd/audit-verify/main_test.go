package main

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// The offline verifier is tested against the shared golden vectors: the
// chain in testdata/vectors.json was hashed by an independent Python
// reference, so "verified" here means this tool agrees with it, not just with
// internal/chain.

const vectorsPath = "../../internal/chain/testdata/vectors.json"

type vectorChainRow struct {
	PrevHash string `json:"prev_hash"`
	Row      struct {
		OrgID            string `json:"org_id"`
		ChainSeq         int64  `json:"chain_seq"`
		OccurredAtMicros int64  `json:"occurred_at_micros"`
		IngestedAtMicros int64  `json:"ingested_at_micros"`
		UserID           string `json:"user_id"`
		ActorRole        string `json:"actor_role"`
		Plane            string `json:"plane"`
		Event            string `json:"event"`
		Subject          string `json:"subject"`
		ResourceID       string `json:"resource_id"`
		Outcome          string `json:"outcome"`
		EventID          string `json:"event_id"`
		RequestID        string `json:"request_id"`
		Host             string `json:"host"`
		UserAgent        string `json:"user_agent"`
		SourceProducer   string `json:"source_producer"`
		SourceBus        string `json:"source_bus"`
		PayloadHash      string `json:"payload_hash"`
		Details          string `json:"details"`
	} `json:"row"`
	RowHash string `json:"row_hash"`
}

func loadVectorChain(t *testing.T) []exportRow {
	t.Helper()
	raw, err := os.ReadFile(vectorsPath)
	if err != nil {
		t.Fatal(err)
	}
	var v struct {
		Chain []vectorChainRow `json:"chain"`
	}
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatal(err)
	}
	if len(v.Chain) < 3 {
		t.Fatalf("vectors chain has %d rows, want at least 3", len(v.Chain))
	}
	var rows []exportRow
	for _, c := range v.Chain {
		r := c.Row
		var payload, prev, hash verifier.Hash
		for _, p := range []struct {
			dst *verifier.Hash
			src string
		}{{&payload, r.PayloadHash}, {&prev, c.PrevHash}, {&hash, c.RowHash}} {
			if err := p.dst.UnmarshalText([]byte(p.src)); err != nil {
				t.Fatal(err)
			}
		}
		rows = append(rows, exportRow{
			OrgID: r.OrgID, ChainSeq: r.ChainSeq,
			OccurredAt: time.UnixMicro(r.OccurredAtMicros).UTC(), IngestedAt: time.UnixMicro(r.IngestedAtMicros).UTC(),
			UserID: r.UserID, ActorRole: r.ActorRole, Plane: r.Plane, Event: r.Event, Subject: r.Subject,
			ResourceID: r.ResourceID, Outcome: r.Outcome, EventID: r.EventID, RequestID: r.RequestID,
			IPAddress: r.Host, UserAgent: r.UserAgent, SourceProducer: r.SourceProducer, SourceBus: r.SourceBus,
			PayloadHash: payload, Details: json.RawMessage(r.Details), ChainVersion: chain.Version,
			PrevHash: prev, RowHash: hash,
		})
	}
	return rows
}

func seed(b byte) []byte {
	s := make([]byte, ed25519.SeedSize)
	for i := range s {
		s[i] = b + byte(i)
	}
	return s
}

func ring(t *testing.T, keyID string, b byte) *chain.KeyRing {
	t.Helper()
	r, err := chain.NewKeyRing(chain.KeyConfig{KeyID: keyID, Private: base64.RawURLEncoding.EncodeToString(seed(b))})
	if err != nil {
		t.Fatal(err)
	}
	return r
}

func publicKey(b byte) string {
	return base64.RawURLEncoding.EncodeToString(ed25519.NewKeyFromSeed(seed(b)).Public().(ed25519.PublicKey))
}

// export is one org's exported pages and manifest on disk.
type export struct {
	dir      string
	rows     []exportRow
	manifest manifest
}

func newExport(t *testing.T) *export {
	t.Helper()
	rows := loadVectorChain(t)
	last := rows[len(rows)-1]
	signedAt := time.Date(2026, 10, 2, 9, 0, 0, 123456000, time.UTC)
	keyID, sig, err := ring(t, "audit-key", 1).SignAttestation(last.OrgID, last.ChainSeq, last.RowHash, signedAt)
	if err != nil {
		t.Fatal(err)
	}
	return &export{
		dir:  t.TempDir(),
		rows: rows,
		manifest: manifest{
			OrgID: last.OrgID, LastSeq: last.ChainSeq, LastRowHash: last.RowHash,
			Attestation: &verifier.Attestation{Seq: last.ChainSeq, Hash: last.RowHash, KeyID: keyID, Signature: sig, SignedAt: signedAt},
		},
	}
}

func writeJSON(t *testing.T, path string, v any) {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatal(err)
	}
}

// pages writes the rows as two pages (an API envelope and a bare array) and
// the manifest, and returns the file arguments.
func (e *export) pages(t *testing.T) (manifestPath string, pagePaths []string) {
	t.Helper()
	split := 1
	first := filepath.Join(e.dir, "page-1.json")
	second := filepath.Join(e.dir, "page-2.json")
	writeJSON(t, first, map[string]any{"data": e.rows[:split], "meta": map[string]any{"count": split}, "error": nil})
	writeJSON(t, second, e.rows[split:])
	manifestPath = filepath.Join(e.dir, "manifest.json")
	writeJSON(t, manifestPath, e.manifest)
	return manifestPath, []string{first, second}
}

func (e *export) run(t *testing.T, extra ...string) (code int, stdout, stderr string) {
	t.Helper()
	manifestPath, pagePaths := e.pages(t)
	args := append([]string{"-manifest", manifestPath, "-key-id", "audit-key", "-public-key", publicKey(1)}, extra...)
	args = append(args, pagePaths...)
	var out, errOut bytes.Buffer
	code = run(args, &out, &errOut)
	return code, out.String(), errOut.String()
}

func reportOf(t *testing.T, stdout string) verifier.Report {
	t.Helper()
	var r verifier.Report
	if err := json.Unmarshal([]byte(stdout), &r); err != nil {
		t.Fatalf("report is not JSON: %v\n%s", err, stdout)
	}
	return r
}

func TestVerifiesTheGoldenVectorChain(t *testing.T) {
	code, out, errOut := newExport(t).run(t)
	if code != exitVerified {
		t.Fatalf("exit = %d, want 0\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	for _, want := range []string{"status: verified", "rows verified: 3", "signature valid: true"} {
		if !strings.Contains(out, want) {
			t.Fatalf("output lacks %q:\n%s", want, out)
		}
	}
}

func TestJSONReportCarriesTheStatusAnchorsAndNeverABareOK(t *testing.T) {
	code, out, _ := newExport(t).run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d", code)
	}
	r := reportOf(t, out)
	if r.Status != verifier.StatusVerified || r.RowsVerified != 3 || r.Head == nil || r.Head.Seq != 3 || r.Attested == nil || !r.Attested.Verified {
		t.Fatalf("report = %+v", r)
	}
	if strings.Contains(strings.ToLower(out), `"ok"`) {
		t.Fatalf("a report must name its status, not say ok:\n%s", out)
	}
}

func TestLegacyRowsFromTheManifestAreReported(t *testing.T) {
	e := newExport(t)
	e.manifest.LegacyUnchainedRows = 4
	code, out, _ := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d, want 0 (verified_with_legacy is still verified)", code)
	}
	if r := reportOf(t, out); r.Status != verifier.StatusVerifiedWithLegacy || r.LegacyUnchainedRows != 4 {
		t.Fatalf("report = %+v", r)
	}
}

func TestTamperedRowIsBrokenWithItsSequence(t *testing.T) {
	e := newExport(t)
	e.rows[1].Outcome = "denied"
	code, out, _ := e.run(t, "-json")
	if code != exitBroken {
		t.Fatalf("exit = %d, want 2", code)
	}
	if r := reportOf(t, out); r.Status != verifier.StatusBroken || r.FirstBrokenSeq != 2 || r.Reason != verifier.ReasonRowHashMismatch {
		t.Fatalf("report = %+v", r)
	}
}

func TestEditedDetailsAreBroken(t *testing.T) {
	e := newExport(t)
	e.rows[0].Details = json.RawMessage(`{"seq":99}`)
	code, out, _ := e.run(t, "-json")
	if code != exitBroken || reportOf(t, out).FirstBrokenSeq != 1 {
		t.Fatalf("exit = %d report = %s", code, out)
	}
}

func TestMissingMiddleRowIsBroken(t *testing.T) {
	e := newExport(t)
	e.rows = []exportRow{e.rows[0], e.rows[2]}
	code, out, _ := e.run(t, "-json")
	if r := reportOf(t, out); code != exitBroken || r.FirstBrokenSeq != 2 || r.Reason != verifier.ReasonSequenceGap {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestTruncatedExportIsBrokenAgainstTheManifestHead(t *testing.T) {
	e := newExport(t)
	e.rows = e.rows[:2]
	code, out, _ := e.run(t, "-json")
	if r := reportOf(t, out); code != exitBroken || r.FirstBrokenSeq != 3 || r.Reason != verifier.ReasonTailMissing {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestPagesInAnyOrderAreSortedByChainSequence(t *testing.T) {
	e := newExport(t)
	e.rows[0], e.rows[2] = e.rows[2], e.rows[0]
	if code, out, errOut := e.run(t); code != exitVerified {
		t.Fatalf("exit = %d\n%s\n%s", code, out, errOut)
	}
}

func TestAttestationSignedWithAnotherKeyIsBroken(t *testing.T) {
	e := newExport(t)
	a := e.manifest.Attestation
	_, sig, err := ring(t, "audit-key", 9).SignAttestation(e.manifest.OrgID, a.Seq, a.Hash, a.SignedAt)
	if err != nil {
		t.Fatal(err)
	}
	a.Signature = sig
	code, out, _ := e.run(t, "-json")
	if r := reportOf(t, out); code != exitBroken || r.Reason != verifier.ReasonAttestationSignature {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestRewrittenChainContradictsTheAttestation(t *testing.T) {
	e := newExport(t)
	// An attacker without the key rewrites row 2 and recomputes the tail.
	prev := []byte(e.rows[1].PrevHash)
	e.rows[1].Outcome = "denied"
	for i := 1; i < len(e.rows); i++ {
		e.rows[i].PrevHash = prev
		hash, err := chain.RowHash(prev, e.rows[i].stored().Row)
		if err != nil {
			t.Fatal(err)
		}
		e.rows[i].RowHash, prev = hash, hash
	}
	e.manifest.LastRowHash = e.rows[2].RowHash
	code, out, _ := e.run(t, "-json")
	if r := reportOf(t, out); code != exitBroken || r.FirstBrokenSeq != 3 || r.Reason != verifier.ReasonAttestationMismatch {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestWithoutAKeyTheChainIsUnverifiableNotVerified(t *testing.T) {
	e := newExport(t)
	manifestPath, pagePaths := e.pages(t)
	var out, errOut bytes.Buffer
	code := run(append([]string{"-manifest", manifestPath, "-json"}, pagePaths...), &out, &errOut)
	if code != exitUnverifiable {
		t.Fatalf("exit = %d, want 3", code)
	}
	if r := reportOf(t, out.String()); r.Status != verifier.StatusKeyUnavailable {
		t.Fatalf("report = %+v", r)
	}
}

func TestWithoutAManifestTheExportIsPartial(t *testing.T) {
	e := newExport(t)
	_, pagePaths := e.pages(t)
	var out, errOut bytes.Buffer
	code := run(append([]string{"-key-id", "audit-key", "-public-key", publicKey(1), "-json"}, pagePaths...), &out, &errOut)
	if code != exitUnverifiable {
		t.Fatalf("exit = %d, want 3", code)
	}
	if r := reportOf(t, out.String()); r.Status != verifier.StatusPartial || r.Reason != verifier.ReasonNoHead {
		t.Fatalf("report = %+v", r)
	}
}

func TestPrunedExportStartsAtTheSignedCheckpoint(t *testing.T) {
	e := newExport(t)
	first := e.rows[0]
	createdAt := time.Date(2026, 10, 2, 8, 0, 0, 1000, time.UTC)
	keyID, sig, err := ring(t, "audit-key", 1).SignCheckpoint(first.OrgID, first.ChainSeq, first.RowHash, 1, createdAt)
	if err != nil {
		t.Fatal(err)
	}
	e.manifest.Checkpoint = &verifier.Checkpoint{Seq: first.ChainSeq, Hash: first.RowHash, PrunedTotal: 1, KeyID: keyID, Signature: sig, CreatedAt: createdAt}

	// The export still lists the pruned row 1; the checkpoint covers it.
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\n%s\n%s", code, out, errOut)
	}
	r := reportOf(t, out)
	if r.FromSeq != 2 || r.RowsVerified != 2 || r.Checkpoint == nil || !r.Checkpoint.Verified {
		t.Fatalf("report = %+v", r)
	}
	if !strings.Contains(errOut, "ignoring 1 rows") {
		t.Fatalf("stderr = %q, want a note about the covered row", errOut)
	}

	// Without the checkpoint the same rows are a chain that starts mid-way:
	// internally consistent, but nothing anchors where it starts.
	e = newExport(t)
	e.rows = e.rows[1:]
	code, out, _ = e.run(t, "-json")
	if r := reportOf(t, out); code != exitUnverifiable || r.Status != verifier.StatusPartial || r.Reason != verifier.ReasonUnanchored {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestUnreadableOrMalformedInputIsUnverifiable(t *testing.T) {
	dir := t.TempDir()
	bad := filepath.Join(dir, "bad.json")
	if err := os.WriteFile(bad, []byte(`{"data": [`), 0o600); err != nil {
		t.Fatal(err)
	}
	empty := filepath.Join(dir, "empty.json")
	if err := os.WriteFile(empty, []byte(`{"data": []}`), 0o600); err != nil {
		t.Fatal(err)
	}
	for name, args := range map[string][]string{
		"missing file":   {filepath.Join(dir, "nope.json")},
		"malformed json": {bad},
		"no events":      {empty},
	} {
		var out, errOut bytes.Buffer
		if code := run(args, &out, &errOut); code != exitUnverifiable {
			t.Errorf("%s: exit = %d, want 3 (stderr %q)", name, code, errOut.String())
		}
	}
}

func TestUsageErrors(t *testing.T) {
	var out, errOut bytes.Buffer
	if code := run(nil, &out, &errOut); code != exitUsage {
		t.Errorf("no pages: exit = %d, want 1", code)
	}
	if code := run([]string{"-key-id", "k", "-public-key", "not base64 !!", "page.json"}, &out, &errOut); code != exitUsage {
		t.Errorf("bad public key: exit = %d, want 1", code)
	}
	if code := run([]string{"-nonsense"}, &out, &errOut); code != exitUsage {
		t.Errorf("unknown flag: exit = %d, want 1", code)
	}
}

func TestManifestOfAnotherOrgIsRefused(t *testing.T) {
	e := newExport(t)
	e.manifest.OrgID = "someone-else"
	if code, _, errOut := e.run(t); code != exitUnverifiable || !strings.Contains(errOut, "different org") {
		t.Fatalf("exit = %d stderr = %q", code, errOut)
	}
}

// manifestOf reads the manifest part of a JSON report.
func manifestOf(t *testing.T, stdout string) manifestStatus {
	t.Helper()
	var j jsonReport
	if err := json.Unmarshal([]byte(stdout), &j); err != nil {
		t.Fatalf("report is not JSON: %v\n%s", err, stdout)
	}
	return j.Manifest
}

// signManifest makes the export's manifest the one the server writes for the
// rows it holds: their range, and the chain head of the whole chain (head).
// store.SignManifest signs it as the server does, with key b under keyID.
func (e *export) signManifest(t *testing.T, head exportRow, keyID string, b byte, count int64) {
	t.Helper()
	first, last := e.rows[0], e.rows[len(e.rows)-1]
	m := store.ComplianceManifest{
		OrgID:               head.OrgID,
		GeneratedAt:         time.Date(2026, 10, 2, 10, 0, 0, 654321000, time.UTC),
		FirstSeq:            first.ChainSeq,
		LastSeq:             last.ChainSeq,
		Count:               count,
		FirstPrevHash:       first.PrevHash,
		LastRowHash:         last.RowHash,
		HeadSeq:             head.ChainSeq,
		HeadHash:            head.RowHash,
		LegacyUnchainedRows: e.manifest.LegacyUnchainedRows,
		Checkpoint:          e.manifest.Checkpoint,
		Attestation:         e.manifest.Attestation,
	}
	if err := store.SignManifest(&m, ring(t, keyID, b)); err != nil {
		t.Fatal(err)
	}
	e.manifest = manifest{
		OrgID:               m.OrgID,
		GeneratedAt:         m.GeneratedAt,
		FirstSeq:            m.FirstSeq,
		LastSeq:             m.LastSeq,
		Count:               m.Count,
		FirstPrevHash:       m.FirstPrevHash,
		LastRowHash:         m.LastRowHash,
		HeadSeq:             m.HeadSeq,
		HeadHash:            m.HeadHash,
		LegacyUnchainedRows: m.LegacyUnchainedRows,
		Checkpoint:          m.Checkpoint,
		Attestation:         m.Attestation,
		KeyID:               m.KeyID,
		Signature:           m.Signature,
	}
}

// subRangeExport exports rows [from, to) of the golden chain, whose head is its
// last row, with the manifest the server signs for that range.
func subRangeExport(t *testing.T, from, to int) *export {
	t.Helper()
	e := newExport(t)
	head := e.rows[len(e.rows)-1]
	e.rows = e.rows[from:to]
	e.signManifest(t, head, "audit-key", 1, int64(to-from))
	return e
}

func TestSignedManifestOfTheWholeChainVerifies(t *testing.T) {
	e := newExport(t)
	e.signManifest(t, e.rows[len(e.rows)-1], "audit-key", 1, 3)
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	if m := manifestOf(t, out); !m.Signed || m.KeyID != "audit-key" || m.SubRange {
		t.Fatalf("manifest = %+v", m)
	}
	if r := reportOf(t, out); r.Status != verifier.StatusVerified || r.Head == nil || r.Head.Seq != 3 {
		t.Fatalf("report = %+v", r)
	}
	_, text, _ := e.run(t)
	if !strings.Contains(text, "manifest: signed, key audit-key, signature valid: true") {
		t.Fatalf("output lacks the signature line:\n%s", text)
	}
}

func TestUnsignedManifestIsSaidToBeUnsigned(t *testing.T) {
	code, out, errOut := newExport(t).run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstderr: %s", code, errOut)
	}
	if m := manifestOf(t, out); !m.Supplied || m.Signed || m.SubRange {
		t.Fatalf("manifest = %+v", m)
	}
	if _, text, _ := newExport(t).run(t); !strings.Contains(text, "manifest: unsigned") {
		t.Fatalf("output lacks manifest: unsigned:\n%s", text)
	}
}

func TestTamperingWithASignedManifestIsBroken(t *testing.T) {
	tampers := map[string]func(e *export){
		"last_seq":        func(e *export) { e.manifest.LastSeq = 2 },
		"legacy count":    func(e *export) { e.manifest.LegacyUnchainedRows = 4 },
		"count":           func(e *export) { e.manifest.Count = 2 },
		"generated_at":    func(e *export) { e.manifest.GeneratedAt = e.manifest.GeneratedAt.Add(time.Second) },
		"first_seq":       func(e *export) { e.manifest.FirstSeq = 2 },
		"first_prev_hash": func(e *export) { e.manifest.FirstPrevHash = bytes.Repeat([]byte{0xAA}, chain.HashSize) },
		"head_seq":        func(e *export) { e.manifest.HeadSeq = 4 },
		"head_hash":       func(e *export) { e.manifest.HeadHash = bytes.Repeat([]byte{0xBB}, chain.HashSize) },
	}
	for name, tamper := range tampers {
		t.Run(name, func(t *testing.T) {
			e := newExport(t)
			e.signManifest(t, e.rows[len(e.rows)-1], "audit-key", 1, 3)
			tamper(e)
			code, out, errOut := e.run(t)
			if code != exitBroken || !strings.Contains(errOut, "signature does not verify") {
				t.Fatalf("exit = %d, want 2 with a signature complaint\nstdout: %s\nstderr: %s", code, out, errOut)
			}
		})
	}
}

func TestSignedManifestUnderAKeyTheVerifierLacksIsUnverifiable(t *testing.T) {
	e := newExport(t)
	e.signManifest(t, e.rows[len(e.rows)-1], "audit-key", 1, 3)
	e.manifest.KeyID = "retired-key"
	code, out, errOut := e.run(t)
	if code != exitUnverifiable || !strings.Contains(errOut, `"retired-key"`) {
		t.Fatalf("exit = %d, want 3 naming the key\nstdout: %s\nstderr: %s", code, out, errOut)
	}
}

func TestManifestSignedByAnotherKeyUnderTheSameIDIsBroken(t *testing.T) {
	e := newExport(t)
	e.signManifest(t, e.rows[len(e.rows)-1], "audit-key", 9, 3)
	if code, out, errOut := e.run(t); code != exitBroken {
		t.Fatalf("exit = %d, want 2\nstdout: %s\nstderr: %s", code, out, errOut)
	}
}

func TestSignedSubRangeVerifiesWhereTheFullRangeRulesReportedItBroken(t *testing.T) {
	// The chain head is seq 3 and the export stops at seq 2. The attestation at
	// seq 3 lies above the range, and passing it reported a truncation that is
	// not there.
	e := subRangeExport(t, 0, 2)
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	if r := reportOf(t, out); r.Status != verifier.StatusVerified || r.FromSeq != 1 || r.ThroughSeq != 2 || r.RowsVerified != 2 || r.Attested != nil {
		t.Fatalf("report = %+v", r)
	}
	if m := manifestOf(t, out); !m.Signed || !m.SubRange || m.ChainHeadSeq != 3 {
		t.Fatalf("manifest = %+v", m)
	}
	if _, text, _ := e.run(t); !strings.Contains(text, "range anchored by the signed manifest rather than by an attestation") {
		t.Fatalf("output does not name the anchor:\n%s", text)
	}
}

func TestSignedSubRangeIsAnchoredAtItsSignedFirstLink(t *testing.T) {
	// Only seq 2 is exported. Its start is anchored by the signed first_prev_hash,
	// the hash of seq 1, which the export does not carry as a row.
	e := subRangeExport(t, 1, 2)
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	if r := reportOf(t, out); r.Status != verifier.StatusVerified || r.FromSeq != 2 || r.RowsVerified != 1 {
		t.Fatalf("report = %+v", r)
	}
}

func TestSubRangeWhoseFirstRowIsRelinkedIsBroken(t *testing.T) {
	e := subRangeExport(t, 1, 2)
	// The row is rewritten to follow another prev and re-hashed. It is consistent
	// with itself but not with the signed first link.
	prev := bytes.Repeat([]byte{0xAA}, chain.HashSize)
	hash, err := chain.RowHash(prev, e.rows[0].stored().Row)
	if err != nil {
		t.Fatal(err)
	}
	e.rows[0].PrevHash, e.rows[0].RowHash = prev, hash
	code, out, _ := e.run(t, "-json")
	if r := reportOf(t, out); code != exitBroken || r.Reason != verifier.ReasonResumeAnchor || r.FirstBrokenSeq != 2 {
		t.Fatalf("exit = %d report = %+v", code, r)
	}
}

func TestUnsignedSubRangeIsUnverifiable(t *testing.T) {
	e := subRangeExport(t, 0, 2)
	e.manifest.KeyID, e.manifest.Signature = "", nil
	code, out, errOut := e.run(t)
	if code != exitUnverifiable || !strings.Contains(errOut, "sub-range") {
		t.Fatalf("exit = %d, want 3\nstdout: %s\nstderr: %s", code, out, errOut)
	}
}

func TestSubRangeWhoseSignedCountDisagreesIsBroken(t *testing.T) {
	e := newExport(t)
	head := e.rows[len(e.rows)-1]
	e.rows = e.rows[:2]
	e.signManifest(t, head, "audit-key", 1, 3) // the range holds two rows
	code, out, errOut := e.run(t)
	if code != exitBroken || !strings.Contains(errOut, "counts 3 rows") {
		t.Fatalf("exit = %d, want 2\nstdout: %s\nstderr: %s", code, out, errOut)
	}
}

func TestSubRangeIsAlsoCheckedAgainstAnAttestationInsideIt(t *testing.T) {
	e := newExport(t)
	head := e.rows[len(e.rows)-1]
	e.rows = e.rows[:2]
	inside := e.rows[1]
	signedAt := time.Date(2026, 10, 2, 9, 30, 0, 0, time.UTC)
	keyID, sig, err := ring(t, "audit-key", 1).SignAttestation(inside.OrgID, inside.ChainSeq, inside.RowHash, signedAt)
	if err != nil {
		t.Fatal(err)
	}
	e.manifest.Attestation = &verifier.Attestation{Seq: inside.ChainSeq, Hash: inside.RowHash, KeyID: keyID, Signature: sig, SignedAt: signedAt}
	e.signManifest(t, head, "audit-key", 1, 2)
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	if r := reportOf(t, out); r.Attested == nil || r.Attested.Seq != 2 || !r.Attested.Verified {
		t.Fatalf("report = %+v", r)
	}
	if _, text, _ := e.run(t); !strings.Contains(text, "range anchored by the signed manifest, and by the attestation at seq 2") {
		t.Fatalf("output does not name both anchors:\n%s", text)
	}
}

// checkpointAt gives the export a checkpoint signed over row, as a chain pruned
// through that row would carry.
func (e *export) checkpointAt(t *testing.T, row exportRow) {
	t.Helper()
	createdAt := time.Date(2026, 10, 2, 8, 0, 0, 1000, time.UTC)
	keyID, sig, err := ring(t, "audit-key", 1).SignCheckpoint(row.OrgID, row.ChainSeq, row.RowHash, 1, createdAt)
	if err != nil {
		t.Fatal(err)
	}
	e.manifest.Checkpoint = &verifier.Checkpoint{Seq: row.ChainSeq, Hash: row.RowHash, PrunedTotal: 1, KeyID: keyID, Signature: sig, CreatedAt: createdAt}
}

func TestSubRangeAfterACheckpointStartsAtTheCheckpoint(t *testing.T) {
	// Seq 1 was pruned under a signed checkpoint. The export holds seq 2, which
	// starts right after the checkpoint and is anchored by it.
	e := newExport(t)
	head := e.rows[len(e.rows)-1]
	e.checkpointAt(t, e.rows[0])
	e.rows = e.rows[1:2]
	e.signManifest(t, head, "audit-key", 1, 1)
	code, out, errOut := e.run(t, "-json")
	if code != exitVerified {
		t.Fatalf("exit = %d\nstdout: %s\nstderr: %s", code, out, errOut)
	}
	if r := reportOf(t, out); r.FromSeq != 2 || r.RowsVerified != 1 || r.Checkpoint == nil || !r.Checkpoint.Verified {
		t.Fatalf("report = %+v", r)
	}
}

func TestSubRangeWhoseCheckpointCoversItsStartIsUnverifiable(t *testing.T) {
	// The checkpoint at seq 1 covers the first row of the signed range, so the
	// start of the range cannot be anchored by the manifest.
	e := newExport(t)
	head := e.rows[len(e.rows)-1]
	e.checkpointAt(t, e.rows[0])
	e.rows = e.rows[:2]
	e.signManifest(t, head, "audit-key", 1, 2)
	code, out, errOut := e.run(t)
	if code != exitUnverifiable || !strings.Contains(errOut, "covers the start") {
		t.Fatalf("exit = %d, want 3\nstdout: %s\nstderr: %s", code, out, errOut)
	}
}

func TestOutputCarriesNoRowContent(t *testing.T) {
	e := newExport(t)
	_, out, _ := e.run(t, "-json")
	for _, r := range e.rows {
		for _, secret := range []string{r.Event, r.Plane, string(r.Details)} {
			if secret != "" && secret != "{}" && strings.Contains(out, secret) {
				t.Fatalf("report leaks row content %q:\n%s", secret, out)
			}
		}
	}
}
