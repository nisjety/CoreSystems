package cost

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

// The real broker, the real Rust producers, the real consumer: the events that
// services/usage-ledger-rs/tests/jetstream_live.rs published (signed by the three producers'
// real signers with throwaway keys, through the real JetStreamLedger) are read by this
// package's durable consumer, verified against the public keys that test wrote, and recorded
// once each. Gated: it needs that test to have run first against the same broker.
//
//	COST_LIVE_NATS_URL   the broker
//	COST_LIVE_KEYS_DIR   the directory USAGE_LEDGER_TEST_KEYS_DIR named
//
// Without them it skips, and FAILS when REQUIRE_CONTRACT is set.
func TestLiveEventsPublishedByTheRustProducersAreRecordedOnceByTheConsumer(t *testing.T) {
	url, dir := strings.TrimSpace(os.Getenv("COST_LIVE_NATS_URL")), strings.TrimSpace(os.Getenv("COST_LIVE_KEYS_DIR"))
	if url == "" || dir == "" {
		if os.Getenv("REQUIRE_CONTRACT") != "" {
			t.Fatal("COST_LIVE_NATS_URL and COST_LIVE_KEYS_DIR are not set while REQUIRE_CONTRACT is")
		}
		t.Skip("COST_LIVE_NATS_URL and COST_LIVE_KEYS_DIR are required (run services/usage-ledger-rs/tests/jetstream_live.rs first)")
	}
	raw, err := os.ReadFile(filepath.Join(dir, "expected-events.txt"))
	if err != nil {
		t.Fatalf("the Rust live test has not run against %s: %v", dir, err)
	}
	expected, err := strconv.Atoi(strings.TrimSpace(string(raw)))
	if err != nil || expected < 1 {
		t.Fatalf("bad expectation %q", raw)
	}
	read := func(name string) []byte {
		pem, err := os.ReadFile(filepath.Join(dir, name+".pub"))
		if err != nil {
			t.Fatalf("read %s key: %v", name, err)
		}
		return pem
	}
	registry, err := NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: read("embedding-engine-rs"),
		RetrievalPublicKeyPEM: read("retrieval-engine-rs"),
		GraphPublicKeyPEM:     read("graph-index-rs"),
	}, 1000)
	if err != nil {
		t.Fatalf("registry from the producers' public keys: %v", err)
	}

	conn, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer conn.Close()
	js, err := conn.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	// The stream the Rust producers created must be the one this service would have created.
	if err := EnsureStream(js); err != nil {
		t.Fatalf("the consumer refuses the stream the producers created: %v", err)
	}
	// Delete a consumer left by an earlier run, so every message is read from the start.
	_ = js.DeleteConsumer(StreamName, DurableName)

	store := newUniqueStore()
	stop, err := NewSignedConsumer(store, conn, registry).Start(context.Background())
	if err != nil {
		t.Fatalf("start: %v", err)
	}
	defer stop()
	waitFor(t, "the Rust producers' events to be recorded", func() bool { return store.count() >= expected })
	time.Sleep(300 * time.Millisecond)
	if store.count() != expected {
		t.Fatalf("recorded %d rows for %d events", store.count(), expected)
	}

	byFeature := map[string][]any{}
	store.mu.Lock()
	for _, row := range store.rows {
		byFeature[row[colFeature].(string)] = row
	}
	store.mu.Unlock()
	for feature, want := range map[string]struct {
		org    string
		input  int64
		output int64
		zdr    bool
		basis  string
	}{
		"contextual-retrieval": {"org-live-a", 1200, 60, false, "metered"},
		"embedding":            {"org-live-a", 900, 0, true, "metered"},
		"graph-extraction":     {"org-live-b", 2100, 480, false, "metered"},
		"web-rerank":           {"org-live-a", 0, 0, false, "metered"},
	} {
		row, ok := byFeature[feature]
		if !ok {
			t.Errorf("no row for %s", feature)
			continue
		}
		if row[colOrgID] != want.org || row[colInputTokens] != want.input || row[colOutputTokens] != want.output || row[colZDR] != want.zdr {
			t.Errorf("%s: org=%v input=%v output=%v zdr=%v, want %+v", feature, row[colOrgID], row[colInputTokens], row[colOutputTokens], row[colZDR], want)
		}
		if row[colUserID] != nil {
			t.Errorf("%s: a usage row names a user", feature)
		}
	}
}

// The other order of a deploy: this service starts first and creates the stream, and a producer
// that starts later must accept it. Run this against an EMPTY broker (it deletes the stream
// first), then run services/usage-ledger-rs/tests/jetstream_live.rs against the same broker: its
// ensure_stream has to find the stream this service made acceptable, and its assertions on the
// stream's settings have to hold for what this service created.
func TestLiveStreamCreatedHereIsAcceptedByTheProducers(t *testing.T) {
	url := strings.TrimSpace(os.Getenv("COST_LIVE_CREATE_STREAM_URL"))
	if url == "" {
		if os.Getenv("REQUIRE_CONTRACT") != "" {
			t.Fatal("COST_LIVE_CREATE_STREAM_URL is not set while REQUIRE_CONTRACT is")
		}
		t.Skip("COST_LIVE_CREATE_STREAM_URL is required (an empty disposable broker)")
	}
	conn, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer conn.Close()
	js, err := conn.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	_ = js.DeleteStream(StreamName)
	if err := EnsureStream(js); err != nil {
		t.Fatalf("create the stream: %v", err)
	}
}
