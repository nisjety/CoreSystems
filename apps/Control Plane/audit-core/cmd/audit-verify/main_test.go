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
