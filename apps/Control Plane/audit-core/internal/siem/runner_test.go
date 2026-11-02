package siem

import (
	"bufio"
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	dto "github.com/prometheus/client_model/go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// ---- fakes -----------------------------------------------------------------

type fakeSource struct {
	mu          sync.Mutex
	rows        []store.ComplianceEvent
	attestation *verifier.Attestation
	pageErr     error
}

func (s *fakeSource) Page(_ context.Context, orgID string, after int64, limit int) (store.CompliancePage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.pageErr != nil {
		return store.CompliancePage{}, s.pageErr
	}
	page := store.CompliancePage{Events: []store.ComplianceEvent{}}
	for _, row := range s.rows {
		if row.OrgID == orgID && row.ChainSeq > after {
			if len(page.Events) == limit {
				page.NextSeq = page.Events[len(page.Events)-1].ChainSeq
				break
			}
			page.Events = append(page.Events, row)
		}
		if row.OrgID == orgID {
			page.HeadSeq = row.ChainSeq
		}
	}
	return page, nil
}

func (s *fakeSource) NewestAttestation(context.Context, string) (*verifier.Attestation, error) {
	return s.attestation, nil
}

func (s *fakeSource) append(rows ...store.ComplianceEvent) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.rows = append(s.rows, rows...)
}

type fakeCursors struct {
	mu      sync.Mutex
	seq     map[string]int64
	streak  map[string]int
	codes   map[string]string
	advance []int64
}

func newFakeCursors() *fakeCursors {
	return &fakeCursors{seq: map[string]int64{}, streak: map[string]int{}, codes: map[string]string{}}
}

func (c *fakeCursors) Get(_ context.Context, id string) (Cursor, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return Cursor{Seq: c.seq[id], ConsecutiveFailures: c.streak[id]}, nil
}

func (c *fakeCursors) Advance(_ context.Context, d Destination, seq int64, _ int) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if seq > c.seq[d.ID] {
		c.seq[d.ID] = seq
	}
	c.streak[d.ID] = 0
	c.advance = append(c.advance, seq)
	return nil
}

func (c *fakeCursors) Fail(_ context.Context, d Destination, code string) (int, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.streak[d.ID]++
	c.codes[d.ID] = code
	return c.streak[d.ID], nil
}

type fakeLister struct{ destinations []Destination }

func (l *fakeLister) ListDestinations(context.Context, time.Time) ([]Destination, error) {
	return l.destinations, nil
}

type clock struct{ now time.Time }

func (c *clock) Now() time.Time { return c.now }

// receiver is a TLS destination that records what it was sent.
type receiver struct {
	server  *httptest.Server
	tls     *tls.Config
	mu      sync.Mutex
	headers []http.Header
	bodies  [][]byte
	// handler decides each answer by request number (1-based); nil means 200.
	status func(n int) int
	onBody func(n int)
}

func newReceiver(t *testing.T) *receiver {
	t.Helper()
	r := &receiver{}
	r.server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		body, _ := io.ReadAll(req.Body)
		r.mu.Lock()
		r.headers = append(r.headers, req.Header.Clone())
		r.bodies = append(r.bodies, body)
		n := len(r.bodies)
		r.mu.Unlock()
		if r.onBody != nil {
			r.onBody(n)
		}
		status := 200
		if r.status != nil {
			status = r.status(n)
		}
		w.WriteHeader(status)
	}))
	t.Cleanup(r.server.Close)
	pool := x509.NewCertPool()
	pool.AddCert(r.server.Certificate())
	r.tls = &tls.Config{RootCAs: pool}
	return r
}

func (r *receiver) count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.bodies)
}

type exportedLine map[string]any

func (r *receiver) lines(t *testing.T) []exportedLine {
	t.Helper()
	r.mu.Lock()
	defer r.mu.Unlock()
	var out []exportedLine
	for _, body := range r.bodies {
		out = append(out, parseLines(t, body)...)
	}
	return out
}

func parseLines(t *testing.T, body []byte) []exportedLine {
	t.Helper()
	var out []exportedLine
	scanner := bufio.NewScanner(bytes.NewReader(body))
	scanner.Buffer(make([]byte, 0, 1<<20), 2<<20)
	for scanner.Scan() {
		var line exportedLine
		if err := json.Unmarshal(scanner.Bytes(), &line); err != nil {
			t.Fatalf("line is not JSON: %v: %q", err, scanner.Text())
		}
		out = append(out, line)
	}
	return out
}

func event(org string, seq int64, name, plane string) store.ComplianceEvent {
	hash := func(tag byte) verifier.Hash {
		h := make(verifier.Hash, 32)
		for i := range h {
			h[i] = tag + byte(seq)
		}
		return h
	}
	at := time.Date(2026, 10, 24, 8, 0, 0, 0, time.UTC).Add(time.Duration(seq) * time.Second)
	return store.ComplianceEvent{
		OrgID: org, ChainSeq: seq, OccurredAt: at, IngestedAt: at.Add(time.Millisecond),
		UserID: "user-1", Plane: plane, Event: name, EventID: fmt.Sprintf("evt-%d", seq), Outcome: "ok",
		PayloadHash: hash(0x40), Details: json.RawMessage(`{"tool":"knowledge_search"}`),
		ChainVersion: 1, PrevHash: hash(0x10), RowHash: hash(0x20),
	}
}

func chainRows(org string, from, to int64) []store.ComplianceEvent {
	var rows []store.ComplianceEvent
	for seq := from; seq <= to; seq++ {
		rows = append(rows, event(org, seq, "tool.call", "model"))
	}
	return rows
}

type harness struct {
	runner  *Runner
	source  *fakeSource
	cursors *fakeCursors
	clock   *clock
	dest    *Destination
}

func newHarness(t *testing.T, rcv *receiver, kind string, rows []store.ComplianceEvent) *harness {
	t.Helper()
	dest := &Destination{
		ID: "dest-1", OrgID: "org-a", Kind: kind, URL: rcv.server.URL + "/ingest?token=URLQUERYSECRET",
		Secret: "ssssssssssssssssssssssss",
	}
	h := &harness{
		source:  &fakeSource{rows: rows},
		cursors: newFakeCursors(),
		clock:   &clock{now: time.Date(2026, 10, 24, 9, 0, 0, 0, time.UTC)},
		dest:    dest,
	}
	h.runner = &Runner{
		Destinations: &lazyLister{get: func() []Destination { return []Destination{*h.dest} }},
		Source:       h.source,
		Cursors:      h.cursors,
		Client:       NewHTTPClient(Egress{AllowPrivate: true, TLSConfig: rcv.tls}),
		Now:          h.clock.Now,
	}
	return h
}

type lazyLister struct{ get func() []Destination }

func (l *lazyLister) ListDestinations(context.Context, time.Time) ([]Destination, error) {
	return l.get(), nil
}

func gaugeValue(t *testing.T, gauge prometheus.Gauge) float64 {
	t.Helper()
	var m dto.Metric
	if err := gauge.Write(&m); err != nil {
		t.Fatal(err)
	}
	return m.GetGauge().GetValue()
}

func counterValue(t *testing.T, counter prometheus.Counter) float64 {
	t.Helper()
	var m dto.Metric
	if err := counter.Write(&m); err != nil {
		t.Fatal(err)
	}
	return m.GetCounter().GetValue()
}

// ---- delivery --------------------------------------------------------------

func TestDeliversTheChainInOrderThenAHeadRecordAndAdvancesTheCursor(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 7))
	h.source.attestation = &verifier.Attestation{Seq: 7, Hash: make(verifier.Hash, 32), KeyID: "k1", Signature: make(verifier.Hash, 64), SignedAt: h.clock.now}

	h.runner.Pass(context.Background())

	lines := rcv.lines(t)
	if len(lines) != 8 {
		t.Fatalf("%d lines, want 7 events and a head record", len(lines))
	}
	for i := 0; i < 7; i++ {
		line := lines[i]
		if line["type"] != TypeEvent || line["org_id"] != "org-a" || line["chain_seq"] != float64(i+1) {
			t.Fatalf("line %d = %v", i, line)
		}
		for _, field := range []string{"prev_hash", "row_hash", "payload_hash", "event_id", "details", "occurred_at", "ingested_at"} {
			if _, ok := line[field]; !ok {
				t.Fatalf("line %d lacks %s: the receiver cannot re-verify the row", i, field)
			}
		}
		if hash, _ := line["row_hash"].(string); len(hash) != 64 {
			t.Fatalf("row_hash is not hex: %v", line["row_hash"])
		}
	}
	head := lines[7]
	attestation, _ := head["attestation"].(map[string]any)
	if head["type"] != TypeHead || head["head_seq"] != float64(7) || attestation["chain_seq"] != float64(7) || attestation["key_id"] != "k1" {
		t.Fatalf("head record = %v", head)
	}
	if h.cursors.seq["dest-1"] != 7 {
		t.Fatalf("cursor = %d, want 7", h.cursors.seq["dest-1"])
	}

	// Caught up: the next pass sends nothing.
	before := rcv.count()
	h.runner.Pass(context.Background())
	if rcv.count() != before {
		t.Fatalf("a caught-up destination received another request")
	}
}

func TestNDJSONRequestIsSignedWithTheDestinationSecret(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	h.runner.Pass(context.Background())

	rcv.mu.Lock()
	header, body := rcv.headers[0], rcv.bodies[0]
	rcv.mu.Unlock()
	timestamp := header.Get("X-Verevon-Timestamp")
	if timestamp != strconv.FormatInt(h.clock.now.Unix(), 10) {
		t.Fatalf("timestamp = %q", timestamp)
	}
	mac := hmac.New(sha256.New, []byte(h.dest.Secret))
	mac.Write([]byte(timestamp + "."))
	mac.Write(body)
	if want := "sha256=" + hex.EncodeToString(mac.Sum(nil)); header.Get("X-Verevon-Signature") != want {
		t.Fatalf("signature = %q, want %q", header.Get("X-Verevon-Signature"), want)
	}
	if header.Get("Content-Type") != "application/x-ndjson" {
		t.Fatalf("content type = %q", header.Get("Content-Type"))
	}
	if strings.Contains(string(body), h.dest.Secret) {
		t.Fatal("the secret is inside the body it signs")
	}
	// A tampered body no longer verifies.
	tampered := hmac.New(sha256.New, []byte(h.dest.Secret))
	tampered.Write([]byte(timestamp + "."))
	tampered.Write(append(body[:len(body)-2:len(body)-2], 'X', '\n'))
	if "sha256="+hex.EncodeToString(tampered.Sum(nil)) == header.Get("X-Verevon-Signature") {
		t.Fatal("signature does not cover the body")
	}
}

func TestHECRequestUsesTheSplunkTokenAndWrapsEachRecord(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "splunk_hec", chainRows("org-a", 1, 2))
	h.runner.Pass(context.Background())

	rcv.mu.Lock()
	header := rcv.headers[0]
	rcv.mu.Unlock()
	if header.Get("Authorization") != "Splunk "+h.dest.Secret {
		t.Fatalf("authorization = %q", header.Get("Authorization"))
	}
	if header.Get("X-Verevon-Signature") != "" {
		t.Fatal("a HEC request carries the token, not an HMAC")
	}
	lines := rcv.lines(t)
	if len(lines) != 3 {
		t.Fatalf("%d lines, want 2 events and the closing head record", len(lines))
	}
	first := lines[0]
	inner, _ := first["event"].(map[string]any)
	if first["sourcetype"] != hecSourceType || first["source"] != hecSource || inner["type"] != TypeEvent || inner["chain_seq"] != float64(1) {
		t.Fatalf("hec envelope = %v", first)
	}
	if _, ok := first["time"].(float64); !ok {
		t.Fatalf("hec envelope has no time: %v", first)
	}
}

// ---- at-least-once ----------------------------------------------------------

func TestACursorAdvancesOnlyOnA2xxAndARetryRedeliversTheSameBatch(t *testing.T) {
	rcv := newReceiver(t)
	rcv.status = func(n int) int {
		switch n {
		case 1:
			return 500
		case 2:
			return 503
		}
		return 200
	}
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 5))

	h.runner.Pass(context.Background())
	if h.cursors.seq["dest-1"] != 0 || h.cursors.codes["dest-1"] != "http_500" || h.cursors.streak["dest-1"] != 1 {
		t.Fatalf("after a 500: cursor=%d code=%q streak=%d", h.cursors.seq["dest-1"], h.cursors.codes["dest-1"], h.cursors.streak["dest-1"])
	}

	// Inside the 5 s backoff nothing is sent.
	h.clock.now = h.clock.now.Add(3 * time.Second)
	h.runner.Pass(context.Background())
	if rcv.count() != 1 {
		t.Fatalf("a destination in backoff was contacted (%d requests)", rcv.count())
	}

	h.clock.now = h.clock.now.Add(3 * time.Second)
	h.runner.Pass(context.Background())
	if h.cursors.seq["dest-1"] != 0 || h.cursors.streak["dest-1"] != 2 || h.cursors.codes["dest-1"] != "http_503" {
		t.Fatalf("after a 503: cursor=%d streak=%d", h.cursors.seq["dest-1"], h.cursors.streak["dest-1"])
	}

	// Second failure doubles the wait to 10 s.
	h.clock.now = h.clock.now.Add(9 * time.Second)
	h.runner.Pass(context.Background())
	if rcv.count() != 2 {
		t.Fatalf("retried before the 10 s backoff elapsed (%d requests)", rcv.count())
	}
	h.clock.now = h.clock.now.Add(2 * time.Second)
	h.runner.Pass(context.Background())

	if h.cursors.seq["dest-1"] != 5 || len(h.cursors.advance) != 1 {
		t.Fatalf("cursor=%d advances=%v, want one advance to 5", h.cursors.seq["dest-1"], h.cursors.advance)
	}
	if h.cursors.streak["dest-1"] != 0 {
		t.Fatalf("a success must reset the failure streak")
	}
	// All three deliveries carried the same events: a receiver de-duplicates on
	// (org_id, chain_seq).
	rcv.mu.Lock()
	defer rcv.mu.Unlock()
	if len(rcv.bodies) != 3 {
		t.Fatalf("%d deliveries, want 3", len(rcv.bodies))
	}
	for i, body := range rcv.bodies {
		var seqs []float64
		for _, line := range parseLines(t, body) {
			if line["type"] == TypeEvent {
				seqs = append(seqs, line["chain_seq"].(float64))
			}
		}
		if fmt.Sprint(seqs) != "[1 2 3 4 5]" {
			t.Fatalf("delivery %d carried %v", i, seqs)
		}
	}
}

func TestARedirectIsAFailureAndTheCursorStaysPut(t *testing.T) {
	rcv := newReceiver(t)
	rcv.status = func(int) int { return http.StatusFound }
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	h.runner.Pass(context.Background())
	if h.cursors.seq["dest-1"] != 0 || h.cursors.codes["dest-1"] != "redirect_refused" {
		t.Fatalf("cursor=%d code=%q", h.cursors.seq["dest-1"], h.cursors.codes["dest-1"])
	}
}

func TestRowsInsertedWhileDeliveringAreSentInOrderWithoutGapOrRepeat(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 4))
	appended := false
	rcv.onBody = func(n int) {
		if n == 1 && !appended {
			appended = true
			h.source.append(chainRows("org-a", 5, 9)...)
		}
	}
	h.runner.Pass(context.Background())

	var seqs []int64
	for _, line := range rcv.lines(t) {
		switch line["type"] {
		case TypeEvent:
			seqs = append(seqs, int64(line["chain_seq"].(float64)))
		case TypeGap:
			t.Fatalf("a gap marker appeared for contiguous rows: %v", line)
		}
	}
	if fmt.Sprint(seqs) != "[1 2 3 4 5 6 7 8 9]" {
		t.Fatalf("delivered %v, want 1..9 once each in order", seqs)
	}
	if h.cursors.seq["dest-1"] != 9 {
		t.Fatalf("cursor = %d", h.cursors.seq["dest-1"])
	}
}

func TestADrainedBacklogIsDeliveredInBatchesOfAtMost500Rows(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 1203))
	h.runner.Pass(context.Background())
	if rcv.count() != 3 {
		t.Fatalf("%d requests, want 3 batches (500, 500, 203)", rcv.count())
	}
	rcv.mu.Lock()
	defer rcv.mu.Unlock()
	want := []int{500, 500, 203}
	for i, body := range rcv.bodies {
		events := 0
		for _, line := range parseLines(t, body) {
			if line["type"] == TypeEvent {
				events++
			}
		}
		if events != want[i] {
			t.Fatalf("batch %d has %d events, want %d", i, events, want[i])
		}
		if len(body) > MaxBodyBytes {
			t.Fatalf("batch %d is %d bytes, over the cap", i, len(body))
		}
	}
	if h.cursors.seq["dest-1"] != 1203 {
		t.Fatalf("cursor = %d", h.cursors.seq["dest-1"])
	}
}

// ---- gaps -------------------------------------------------------------------

func TestMissingRowsAreMarkedWithAGapRecord(t *testing.T) {
	rcv := newReceiver(t)
	// Rows 1-2 were pruned by retention; 6-7 are missing in the middle.
	rows := append(chainRows("org-a", 3, 5), chainRows("org-a", 8, 9)...)
	h := newHarness(t, rcv, "ndjson_webhook", rows)
	h.runner.Pass(context.Background())

	var gaps []exportedLine
	var order []string
	for _, line := range rcv.lines(t) {
		switch line["type"] {
		case TypeGap:
			gaps = append(gaps, line)
			order = append(order, fmt.Sprintf("gap%v-%v", line["from_seq"], line["to_seq"]))
		case TypeEvent:
			order = append(order, fmt.Sprint(line["chain_seq"]))
		}
	}
	if len(gaps) != 2 {
		t.Fatalf("%d gap markers, want 2: %v", len(gaps), order)
	}
	if fmt.Sprint(order) != "[gap1-2 3 4 5 gap6-7 8 9]" {
		t.Fatalf("order = %v", order)
	}
	if gaps[0]["reason"] != GapMissing || gaps[0]["org_id"] != "org-a" {
		t.Fatalf("gap = %v", gaps[0])
	}
	if h.cursors.seq["dest-1"] != 9 {
		t.Fatalf("cursor = %d", h.cursors.seq["dest-1"])
	}
}

func TestTheEventFilterThinsTheExportAndLeavesAMarkerForWhatItDropped(t *testing.T) {
	rcv := newReceiver(t)
	rows := []store.ComplianceEvent{
		event("org-a", 1, "tool.call", "model"),
		event("org-a", 2, "login", "control"),
		event("org-a", 3, "login", "control"),
		event("org-a", 4, "tool.call", "control"),
		event("org-a", 5, "tool.call", "model"),
		event("org-a", 6, "login", "control"),
	}
	h := newHarness(t, rcv, "ndjson_webhook", rows)
	h.dest.EventFilter = EventFilter{Events: []string{"tool.call"}, Planes: []string{"model"}}
	h.runner.Pass(context.Background())

	var order []string
	for _, line := range rcv.lines(t) {
		switch line["type"] {
		case TypeGap:
			if line["reason"] != GapFiltered {
				t.Fatalf("gap reason = %v", line["reason"])
			}
			order = append(order, fmt.Sprintf("filtered%v-%v", line["from_seq"], line["to_seq"]))
		case TypeEvent:
			order = append(order, fmt.Sprint(line["chain_seq"]))
		}
	}
	if fmt.Sprint(order) != "[1 filtered2-4 5 filtered6-6]" {
		t.Fatalf("order = %v", order)
	}
	if h.cursors.seq["dest-1"] != 6 {
		t.Fatalf("a filtered tail must still advance the cursor, got %d", h.cursors.seq["dest-1"])
	}
}

func bigEvent(org string, seq int64, size int) store.ComplianceEvent {
	e := event(org, seq, "tool.call", "model")
	e.Details = json.RawMessage(`{"tool":"` + strings.Repeat("x", size) + `"}`)
	return e
}

func TestRowsArePackedUnderTheBodyCapAndTheRestGoInTheNextBatch(t *testing.T) {
	rcv := newReceiver(t)
	rows := []store.ComplianceEvent{bigEvent("org-a", 1, 600<<10), bigEvent("org-a", 2, 600<<10), bigEvent("org-a", 3, 100)}
	h := newHarness(t, rcv, "ndjson_webhook", rows)
	h.runner.Pass(context.Background())

	rcv.mu.Lock()
	defer rcv.mu.Unlock()
	if len(rcv.bodies) != 2 {
		t.Fatalf("%d requests, want the 600 KiB rows split across two", len(rcv.bodies))
	}
	var firstSeqs, secondSeqs []float64
	for _, line := range parseLines(t, rcv.bodies[0]) {
		if line["type"] == TypeEvent {
			firstSeqs = append(firstSeqs, line["chain_seq"].(float64))
		}
	}
	for _, line := range parseLines(t, rcv.bodies[1]) {
		if line["type"] == TypeEvent {
			secondSeqs = append(secondSeqs, line["chain_seq"].(float64))
		}
	}
	if fmt.Sprint(firstSeqs) != "[1]" || fmt.Sprint(secondSeqs) != "[2 3]" {
		t.Fatalf("batches carried %v then %v", firstSeqs, secondSeqs)
	}
	for _, body := range rcv.bodies {
		if len(body) > MaxBodyBytes {
			t.Fatalf("body of %d bytes is over the cap", len(body))
		}
	}
}

func TestARowTooLargeForAnyBodyIsMarkedNotStalledOn(t *testing.T) {
	rcv := newReceiver(t)
	rows := []store.ComplianceEvent{bigEvent("org-a", 1, 1100<<10), event("org-a", 2, "tool.call", "model")}
	h := newHarness(t, rcv, "ndjson_webhook", rows)
	h.runner.Pass(context.Background())

	var order []string
	for _, line := range rcv.lines(t) {
		switch line["type"] {
		case TypeGap:
			order = append(order, fmt.Sprintf("gap:%v:%v-%v", line["reason"], line["from_seq"], line["to_seq"]))
		case TypeEvent:
			order = append(order, fmt.Sprint(line["chain_seq"]))
		}
	}
	if fmt.Sprint(order) != "[gap:row_too_large:1-1 2]" {
		t.Fatalf("order = %v", order)
	}
	if h.cursors.seq["dest-1"] != 2 {
		t.Fatalf("cursor = %d", h.cursors.seq["dest-1"])
	}
}

// ---- configuration faults ---------------------------------------------------

func TestADestinationTheGuardRefusesIsRecordedAndNeverContacted(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	for url, code := range map[string]string{
		"http://siem.example.com/ingest":    "url_invalid",
		"https://user:pw@siem.example.com/": "url_invalid",
		"https:///nohost":                   "url_invalid",
	} {
		h.dest.URL = url
		h.runner.Pass(context.Background())
		if h.cursors.codes["dest-1"] != code {
			t.Errorf("%s: code = %q, want %q", url, h.cursors.codes["dest-1"], code)
		}
		h.clock.now = h.clock.now.Add(time.Hour)
	}
	h.dest.Kind = "carrier_pigeon"
	h.dest.URL = rcv.server.URL
	h.runner.Pass(context.Background())
	if h.cursors.codes["dest-1"] != "kind_invalid" {
		t.Errorf("unknown kind: code = %q", h.cursors.codes["dest-1"])
	}
	if rcv.count() != 0 || h.cursors.seq["dest-1"] != 0 {
		t.Fatalf("an invalid destination was contacted or advanced")
	}
}

func TestOneFailingDestinationDoesNotHoldBackAnother(t *testing.T) {
	bad, good := newReceiver(t), newReceiver(t)
	bad.status = func(int) int { return 500 }
	h := newHarness(t, good, "ndjson_webhook", chainRows("org-a", 1, 3))
	second := *h.dest
	second.ID = "dest-bad"
	second.URL = bad.server.URL
	first := *h.dest
	h.runner.Destinations = &fakeLister{destinations: []Destination{second, first}}
	h.runner.Client = NewHTTPClient(Egress{AllowPrivate: true, TLSConfig: mergedRoots(bad, good)})
	h.runner.Pass(context.Background())

	if h.cursors.seq["dest-1"] != 3 || h.cursors.seq["dest-bad"] != 0 {
		t.Fatalf("cursors: good=%d bad=%d", h.cursors.seq["dest-1"], h.cursors.seq["dest-bad"])
	}
}

func mergedRoots(receivers ...*receiver) *tls.Config {
	pool := x509.NewCertPool()
	for _, r := range receivers {
		pool.AddCert(r.server.Certificate())
	}
	return &tls.Config{RootCAs: pool}
}

// ---- observability ----------------------------------------------------------

func TestLagGrowsWhileADestinationIsDownAndClearsOnceItIsCaughtUp(t *testing.T) {
	rcv := newReceiver(t)
	var down = true
	rcv.status = func(int) int {
		if down {
			return 503
		}
		return 200
	}
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 3))
	lag := exportLag.WithLabelValues("ndjson_webhook")

	h.clock.now = event("org-a", 1, "", "").IngestedAt.Add(2 * time.Minute)
	h.runner.Pass(context.Background())
	first := gaugeValue(t, lag)
	if first < 119 || first > 121 {
		t.Fatalf("lag after 2 minutes down = %v", first)
	}
	h.clock.now = h.clock.now.Add(20 * time.Minute)
	h.runner.Pass(context.Background())
	if second := gaugeValue(t, lag); second < 20*60 {
		t.Fatalf("lag after 22 minutes down = %v, want it to keep growing", second)
	}

	down = false
	h.clock.now = h.clock.now.Add(time.Hour)
	h.runner.Pass(context.Background())
	if gaugeValue(t, lag) != 0 || h.cursors.seq["dest-1"] != 3 {
		t.Fatalf("lag=%v cursor=%d after recovery", gaugeValue(t, lag), h.cursors.seq["dest-1"])
	}
}

func TestBatchAndDestinationMetricsCountOutcomes(t *testing.T) {
	rcv := newReceiver(t)
	rcv.status = func(n int) int {
		if n == 1 {
			return 500
		}
		return 200
	}
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	okBefore := counterValue(t, exportBatches.WithLabelValues(resultOK))
	failedBefore := counterValue(t, exportBatches.WithLabelValues(resultFailed))

	h.runner.Pass(context.Background())
	if got := gaugeValue(t, exportDestinations.WithLabelValues(stateBackingOff)); got != 1 {
		t.Fatalf("backing_off destinations = %v", got)
	}
	h.clock.now = h.clock.now.Add(time.Minute)
	h.runner.Pass(context.Background())

	if counterValue(t, exportBatches.WithLabelValues(resultFailed))-failedBefore != 1 ||
		counterValue(t, exportBatches.WithLabelValues(resultOK))-okBefore != 1 {
		t.Fatalf("batch counters did not record one failure and one success")
	}
	if got := gaugeValue(t, exportDestinations.WithLabelValues(stateActive)); got != 1 {
		t.Fatalf("active destinations = %v", got)
	}
}

func TestExporterLogsCarryNeitherSecretNorURLNorBody(t *testing.T) {
	var logs bytes.Buffer
	saved := log.Logger
	log.Logger = zerolog.New(&logs)
	t.Cleanup(func() { log.Logger = saved })

	rcv := newReceiver(t)
	rcv.status = func(int) int { return 500 }
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	h.runner.Pass(context.Background())

	// Also an internal error path and an invalid destination.
	h.source.pageErr = errors.New("database unavailable")
	h.clock.now = h.clock.now.Add(time.Hour)
	h.runner.Pass(context.Background())
	h.source.pageErr = nil
	h.dest.URL = "http://leaky-host.example.com/path?token=URLQUERYSECRET"
	h.clock.now = h.clock.now.Add(time.Hour)
	h.runner.Pass(context.Background())

	text := logs.String()
	if text == "" {
		t.Fatal("the exporter logged nothing for a failing destination")
	}
	for _, leak := range []string{h.dest.Secret, "URLQUERYSECRET", "leaky-host", "127.0.0.1", "knowledge_search", "evt-1", "Authorization", "X-Verevon-Signature"} {
		if strings.Contains(text, leak) {
			t.Fatalf("exporter log leaks %q:\n%s", leak, text)
		}
	}
	if !strings.Contains(text, `"code":"http_500"`) || !strings.Contains(text, `"code":"url_invalid"`) {
		t.Fatalf("failure codes missing from the log:\n%s", text)
	}
}

func TestADestinationDroppedByOrgCoreIsForgottenButItsCursorIsKept(t *testing.T) {
	rcv := newReceiver(t)
	h := newHarness(t, rcv, "ndjson_webhook", chainRows("org-a", 1, 2))
	h.runner.Pass(context.Background())
	if len(h.runner.state) != 1 {
		t.Fatalf("state = %v", h.runner.state)
	}
	h.runner.Destinations = &fakeLister{}
	h.runner.Pass(context.Background())
	if len(h.runner.state) != 0 {
		t.Fatalf("state for a disabled destination was kept: %v", h.runner.state)
	}
	if h.cursors.seq["dest-1"] != 2 {
		t.Fatalf("the cursor must survive a disable: %d", h.cursors.seq["dest-1"])
	}
}
