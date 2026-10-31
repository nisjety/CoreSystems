// Package chain is the pure encoding half of audit-core's tamper-evident
// log: the canonical byte layout every row hash and every signed payload is
// computed over. It has no database, no clock and no key material, so the
// inserter, the verifier, the compliance export and the offline audit-verify
// CLI all derive the same bytes from one place. testdata/vectors.json is the
// single golden source; any change to an encoding below is a new format
// version (a new magic), never an edit of v1.
package chain

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	// Version is the row-hash format version stored in audit_events.chain_version.
	Version = 1

	// HashSize is the length in bytes of every hash in the chain.
	HashSize = sha256.Size

	rowMagic         = "VAC1\x00"
	genesisPrefix    = "VAC1/genesis/"
	attestationMagic = "VAH1\n"
	checkpointMagic  = "VAK1\n"

	// SignedTimeLayout is the one rendering of a timestamp inside a signed
	// payload: UTC, fixed microsecond width (Postgres stores microseconds).
	SignedTimeLayout = "2006-01-02T15:04:05.000000Z"
)

var (
	ErrOrgRequired  = errors.New("chain: org id is required")
	ErrBadPrevHash  = errors.New("chain: prev hash must be 32 bytes")
	ErrBadHash      = errors.New("chain: hash must be 32 bytes")
	ErrBadSequence  = errors.New("chain: sequence must be at least 1")
	ErrBadCount     = errors.New("chain: pruned total must not be negative")
	ErrOrgNewline   = errors.New("chain: org id must not contain a newline")
	ErrEmptyDetails = errors.New("chain: details must be a JSON value")
	ErrDetailsTrail = errors.New("chain: details has trailing data after the JSON value")
	errUnknownKind  = errors.New("chain: unsupported JSON value in details")
)

// Row is every field a row hash covers, as read back from the stored row
// (never as received at ingest, so Postgres normalisation of jsonb, inet and
// timestamptz cannot make a recomputation disagree). NULL columns are empty
// strings. id, source_subject and source_stream_sequence are deliberately not
// part of the hash: they are storage coordinates, not the audited fact.
type Row struct {
	OrgID          string
	ChainSeq       int64
	OccurredAt     time.Time
	IngestedAt     time.Time
	UserID         string
	ActorRole      string
	Plane          string
	Event          string
	Subject        string
	ResourceID     string
	Outcome        string
	EventID        string
	RequestID      string
	Host           string // host(ip_address): the address without a netmask
	UserAgent      string
	SourceProducer string
	SourceBus      string
	PayloadHash    []byte // raw bytes of payload_hash; nil when NULL
	Details        []byte // details as jsonb text read back from the row
}

// GenesisHash is the prev hash of an org's first chained row. Binding it to
// the org id means a row moved to another org's chain cannot verify.
func GenesisHash(orgID string) ([]byte, error) {
	if orgID == "" {
		return nil, ErrOrgRequired
	}
	sum := sha256.Sum256([]byte(genesisPrefix + orgID))
	return sum[:], nil
}

// RowHash computes the v1 row hash:
//
//	SHA256("VAC1\x00" || prev || F(org_id) || U64(chain_seq) ||
//	       I64(occurred_at_micros) || I64(ingested_at_micros) || F(user_id) ||
//	       F(actor_role) || F(plane) || F(event) || F(subject) || F(resource_id) ||
//	       F(outcome) || F(event_id) || F(request_id) || F(host) || F(user_agent) ||
//	       F(source_producer) || F(source_bus) || F(payload_hash) ||
//	       F(details_canonical))
//
// where F(x) is uvarint(len(x)) || x. The length prefix is what keeps
// ("ab","c") and ("a","bc") from colliding.
func RowHash(prev []byte, r Row) ([]byte, error) {
	if len(prev) != HashSize {
		return nil, ErrBadPrevHash
	}
	if r.OrgID == "" {
		return nil, ErrOrgRequired
	}
	if r.ChainSeq < 1 {
		return nil, ErrBadSequence
	}
	details, err := CanonicalDetails(r.Details)
	if err != nil {
		return nil, err
	}

	var buf bytes.Buffer
	buf.WriteString(rowMagic)
	buf.Write(prev)
	writeField(&buf, []byte(r.OrgID))
	writeUint64(&buf, uint64(r.ChainSeq))
	writeInt64(&buf, r.OccurredAt.UnixMicro())
	writeInt64(&buf, r.IngestedAt.UnixMicro())
	for _, s := range []string{
		r.UserID, r.ActorRole, r.Plane, r.Event, r.Subject, r.ResourceID,
		r.Outcome, r.EventID, r.RequestID, r.Host, r.UserAgent,
		r.SourceProducer, r.SourceBus,
	} {
		writeField(&buf, []byte(s))
	}
	writeField(&buf, r.PayloadHash)
	writeField(&buf, details)

	sum := sha256.Sum256(buf.Bytes())
	return sum[:], nil
}

// AttestationPayload is the exact byte string a head attestation signs:
// "VAH1\n"+org+"\n"+seq+"\n"+hex(hash)+"\n"+signed_at.
func AttestationPayload(orgID string, chainSeq int64, rowHash []byte, signedAt time.Time) ([]byte, error) {
	if err := checkSignedHeader(orgID, chainSeq, rowHash); err != nil {
		return nil, err
	}
	return []byte(attestationMagic + orgID + "\n" + strconv.FormatInt(chainSeq, 10) + "\n" +
		hex.EncodeToString(rowHash) + "\n" + signedAt.UTC().Format(SignedTimeLayout)), nil
}

// CheckpointPayload is the exact byte string a retention checkpoint signs:
// "VAK1\n"+org+"\n"+through_seq+"\n"+hex(hash)+"\n"+pruned_total+"\n"+created_at.
// pruned_total is cumulative, so a checkpoint also commits to how much was
// legitimately removed before it.
func CheckpointPayload(orgID string, throughSeq int64, throughHash []byte, prunedTotal int64, createdAt time.Time) ([]byte, error) {
	if err := checkSignedHeader(orgID, throughSeq, throughHash); err != nil {
		return nil, err
	}
	if prunedTotal < 0 {
		return nil, ErrBadCount
	}
	return []byte(checkpointMagic + orgID + "\n" + strconv.FormatInt(throughSeq, 10) + "\n" +
		hex.EncodeToString(throughHash) + "\n" + strconv.FormatInt(prunedTotal, 10) + "\n" +
		createdAt.UTC().Format(SignedTimeLayout)), nil
}

// checkSignedHeader rejects what would make a newline-delimited payload
// ambiguous or unverifiable: an org id that can smuggle a line break, a
// non-positive sequence, or a hash of the wrong size.
func checkSignedHeader(orgID string, seq int64, hash []byte) error {
	switch {
	case orgID == "":
		return ErrOrgRequired
	case strings.ContainsRune(orgID, '\n'):
		return ErrOrgNewline
	case seq < 1:
		return ErrBadSequence
	case len(hash) != HashSize:
		return ErrBadHash
	}
	return nil
}

// CanonicalDetails rewrites a JSON document into its canonical form: object
// keys sorted by UTF-8 bytes, no insignificant whitespace, no HTML escaping,
// strings escaped by the fixed rule in writeString, and every number kept as
// its original token. Numbers are never parsed, so 1.0, 1e3 and 1000 stay
// distinct instead of drifting through float64; Postgres has already
// normalised whatever it stores, and the hash is taken over that read-back
// text. The output is a fixed point: canonicalising it again returns it.
func CanonicalDetails(raw []byte) ([]byte, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		if errors.Is(err, io.EOF) {
			return nil, ErrEmptyDetails
		}
		return nil, fmt.Errorf("chain: details is not valid JSON: %w", err)
	}
	if _, err := dec.Token(); !errors.Is(err, io.EOF) {
		return nil, ErrDetailsTrail
	}
	var buf bytes.Buffer
	if err := writeCanonical(&buf, v); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func writeCanonical(buf *bytes.Buffer, v any) error {
	switch t := v.(type) {
	case nil:
		buf.WriteString("null")
	case bool:
		buf.WriteString(strconv.FormatBool(t))
	case json.Number:
		buf.WriteString(string(t))
	case string:
		writeString(buf, t)
	case []any:
		buf.WriteByte('[')
		for i, e := range t {
			if i > 0 {
				buf.WriteByte(',')
			}
			if err := writeCanonical(buf, e); err != nil {
				return err
			}
		}
		buf.WriteByte(']')
	case map[string]any:
		keys := make([]string, 0, len(t))
		for k := range t {
			keys = append(keys, k)
		}
		sort.Strings(keys) // Go compares strings bytewise, which is UTF-8 order
		buf.WriteByte('{')
		for i, k := range keys {
			if i > 0 {
				buf.WriteByte(',')
			}
			writeString(buf, k)
			buf.WriteByte(':')
			if err := writeCanonical(buf, t[k]); err != nil {
				return err
			}
		}
		buf.WriteByte('}')
	default:
		return errUnknownKind
	}
	return nil
}

// writeString is the canonical string escape, spelled out here rather than
// delegated to encoding/json so the byte layout cannot move with a Go
// release and a non-Go verifier can implement it from this comment: '"' and
// '\\' are backslash-escaped, \b \f \n \r \t use their short escapes, every
// other byte below 0x20 is \u00xx in lowercase hex, and everything else
// (including '<', '>', '&', 0x7f, U+2028 and U+2029) is emitted as raw UTF-8.
func writeString(buf *bytes.Buffer, s string) {
	const hexDigits = "0123456789abcdef"
	buf.WriteByte('"')
	for _, r := range s {
		switch {
		case r == '"':
			buf.WriteString(`\"`)
		case r == '\\':
			buf.WriteString(`\\`)
		case r == '\b':
			buf.WriteString(`\b`)
		case r == '\f':
			buf.WriteString(`\f`)
		case r == '\n':
			buf.WriteString(`\n`)
		case r == '\r':
			buf.WriteString(`\r`)
		case r == '\t':
			buf.WriteString(`\t`)
		case r < 0x20:
			buf.WriteString(`\u00`)
			buf.WriteByte(hexDigits[r>>4])
			buf.WriteByte(hexDigits[r&0xf])
		default:
			buf.WriteString(string(r)) // invalid bytes arrive here as U+FFFD
		}
	}
	buf.WriteByte('"')
}

func writeField(buf *bytes.Buffer, b []byte) {
	var n [binary.MaxVarintLen64]byte
	buf.Write(n[:binary.PutUvarint(n[:], uint64(len(b)))])
	buf.Write(b)
}

func writeUint64(buf *bytes.Buffer, v uint64) {
	var b [8]byte
	binary.BigEndian.PutUint64(b[:], v)
	buf.Write(b[:])
}

func writeInt64(buf *bytes.Buffer, v int64) {
	writeUint64(buf, uint64(v))
}
