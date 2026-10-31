package siem

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"strconv"
	"time"

	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// Record types in an export. A receiver de-duplicates audit_event records on
// (org_id, chain_seq) and re-verifies the chain from the fields they carry.
const (
	TypeEvent = "audit_event"
	TypeHead  = "audit_chain_head"
	TypeGap   = "audit_export_gap"

	// Why a stretch of chain_seq is absent from the export. A receiver or an
	// offline verifier reads the reason instead of guessing at a hole.
	GapMissing  = "missing_rows" // the rows are not in the chain table (pruned by retention)
	GapFiltered = "filtered"     // the destination's event filter excluded them
	GapOversize = "row_too_large"
)

// Item is one record of a batch, before it is encoded for a destination kind.
// Exactly one of Event, Gap and Head is set. At is the record's time (the
// event's occurred_at, or the export time for a gap and a head).
type Item struct {
	At    time.Time
	Event *store.ComplianceEvent
	Gap   *Gap
	Head  *Head
}

// Gap says that chain_seq From..To (inclusive) of OrgID is not in the export.
type Gap struct {
	OrgID  string
	From   int64
	To     int64
	Reason string
}

// Head is the org's chain head with its newest signed attestation: a copy of
// the trust anchor outside the database that produced it, which is what makes
// the SIEM an independent witness. Attestation is nil when none is signed yet.
type Head struct {
	OrgID       string
	HeadSeq     int64
	Attestation *verifier.Attestation
}

// format encodes one destination kind: one line per record, and the request
// headers (including the authentication) for the assembled body.
type format struct {
	line   func(Item) ([]byte, error)
	header func(secret string, body []byte, now time.Time) http.Header
}

func formatFor(kind string) (format, bool) {
	switch kind {
	case "ndjson_webhook":
		return format{line: ndjsonLine, header: ndjsonHeader}, true
	case "splunk_hec":
		return format{line: hecLine, header: hecHeader}, true
	}
	return format{}, false
}

type eventRecord struct {
	Type string `json:"type"`
	store.ComplianceEvent
}

type gapRecord struct {
	Type   string `json:"type"`
	OrgID  string `json:"org_id"`
	From   int64  `json:"from_seq"`
	To     int64  `json:"to_seq"`
	Reason string `json:"reason"`
}

type headRecord struct {
	Type        string                `json:"type"`
	OrgID       string                `json:"org_id"`
	HeadSeq     int64                 `json:"head_seq"`
	Attestation *verifier.Attestation `json:"attestation"`
}

// record is the JSON shape of an item, shared by both destination kinds so a
// receiver sees the same fields whichever wrapper carries them.
func record(item Item) any {
	switch {
	case item.Event != nil:
		return eventRecord{Type: TypeEvent, ComplianceEvent: *item.Event}
	case item.Gap != nil:
		return gapRecord{Type: TypeGap, OrgID: item.Gap.OrgID, From: item.Gap.From, To: item.Gap.To, Reason: item.Gap.Reason}
	default:
		return headRecord{Type: TypeHead, OrgID: item.Head.OrgID, HeadSeq: item.Head.HeadSeq, Attestation: item.Head.Attestation}
	}
}

func ndjsonLine(item Item) ([]byte, error) {
	return json.Marshal(record(item))
}

// ndjsonHeader signs the body: X-Verevon-Signature is sha256=hex(HMAC-SHA256(
// secret, timestamp + "." + body)) and the timestamp is sent beside it, so a
// receiver can check the body and refuse a stale replay.
func ndjsonHeader(secret string, body []byte, now time.Time) http.Header {
	timestamp := strconv.FormatInt(now.Unix(), 10)
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(timestamp))
	mac.Write([]byte("."))
	mac.Write(body)
	header := http.Header{}
	header.Set("Content-Type", "application/x-ndjson")
	header.Set("X-Verevon-Timestamp", timestamp)
	header.Set("X-Verevon-Signature", "sha256="+hex.EncodeToString(mac.Sum(nil)))
	return header
}
