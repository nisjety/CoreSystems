// audit-verify checks an exported audit chain without a database or a network:
// it reads event pages, a manifest and a public key, recomputes every row hash
// and checks the signed checkpoint and attestation, using the same rules the
// server's verifier uses (internal/verifier over internal/chain).
//
//	audit-verify -manifest manifest.json -key-id KEY -public-key BASE64 page1.json page2.json ...
//
// Exit status: 0 verified (or verified_with_legacy), 2 broken, 3 unverifiable
// (partial, key_unavailable, or input it cannot read), 1 for bad usage. The
// report never says a bare "ok": the status names what was and was not shown.
//
// A page is the compliance API envelope {"data":[...]} or a bare JSON array of
// events. Each event carries every field the row hash covers plus the stored
// link and hash (hashes and signatures are lowercase hex):
//
//	org_id, chain_seq, occurred_at, ingested_at (RFC 3339), user_id, actor_role,
//	plane, event, subject, resource_id, outcome, event_id, request_id,
//	ip_address (the host part, no netmask), user_agent, source_producer,
//	source_bus, payload_hash, details (the JSON value), chain_version,
//	prev_hash, row_hash
//
// The manifest names what the export claims: the head (last_seq, last_row_hash),
// the newest checkpoint and the newest attestation, both signed, and the number
// of rows the org has that were never chained. Nothing in the manifest is
// trusted beyond its signatures: the rows must still reach the claimed head.
package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"sort"
	"time"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

const (
	exitVerified     = 0
	exitUsage        = 1
	exitBroken       = 2
	exitUnverifiable = 3
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// exportRow is one event of a page, mirroring the stored columns the row hash
// is computed over.
type exportRow struct {
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

func (r exportRow) stored() verifier.StoredRow {
	return verifier.StoredRow{
		Row: chain.Row{
			OrgID: r.OrgID, ChainSeq: r.ChainSeq, OccurredAt: r.OccurredAt, IngestedAt: r.IngestedAt,
			UserID: r.UserID, ActorRole: r.ActorRole, Plane: r.Plane, Event: r.Event, Subject: r.Subject,
			ResourceID: r.ResourceID, Outcome: r.Outcome, EventID: r.EventID, RequestID: r.RequestID,
			Host: r.IPAddress, UserAgent: r.UserAgent, SourceProducer: r.SourceProducer, SourceBus: r.SourceBus,
			PayloadHash: r.PayloadHash, Details: []byte(r.Details),
		},
		PrevHash: r.PrevHash,
		RowHash:  r.RowHash,
		Version:  r.ChainVersion,
	}
}

// manifest is what the export claims about the org's chain.
type manifest struct {
	OrgID               string                `json:"org_id"`
	LastSeq             int64                 `json:"last_seq"`
	LastRowHash         verifier.Hash         `json:"last_row_hash"`
	Checkpoint          *verifier.Checkpoint  `json:"checkpoint"`
	Attestation         *verifier.Attestation `json:"attestation"`
	LegacyUnchainedRows int64                 `json:"legacy_unchained_rows"`
}

// rowQueue is the verifier's row source over the loaded pages, with the
// manifest's attestation attached to the row it was signed at.
type rowQueue struct {
	rows []verifier.StoredRow
	next int
}

func (q *rowQueue) Next() (verifier.StoredRow, bool, error) {
	if q.next >= len(q.rows) {
		return verifier.StoredRow{}, false, nil
	}
	row := q.rows[q.next]
	q.next++
	return row, true, nil
}

func run(args []string, stdout, stderr io.Writer) int {
	fs := flag.NewFlagSet("audit-verify", flag.ContinueOnError)
	fs.SetOutput(stderr)
	manifestPath := fs.String("manifest", "", "manifest file naming the head, checkpoint and attestation")
	keyID := fs.String("key-id", "", "id of the public key (the key id signatures name)")
	publicKey := fs.String("public-key", "", "base64 Ed25519 public key for -key-id")
	verifyKeys := fs.String("verify-keys", "", "retired keys: comma-separated key_id:public_key pairs")
	asJSON := fs.Bool("json", false, "print the report as JSON")
	fs.Usage = func() {
		fmt.Fprintln(stderr, "usage: audit-verify [-manifest FILE] [-key-id ID -public-key BASE64] [-verify-keys LIST] [-json] PAGE...")
		fs.PrintDefaults()
	}
	if err := fs.Parse(args); err != nil {
		return exitUsage
	}
	if fs.NArg() == 0 {
		fs.Usage()
		return exitUsage
	}
	ring, err := chain.NewKeyRing(chain.KeyConfig{KeyID: *keyID, Public: *publicKey, VerifyKeys: *verifyKeys})
	if err != nil {
		fmt.Fprintf(stderr, "audit-verify: %v\n", err)
		return exitUsage
	}

	// Unreadable or malformed input cannot be told from a damaged export, so
	// it is "unverifiable", never "broken" and never "verified".
	rows, err := loadPages(fs.Args())
	if err != nil {
		fmt.Fprintf(stderr, "audit-verify: %v\n", err)
		return exitUnverifiable
	}
	var man manifest
	if *manifestPath != "" {
		if man, err = loadManifest(*manifestPath); err != nil {
			fmt.Fprintf(stderr, "audit-verify: %v\n", err)
			return exitUnverifiable
		}
	}
	if len(rows) == 0 {
		fmt.Fprintln(stderr, "audit-verify: the pages contain no events")
		return exitUnverifiable
	}
	org := rows[0].OrgID
	if man.OrgID != "" && man.OrgID != org {
		fmt.Fprintln(stderr, "audit-verify: the manifest belongs to a different org than the pages")
		return exitUnverifiable
	}

	sort.SliceStable(rows, func(i, j int) bool { return rows[i].ChainSeq < rows[j].ChainSeq })
	fromSeq := rows[0].ChainSeq
	if cp := man.Checkpoint; cp != nil {
		// Rows at or below a checkpoint were pruned on purpose; an export taken
		// before the prune still lists them, and the signed checkpoint covers them.
		kept := rows[:0:0]
		for _, r := range rows {
			if r.ChainSeq > cp.Seq {
				kept = append(kept, r)
			}
		}
		if dropped := len(rows) - len(kept); dropped > 0 {
			fmt.Fprintf(stderr, "audit-verify: ignoring %d rows at or below the checkpoint (seq %d), which covers them\n", dropped, cp.Seq)
		}
		rows = kept
		fromSeq = cp.Seq + 1
	}
	params := verifier.Params{
		OrgID: org, Keys: ring, Checkpoint: man.Checkpoint, Newest: man.Attestation,
		Legacy: man.LegacyUnchainedRows, FromSeq: fromSeq,
	}
	if man.LastSeq > 0 {
		params.Head = &verifier.Head{Seq: man.LastSeq, Hash: man.LastRowHash}
	}
	queue := &rowQueue{rows: make([]verifier.StoredRow, len(rows))}
	for i, r := range rows {
		queue.rows[i] = r.stored()
		if a := man.Attestation; a != nil && a.Seq == r.ChainSeq {
			queue.rows[i].Attestation = a
		}
	}

	report, err := verifier.Verify(params, queue)
	if err != nil {
		fmt.Fprintf(stderr, "audit-verify: %v\n", err)
		return exitUnverifiable
	}
	if *asJSON {
		enc := json.NewEncoder(stdout)
		enc.SetIndent("", "  ")
		_ = enc.Encode(report)
	} else {
		printReport(stdout, report)
	}
	switch {
	case report.OK():
		return exitVerified
	case report.Status == verifier.StatusBroken:
		return exitBroken
	default:
		return exitUnverifiable
	}
}

func printReport(w io.Writer, r verifier.Report) {
	fmt.Fprintf(w, "status: %s\n", r.Status)
	if r.Reason != "" {
		fmt.Fprintf(w, "reason: %s\n", r.Reason)
	}
	if r.FirstBrokenSeq > 0 {
		fmt.Fprintf(w, "first_broken_seq: %d\n", r.FirstBrokenSeq)
	}
	fmt.Fprintf(w, "org: %s\n", r.OrgID)
	fmt.Fprintf(w, "rows verified: %d (from seq %d, through seq %d)\n", r.RowsVerified, r.FromSeq, r.ThroughSeq)
	if r.Checkpoint != nil {
		fmt.Fprintf(w, "checkpoint: through seq %d, key %s, signature valid: %t\n",
			r.Checkpoint.ThroughSeq, r.Checkpoint.KeyID, r.Checkpoint.Verified)
	}
	if r.Head != nil {
		fmt.Fprintf(w, "head: seq %d\n", r.Head.Seq)
	}
	if r.Attested != nil {
		fmt.Fprintf(w, "attested: seq %d, key %s, signature valid: %t\n", r.Attested.Seq, r.Attested.KeyID, r.Attested.Verified)
	} else {
		fmt.Fprintln(w, "attested: none")
	}
	fmt.Fprintf(w, "legacy unchained rows (not covered): %d\n", r.LegacyUnchainedRows)
}

// loadPages reads every page file into one list of events.
func loadPages(paths []string) ([]exportRow, error) {
	var all []exportRow
	for _, path := range paths {
		raw, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		page, err := parsePage(raw)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", path, err)
		}
		all = append(all, page...)
	}
	return all, nil
}

// parsePage accepts the API envelope or a bare array.
func parsePage(raw []byte) ([]exportRow, error) {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 {
		return nil, errors.New("page is empty")
	}
	var rows []exportRow
	if trimmed[0] == '[' {
		return rows, strictUnmarshal(trimmed, &rows)
	}
	var envelope struct {
		Data []exportRow `json:"data"`
	}
	if err := strictUnmarshal(trimmed, &envelope); err != nil {
		return nil, err
	}
	return envelope.Data, nil
}

func loadManifest(path string) (manifest, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return manifest{}, err
	}
	var m manifest
	if err := strictUnmarshal(raw, &m); err != nil {
		return manifest{}, fmt.Errorf("%s: %w", path, err)
	}
	return m, nil
}

// strictUnmarshal decodes one JSON value and refuses trailing data. Unknown
// fields are allowed: an export may carry fields this verifier does not hash.
func strictUnmarshal(raw []byte, v any) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	if err := dec.Decode(v); err != nil {
		return err
	}
	if _, err := dec.Token(); !errors.Is(err, io.EOF) {
		return errors.New("unexpected data after the JSON value")
	}
	return nil
}
