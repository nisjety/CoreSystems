package chain

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math/rand"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

// vectorsSHA256 pins testdata/vectors.json (line endings normalised to LF so
// a Windows checkout agrees). The same value is recorded in the audit
// integrity ADR. A deliberate format change updates the file, this constant
// and the ADR together; an accidental one fails here first.
const vectorsSHA256 = "a4d3e9a8a3beec560015d0d7b6eb5dd1c9312585d4210eb7c8daff14fe7e73cf"

type vectorRow struct {
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
}

func (v vectorRow) row(t *testing.T) Row {
	t.Helper()
	var payload []byte
	if v.PayloadHash != "" {
		payload = mustHex(t, v.PayloadHash)
	}
	return Row{
		OrgID:          v.OrgID,
		ChainSeq:       v.ChainSeq,
		OccurredAt:     time.UnixMicro(v.OccurredAtMicros),
		IngestedAt:     time.UnixMicro(v.IngestedAtMicros),
		UserID:         v.UserID,
		ActorRole:      v.ActorRole,
		Plane:          v.Plane,
		Event:          v.Event,
		Subject:        v.Subject,
		ResourceID:     v.ResourceID,
		Outcome:        v.Outcome,
		EventID:        v.EventID,
		RequestID:      v.RequestID,
		Host:           v.Host,
		UserAgent:      v.UserAgent,
		SourceProducer: v.SourceProducer,
		SourceBus:      v.SourceBus,
		PayloadHash:    payload,
		Details:        []byte(v.Details),
	}
}

type vectors struct {
	Format  string `json:"format"`
	Genesis []struct {
		OrgID string `json:"org_id"`
		Hash  string `json:"hash"`
	} `json:"genesis"`
	CanonicalDetails []struct {
		Name      string `json:"name"`
		Input     string `json:"input"`
		Canonical string `json:"canonical"`
	} `json:"canonical_details"`
	Rows []struct {
		Name     string    `json:"name"`
		PrevHash string    `json:"prev_hash"`
		Row      vectorRow `json:"row"`
		RowHash  string    `json:"row_hash"`
	} `json:"rows"`
	Chain []struct {
		PrevHash string    `json:"prev_hash"`
		Row      vectorRow `json:"row"`
		RowHash  string    `json:"row_hash"`
	} `json:"chain"`
	Attestations []struct {
		OrgID          string `json:"org_id"`
		ChainSeq       int64  `json:"chain_seq"`
		RowHash        string `json:"row_hash"`
		SignedAtMicros int64  `json:"signed_at_micros"`
		Payload        string `json:"payload"`
	} `json:"attestations"`
	Checkpoints []struct {
		OrgID           string `json:"org_id"`
		ThroughSeq      int64  `json:"through_seq"`
		ThroughHash     string `json:"through_hash"`
		PrunedTotal     int64  `json:"pruned_total"`
		CreatedAtMicros int64  `json:"created_at_micros"`
		Payload         string `json:"payload"`
	} `json:"checkpoints"`
}

func mustHex(t testing.TB, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("bad hex %q: %v", s, err)
	}
	return b
}

func loadVectors(t *testing.T) vectors {
	t.Helper()
	raw, err := os.ReadFile("testdata/vectors.json")
	if err != nil {
		t.Fatal(err)
	}
	var v vectors
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatal(err)
	}
	if v.Format != "VAC1" {
		t.Fatalf("vectors format = %q", v.Format)
	}
	return v
}

func TestVectorsFileIsPinned(t *testing.T) {
	raw, err := os.ReadFile("testdata/vectors.json")
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(bytes.ReplaceAll(raw, []byte("\r\n"), []byte("\n")))
	if got := hex.EncodeToString(sum[:]); got != vectorsSHA256 {
		t.Fatalf("testdata/vectors.json sha256 = %s, pinned %s: a format change must update the constant and the ADR", got, vectorsSHA256)
	}
}

func TestGenesisHashVectors(t *testing.T) {
	for _, g := range loadVectors(t).Genesis {
		got, err := GenesisHash(g.OrgID)
		if err != nil {
			t.Fatal(err)
		}
		if hex.EncodeToString(got) != g.Hash {
			t.Errorf("GenesisHash(%q) = %x, want %s", g.OrgID, got, g.Hash)
		}
	}
	if _, err := GenesisHash(""); !errors.Is(err, ErrOrgRequired) {
		t.Fatalf("empty org error = %v", err)
	}
	a, _ := GenesisHash("org_a")
	b, _ := GenesisHash("org_b")
	if bytes.Equal(a, b) {
		t.Fatal("genesis must differ per org")
	}
}

func TestCanonicalDetailsVectors(t *testing.T) {
	for _, c := range loadVectors(t).CanonicalDetails {
		got, err := CanonicalDetails([]byte(c.Input))
		if err != nil {
			t.Errorf("%s: %v", c.Name, err)
			continue
		}
		if string(got) != c.Canonical {
			t.Errorf("%s: canonical = %s, want %s", c.Name, got, c.Canonical)
		}
		again, err := CanonicalDetails(got)
		if err != nil || !bytes.Equal(again, got) {
			t.Errorf("%s: canonical form is not a fixed point: %s (%v)", c.Name, again, err)
		}
	}
}

func TestRowHashVectors(t *testing.T) {
	v := loadVectors(t)
	if len(v.Rows) == 0 {
		t.Fatal("no row vectors")
	}
	seen := map[string]string{}
	for _, c := range v.Rows {
		got, err := RowHash(mustHex(t, c.PrevHash), c.Row.row(t))
		if err != nil {
			t.Errorf("%s: %v", c.Name, err)
			continue
		}
		if hex.EncodeToString(got) != c.RowHash {
			t.Errorf("%s: row hash = %x, want %s", c.Name, got, c.RowHash)
		}
		// The key-order pair is the only intentional collision.
		if other, dup := seen[c.RowHash]; dup && !(strings.HasPrefix(c.Name, "key_order") && strings.HasPrefix(other, "key_order")) {
			t.Errorf("%s collides with %s", c.Name, other)
		}
		seen[c.RowHash] = c.Name
	}
}

func TestChainVectorsLinkEachRowToItsPredecessor(t *testing.T) {
	v := loadVectors(t)
	prev, err := GenesisHash(v.Chain[0].Row.OrgID)
	if err != nil {
		t.Fatal(err)
	}
	for i, c := range v.Chain {
		if hex.EncodeToString(prev) != c.PrevHash {
			t.Fatalf("row %d prev_hash = %s, want %x", i, c.PrevHash, prev)
		}
		got, err := RowHash(prev, c.Row.row(t))
		if err != nil {
			t.Fatal(err)
		}
		if hex.EncodeToString(got) != c.RowHash {
			t.Fatalf("row %d hash = %x, want %s", i, got, c.RowHash)
		}
		prev = got
	}
}

func TestSignedPayloadVectors(t *testing.T) {
	v := loadVectors(t)
	for _, a := range v.Attestations {
		got, err := AttestationPayload(a.OrgID, a.ChainSeq, mustHex(t, a.RowHash), time.UnixMicro(a.SignedAtMicros))
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != a.Payload {
			t.Errorf("attestation payload = %q, want %q", got, a.Payload)
		}
	}
	for _, c := range v.Checkpoints {
		got, err := CheckpointPayload(c.OrgID, c.ThroughSeq, mustHex(t, c.ThroughHash), c.PrunedTotal, time.UnixMicro(c.CreatedAtMicros))
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != c.Payload {
			t.Errorf("checkpoint payload = %q, want %q", got, c.Payload)
		}
	}
	if len(v.Attestations) == 0 || len(v.Checkpoints) == 0 {
		t.Fatal("missing signed payload vectors")
	}
}

func TestSignedPayloadsAreDomainSeparated(t *testing.T) {
	h := bytes.Repeat([]byte{7}, HashSize)
	at := time.UnixMicro(1760000000000000)
	a, _ := AttestationPayload("org_a", 5, h, at)
	k, _ := CheckpointPayload("org_a", 5, h, 0, at)
	if !strings.HasPrefix(string(a), "VAH1\n") || !strings.HasPrefix(string(k), "VAK1\n") {
		t.Fatalf("magics wrong: %q %q", a, k)
	}
	if bytes.Equal(a, k) || bytes.HasPrefix(k, a) || bytes.HasPrefix(a, k) {
		t.Fatal("an attestation must never be valid as a checkpoint or the reverse")
	}
	// The signed time is UTC whatever zone the caller holds it in.
	zone := time.FixedZone("x", 5*3600)
	b, _ := AttestationPayload("org_a", 5, h, at.In(zone))
	if !bytes.Equal(a, b) {
		t.Fatalf("zone changed the payload: %q vs %q", a, b)
	}
}

func TestSignedPayloadRejectsAmbiguousInput(t *testing.T) {
	h := bytes.Repeat([]byte{1}, HashSize)
	at := time.Unix(0, 0)
	cases := []struct {
		name string
		fn   func() error
		want error
	}{
		{"attest empty org", func() error { _, e := AttestationPayload("", 1, h, at); return e }, ErrOrgRequired},
		{"attest newline org", func() error { _, e := AttestationPayload("a\nb", 1, h, at); return e }, ErrOrgNewline},
		{"attest zero seq", func() error { _, e := AttestationPayload("o", 0, h, at); return e }, ErrBadSequence},
		{"attest short hash", func() error { _, e := AttestationPayload("o", 1, h[:5], at); return e }, ErrBadHash},
		{"checkpoint newline org", func() error { _, e := CheckpointPayload("a\nb", 1, h, 0, at); return e }, ErrOrgNewline},
		{"checkpoint negative total", func() error { _, e := CheckpointPayload("o", 1, h, -1, at); return e }, ErrBadCount},
		{"checkpoint short hash", func() error { _, e := CheckpointPayload("o", 1, nil, 0, at); return e }, ErrBadHash},
	}
	for _, c := range cases {
		if err := c.fn(); !errors.Is(err, c.want) {
			t.Errorf("%s: err = %v, want %v", c.name, err, c.want)
		}
	}
}

func baseRow() Row {
	return Row{
		OrgID: "org_a", ChainSeq: 3,
		OccurredAt: time.UnixMicro(1760000000000000), IngestedAt: time.UnixMicro(1760000000000001),
		UserID: "u", ActorRole: "admin", Plane: "control", Event: "e", Subject: "s", ResourceID: "r",
		Outcome: "ok", EventID: "ev", RequestID: "rq", Host: "203.0.113.1", UserAgent: "ua",
		SourceProducer: "p", SourceBus: "b", PayloadHash: bytes.Repeat([]byte{9}, HashSize),
		Details: []byte(`{"k":"v"}`),
	}
}

func TestLengthPrefixPreventsFieldBoundaryCollisions(t *testing.T) {
	prev, _ := GenesisHash("org_a")
	a, b := baseRow(), baseRow()
	a.UserID, a.ActorRole = "ab", "c"
	b.UserID, b.ActorRole = "a", "bc"
	ha, _ := RowHash(prev, a)
	hb, _ := RowHash(prev, b)
	if bytes.Equal(ha, hb) {
		t.Fatal(`("ab","c") and ("a","bc") hash the same`)
	}
	// Moving a value between an empty and a populated neighbour also changes it.
	c, d := baseRow(), baseRow()
	c.Subject, c.ResourceID = "x", ""
	d.Subject, d.ResourceID = "", "x"
	hc, _ := RowHash(prev, c)
	hd, _ := RowHash(prev, d)
	if bytes.Equal(hc, hd) {
		t.Fatal("empty-versus-populated neighbours hash the same")
	}
}

func TestEveryHashedFieldChangesTheHash(t *testing.T) {
	prev, _ := GenesisHash("org_a")
	base, err := RowHash(prev, baseRow())
	if err != nil {
		t.Fatal(err)
	}
	mutations := map[string]func(*Row){
		"OrgID":          func(r *Row) { r.OrgID = "org_b" },
		"ChainSeq":       func(r *Row) { r.ChainSeq++ },
		"OccurredAt":     func(r *Row) { r.OccurredAt = r.OccurredAt.Add(time.Microsecond) },
		"IngestedAt":     func(r *Row) { r.IngestedAt = r.IngestedAt.Add(time.Microsecond) },
		"UserID":         func(r *Row) { r.UserID += "x" },
		"ActorRole":      func(r *Row) { r.ActorRole += "x" },
		"Plane":          func(r *Row) { r.Plane += "x" },
		"Event":          func(r *Row) { r.Event += "x" },
		"Subject":        func(r *Row) { r.Subject += "x" },
		"ResourceID":     func(r *Row) { r.ResourceID += "x" },
		"Outcome":        func(r *Row) { r.Outcome += "x" },
		"EventID":        func(r *Row) { r.EventID += "x" },
		"RequestID":      func(r *Row) { r.RequestID += "x" },
		"Host":           func(r *Row) { r.Host += "1" },
		"UserAgent":      func(r *Row) { r.UserAgent += "x" },
		"SourceProducer": func(r *Row) { r.SourceProducer += "x" },
		"SourceBus":      func(r *Row) { r.SourceBus += "x" },
		"PayloadHash":    func(r *Row) { r.PayloadHash = bytes.Repeat([]byte{8}, HashSize) },
		"Details":        func(r *Row) { r.Details = []byte(`{"k":"w"}`) },
	}
	rt := reflect.TypeOf(Row{})
	if len(mutations) != rt.NumField() {
		t.Fatalf("%d mutations for %d Row fields: a new field must be hashed and covered here", len(mutations), rt.NumField())
	}
	for name, mutate := range mutations {
		r := baseRow()
		mutate(&r)
		got, err := RowHash(prev, r)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if bytes.Equal(got, base) {
			t.Errorf("changing %s did not change the row hash", name)
		}
	}
	otherPrev := bytes.Repeat([]byte{4}, HashSize)
	if got, _ := RowHash(otherPrev, baseRow()); bytes.Equal(got, base) {
		t.Error("changing prev_hash did not change the row hash")
	}
}

func TestRowHashIgnoresNothingButIsZoneAndDetailsLayoutIndependent(t *testing.T) {
	prev, _ := GenesisHash("org_a")
	a := baseRow()
	b := baseRow()
	zone := time.FixedZone("x", -7*3600)
	b.OccurredAt, b.IngestedAt = a.OccurredAt.In(zone), a.IngestedAt.In(zone)
	b.Details = []byte(" {\n  \"k\" : \"v\"\n} ")
	ha, _ := RowHash(prev, a)
	hb, _ := RowHash(prev, b)
	if !bytes.Equal(ha, hb) {
		t.Fatal("time zone or details whitespace changed the hash")
	}
	// A NULL payload_hash and an empty one are the same thing: absent.
	a.PayloadHash, b.PayloadHash = nil, []byte{}
	ha, _ = RowHash(prev, a)
	hb, _ = RowHash(prev, b)
	if !bytes.Equal(ha, hb) {
		t.Fatal("nil and empty payload hash differ")
	}
}

func TestRowHashRejectsMalformedInput(t *testing.T) {
	prev, _ := GenesisHash("org_a")
	cases := []struct {
		name string
		prev []byte
		edit func(*Row)
		want error
	}{
		{"short prev", prev[:10], func(*Row) {}, ErrBadPrevHash},
		{"nil prev", nil, func(*Row) {}, ErrBadPrevHash},
		{"empty org", prev, func(r *Row) { r.OrgID = "" }, ErrOrgRequired},
		{"zero seq", prev, func(r *Row) { r.ChainSeq = 0 }, ErrBadSequence},
		{"negative seq", prev, func(r *Row) { r.ChainSeq = -4 }, ErrBadSequence},
		{"empty details", prev, func(r *Row) { r.Details = nil }, ErrEmptyDetails},
		{"trailing details", prev, func(r *Row) { r.Details = []byte(`{} {}`) }, ErrDetailsTrail},
	}
	for _, c := range cases {
		r := baseRow()
		c.edit(&r)
		if _, err := RowHash(c.prev, r); !errors.Is(err, c.want) {
			t.Errorf("%s: err = %v, want %v", c.name, err, c.want)
		}
	}
	r := baseRow()
	r.Details = []byte(`{"a":`)
	if _, err := RowHash(prev, r); err == nil {
		t.Error("truncated details accepted")
	}
}

func TestCanonicalDetailsKeepsNumberTokens(t *testing.T) {
	got, err := CanonicalDetails([]byte(`{"a":1.0,"b":1e3,"c":1000,"d":12345678901234567890123,"e":-0.0}`))
	if err != nil {
		t.Fatal(err)
	}
	want := `{"a":1.0,"b":1e3,"c":1000,"d":12345678901234567890123,"e":-0.0}`
	if string(got) != want {
		t.Fatalf("got %s, want %s", got, want)
	}
	// 1.0 and 1 are different stored values to the hash: it hashes read-back text.
	one, _ := CanonicalDetails([]byte(`{"a":1}`))
	if bytes.Equal(one, []byte(`{"a":1.0}`)) {
		t.Fatal("1 and 1.0 collapsed")
	}
}

func TestCanonicalDetailsKeyOrderPermutations(t *testing.T) {
	keys := []string{"alpha", "beta", "gamma", "delta", "éclair", "中"}
	rng := rand.New(rand.NewSource(7))
	var want []byte
	for i := 0; i < 50; i++ {
		rng.Shuffle(len(keys), func(a, b int) { keys[a], keys[b] = keys[b], keys[a] })
		var sb strings.Builder
		sb.WriteByte('{')
		for j, k := range keys {
			if j > 0 {
				sb.WriteString(", ")
			}
			sb.WriteString(`"` + k + `": {"y": 1, "x": [2, {"b": 1, "a": 2}]}`)
		}
		sb.WriteByte('}')
		got, err := CanonicalDetails([]byte(sb.String()))
		if err != nil {
			t.Fatal(err)
		}
		if want == nil {
			want = got
		} else if !bytes.Equal(got, want) {
			t.Fatalf("permutation %d produced %s, want %s", i, got, want)
		}
	}
}

func TestCanonicalDetailsRejectsInvalidDocuments(t *testing.T) {
	for _, in := range []string{``, `   `, `{`, `{"a":}`, `{"a":1,}`, `nul`, `{"a":1}x`, `{} []`} {
		if _, err := CanonicalDetails([]byte(in)); err == nil {
			t.Errorf("%q accepted", in)
		}
	}
}

func TestCanonicalDetailsIdempotentOverRandomDocuments(t *testing.T) {
	rng := rand.New(rand.NewSource(20261023))
	for i := 0; i < 500; i++ {
		doc := randomJSON(rng, 0)
		first, err := CanonicalDetails([]byte(doc))
		if err != nil {
			t.Fatalf("%q: %v", doc, err)
		}
		second, err := CanonicalDetails(first)
		if err != nil || !bytes.Equal(first, second) {
			t.Fatalf("not idempotent for %q: %s then %s (%v)", doc, first, second, err)
		}
	}
}

func randomJSON(rng *rand.Rand, depth int) string {
	atoms := []string{`null`, `true`, `false`, `0`, `-1`, `1.0`, `1e3`, `2.50`, `"x"`, `"a\"b"`, `"é\n"`, `" "`, `"<&>"`, `"\u0001"`}
	if depth > 3 {
		return atoms[rng.Intn(len(atoms))]
	}
	switch rng.Intn(4) {
	case 0:
		n := rng.Intn(4)
		parts := make([]string, n)
		for i := range parts {
			parts[i] = randomJSON(rng, depth+1)
		}
		return "[ " + strings.Join(parts, " , ") + " ]"
	case 1:
		n := rng.Intn(4)
		names := []string{"a", "b", "c", "é", "zz", "Z", ""}
		rng.Shuffle(len(names), func(i, j int) { names[i], names[j] = names[j], names[i] })
		parts := make([]string, n)
		for i := range parts {
			parts[i] = `"` + names[i] + `" : ` + randomJSON(rng, depth+1)
		}
		return "{ " + strings.Join(parts, " , ") + " }"
	default:
		return atoms[rng.Intn(len(atoms))]
	}
}

// FuzzCanonicalDetails holds the idempotence contract for arbitrary input: a
// document that canonicalises must canonicalise to itself, and the result
// must still be valid JSON. The seeds run as ordinary tests.
func FuzzCanonicalDetails(f *testing.F) {
	for _, s := range []string{`{}`, `[]`, `{"b":1,"a":1.0}`, `"😀"`, `{"s":"\u0000 <&>"}`, `1e3`, ` null `} {
		f.Add([]byte(s))
	}
	f.Fuzz(func(t *testing.T, in []byte) {
		first, err := CanonicalDetails(in)
		if err != nil {
			return
		}
		if !json.Valid(first) {
			t.Fatalf("canonical output is not JSON: %q from %q", first, in)
		}
		second, err := CanonicalDetails(first)
		if err != nil || !bytes.Equal(first, second) {
			t.Fatalf("not idempotent for %q: %q then %q (%v)", in, first, second, err)
		}
	})
}
