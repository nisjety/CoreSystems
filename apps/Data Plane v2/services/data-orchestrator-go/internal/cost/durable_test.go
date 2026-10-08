package cost

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
)

// ── a real broker, and a store that has the one property of the real one that matters ───

func startBroker(t *testing.T) (*server.Server, *nats.Conn, nats.JetStreamContext) {
	t.Helper()
	instance, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1, NoLog: true, NoSigs: true})
	if err != nil {
		t.Fatalf("start the test broker: %v", err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("the test broker did not become ready")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })
	conn, err := nats.Connect(instance.ClientURL())
	if err != nil {
		t.Fatalf("connect to the test broker: %v", err)
	}
	t.Cleanup(conn.Close)
	js, err := conn.JetStream()
	if err != nil {
		t.Fatalf("JetStream: %v", err)
	}
	return instance, conn, js
}

// uniqueStore stands in for cost_events in one respect only: the unique index on the
// idempotency key (ON CONFLICT DO NOTHING). It does not run the SQL; the statement is run
// against a real Postgres by integration_test.go. It can be made to fail.
type uniqueStore struct {
	mu       sync.Mutex
	rows     [][]any
	seen     map[any]bool
	failures int   // fail this many writes first
	failWith error // with this error
}

func newUniqueStore() *uniqueStore { return &uniqueStore{seen: map[any]bool{}} }

func (s *uniqueStore) Exec(_ context.Context, _ string, args ...any) (pgconn.CommandTag, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.failures > 0 {
		s.failures--
		return pgconn.NewCommandTag(""), s.failWith
	}
	if key := args[colIdempotencyKey]; key != nil {
		if s.seen[key] {
			return pgconn.NewCommandTag("INSERT 0 0"), nil
		}
		s.seen[key] = true
	}
	s.rows = append(s.rows, append([]any(nil), args...))
	return pgconn.NewCommandTag("INSERT 0 1"), nil
}

func (s *uniqueStore) count() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.rows)
}

func (s *uniqueStore) keys() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	keys := make([]string, 0, len(s.rows))
	for _, row := range s.rows {
		keys = append(keys, fmt.Sprint(row[colIdempotencyKey]))
	}
	return keys
}

func waitFor(t *testing.T, what string, condition func() bool) {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for !condition() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func fastRetries(t *testing.T) {
	t.Helper()
	previous := retryStep
	retryStep = 20 * time.Millisecond
	t.Cleanup(func() { retryStep = previous })
}

func consumerInfo(t *testing.T, js nats.JetStreamContext) *nats.ConsumerInfo {
	t.Helper()
	info, err := js.ConsumerInfo(StreamName, DurableName)
	if err != nil {
		t.Fatalf("consumer info: %v", err)
	}
	return info
}

// ── the stream ───────────────────────────────────────────────────────────────

func TestEnsureStreamCreatesTheLedgerStreamAndIsIdempotent(t *testing.T) {
	_, _, js := startBroker(t)
	for i := 0; i < 2; i++ {
		if err := EnsureStream(js); err != nil {
			t.Fatalf("ensure %d: %v", i+1, err)
		}
	}
	info, err := js.StreamInfo(StreamName)
	if err != nil {
		t.Fatalf("stream info: %v", err)
	}
	config := info.Config
	if config.Retention != nats.LimitsPolicy || config.Storage != nats.FileStorage ||
		config.MaxAge != 7*24*time.Hour || config.MaxBytes != 512*1024*1024 ||
		config.Duplicates != 2*time.Minute || config.Discard != nats.DiscardOld ||
		len(config.Subjects) != 1 || config.Subjects[0] != "dataplane.cost.ledger" {
		t.Fatalf("the ledger stream is not as defined: %+v", config)
	}
}

// The values are asserted from the Rust side too (usage-ledger-rs stream.rs). Producers create
// the stream as well, so a producer that starts first must create the same one.
func TestStreamDefinitionMatchesTheProducersDefinition(t *testing.T) {
	if StreamName != "DATAPLANE_COST" || SubjectCostLedger != "dataplane.cost.ledger" || DurableName != "data-orchestrator-cost" {
		t.Fatalf("names changed: %s %s %s", StreamName, SubjectCostLedger, DurableName)
	}
	if streamMaxAge != 7*24*time.Hour || streamMaxBytes != 512*1024*1024 || streamDuplicateWindow != 2*time.Minute {
		t.Fatal("limits changed: change usage-ledger-rs/src/stream.rs with them")
	}
}

// Retention cannot be changed in place, and every other policy discards an event that arrives
// while no consumer is bound: the producer-before-consumer order of a deploy would lose it.
func TestEnsureStreamRefusesAStreamThatWouldDiscardEvents(t *testing.T) {
	for name, retention := range map[string]nats.RetentionPolicy{"interest": nats.InterestPolicy, "work queue": nats.WorkQueuePolicy} {
		t.Run(name, func(t *testing.T) {
			_, _, js := startBroker(t)
			config := StreamConfig()
			config.Retention = retention
			if _, err := js.AddStream(config); err != nil {
				t.Fatalf("pre-create: %v", err)
			}
			err := EnsureStream(js)
			if !errors.Is(err, errStreamUnusable) {
				t.Fatalf("EnsureStream on a %s stream = %v, want it refused", name, err)
			}
		})
	}
	t.Run("memory storage", func(t *testing.T) {
		_, _, js := startBroker(t)
		config := StreamConfig()
		config.Storage = nats.MemoryStorage
		if _, err := js.AddStream(config); err != nil {
			t.Fatalf("pre-create: %v", err)
		}
		if err := EnsureStream(js); !errors.Is(err, errStreamUnusable) {
			t.Fatalf("EnsureStream on a memory stream = %v, want it refused", err)
		}
	})
}

func TestEnsureStreamRaisesWhatIsBehindAndKeepsWhatAnOperatorWidened(t *testing.T) {
	_, _, js := startBroker(t)
	behind := StreamConfig()
	behind.MaxAge = 24 * time.Hour
	behind.Duplicates = time.Second
	if _, err := js.AddStream(behind); err != nil {
		t.Fatalf("pre-create: %v", err)
	}
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure: %v", err)
	}
	info, _ := js.StreamInfo(StreamName)
	if info.Config.MaxAge != streamMaxAge || info.Config.Duplicates != streamDuplicateWindow {
		t.Fatalf("limits were not converged: %+v", info.Config)
	}

	widened := info.Config
	widened.MaxAge = 30 * 24 * time.Hour
	if _, err := js.UpdateStream(&widened); err != nil {
		t.Fatalf("operator widened: %v", err)
	}
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure after widening: %v", err)
	}
	info, _ = js.StreamInfo(StreamName)
	if info.Config.MaxAge != 30*24*time.Hour {
		t.Fatalf("an operator's wider limit was lowered to %v", info.Config.MaxAge)
	}
}

func TestEnsureStreamSaysSoWhenTheSubjectBelongsToAnotherStream(t *testing.T) {
	_, _, js := startBroker(t)
	if _, err := js.AddStream(&nats.StreamConfig{Name: "SOMETHING_ELSE", Subjects: []string{SubjectCostLedger}}); err != nil {
		t.Fatalf("pre-create: %v", err)
	}
	if err := EnsureStream(js); err == nil {
		t.Fatal("EnsureStream succeeded although another stream owns the subject")
	}
}

// ── the durable consumer, against a real broker ──────────────────────────────

func startConsumer(t *testing.T, conn *nats.Conn, store eventStore, registry *VerifierRegistry) func() {
	t.Helper()
	cleanup, err := NewSignedConsumer(store, conn, registry).Start(context.Background())
	if err != nil {
		t.Fatalf("start the durable consumer: %v", err)
	}
	t.Cleanup(cleanup)
	return cleanup
}

// The point of the stream. Events published while no consumer exists (a producer deployed
// first, a consumer down for a deploy) are read when it starts, however old their envelopes
// are by then, and are written once. Both ways a producer can publish are captured: the
// producer with the stream, and a core publish from a producer that has not been upgraded.
func TestEventsPublishedWhileTheConsumerWasDownAreRecordedOnceWhenItStarts(t *testing.T) {
	_, conn, js := startBroker(t)
	fx := loadFixture(t)
	if err := EnsureStream(js); err != nil { // a producer creates the stream before the consumer exists
		t.Fatalf("producer ensure: %v", err)
	}
	for i, tc := range fx.Cases {
		if i%2 == 0 {
			if _, err := js.Publish(SubjectCostLedger, []byte(tc.Envelope), nats.MsgId(tc.Name)); err != nil {
				t.Fatalf("publish %s with the stream: %v", tc.Name, err)
			}
		} else if err := conn.Publish(SubjectCostLedger, []byte(tc.Envelope)); err != nil {
			t.Fatalf("publish %s on the core subject: %v", tc.Name, err)
		}
	}
	if err := conn.Flush(); err != nil {
		t.Fatalf("flush: %v", err)
	}

	// Ten minutes later: every envelope is past its two-minute lifetime.
	store := newUniqueStore()
	startConsumer(t, conn, store, fx.registry(t, 10*time.Minute))
	waitFor(t, "every event to be recorded", func() bool { return store.count() == len(fx.Cases) })
	waitFor(t, "every event to be acknowledged", func() bool { return consumerInfo(t, js).NumAckPending == 0 && consumerInfo(t, js).NumPending == 0 })

	if got := len(store.keys()); got != len(fx.Cases) {
		t.Fatalf("recorded %d rows for %d events", got, len(fx.Cases))
	}
}

// Restart: what was acknowledged is not delivered again, and an event whose acknowledgement
// was lost is delivered again and is one row.
func TestARestartedConsumerRecordsNothingTwice(t *testing.T) {
	_, conn, js := startBroker(t)
	fx := loadFixture(t)
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure: %v", err)
	}
	for _, tc := range fx.Cases {
		if _, err := js.Publish(SubjectCostLedger, []byte(tc.Envelope), nats.MsgId(tc.Name)); err != nil {
			t.Fatalf("publish: %v", err)
		}
	}

	store := newUniqueStore()
	stop := startConsumer(t, conn, store, fx.registry(t, 10*time.Minute))
	waitFor(t, "the first run to record everything", func() bool { return store.count() == len(fx.Cases) })
	waitFor(t, "the first run to acknowledge everything", func() bool { return consumerInfo(t, js).NumAckPending == 0 })
	stop()

	// The same events again, as a producer's retry after a lost PubAck would publish them
	// (a different broker message id, so the broker keeps them), and a new process with an
	// empty replay cache.
	for _, tc := range fx.Cases[:2] {
		if _, err := js.Publish(SubjectCostLedger, []byte(tc.Envelope), nats.MsgId(tc.Name+"-republished")); err != nil {
			t.Fatalf("republish: %v", err)
		}
	}
	startConsumer(t, conn, store, fx.registry(t, 10*time.Minute))
	waitFor(t, "the republished events to be read", func() bool { return consumerInfo(t, js).NumPending == 0 && consumerInfo(t, js).NumAckPending == 0 })
	time.Sleep(200 * time.Millisecond)

	if store.count() != len(fx.Cases) {
		t.Fatalf("after a restart and a republish there are %d rows, want %d: an event was recorded twice or lost", store.count(), len(fx.Cases))
	}
}

// A database that is down is not a reason to lose an event. The delivery is refused, tried
// again, and recorded once when the database returns.
func TestEventsAreRetriedUntilTheDatabaseTakesThemAndThenRecordedOnce(t *testing.T) {
	fastRetries(t)
	_, conn, js := startBroker(t)
	fx := loadFixture(t)
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure: %v", err)
	}
	store := newUniqueStore()
	store.failures, store.failWith = 5, errors.New("database unavailable")
	startConsumer(t, conn, store, fx.registry(t, 10*time.Minute))
	if _, err := js.Publish(SubjectCostLedger, []byte(fx.Cases[0].Envelope)); err != nil {
		t.Fatalf("publish: %v", err)
	}
	waitFor(t, "the event to be recorded after the database recovered", func() bool { return store.count() == 1 })
	waitFor(t, "the event to be acknowledged", func() bool { return consumerInfo(t, js).NumAckPending == 0 })
	time.Sleep(200 * time.Millisecond)
	if store.count() != 1 {
		t.Fatalf("recorded %d rows, want exactly 1", store.count())
	}
	if redelivered := consumerInfo(t, js).NumRedelivered; redelivered != 0 {
		t.Fatalf("%d messages still counted as redelivered after being acknowledged", redelivered)
	}
}

// What cannot become a row is terminated, not retried for ever, and does not stop what can:
// the valid event behind it is recorded.
func TestPoisonIsTerminatedAndTheEventBehindItIsRecorded(t *testing.T) {
	_, conn, js := startBroker(t)
	fx := loadFixture(t)
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure: %v", err)
	}
	for _, poison := range []string{
		`not json at all`,
		`{"authorization":"Bearer a.b.c","data":"e30"}`,
		`{"event_type":"embedding","org_id":"org-a","zdr":false}`, // a raw domain event, unsigned
	} {
		if _, err := js.Publish(SubjectCostLedger, []byte(poison)); err != nil {
			t.Fatalf("publish poison: %v", err)
		}
	}
	if _, err := js.Publish(SubjectCostLedger, []byte(fx.Cases[0].Envelope)); err != nil {
		t.Fatalf("publish valid: %v", err)
	}
	store := newUniqueStore()
	startConsumer(t, conn, store, fx.registry(t, 10*time.Minute))
	waitFor(t, "the valid event to be recorded", func() bool { return store.count() == 1 })
	// Terminated means the broker holds nothing for it: not pending, not waiting for an
	// acknowledgement that would come after a minute, not on its way to being redelivered.
	waitFor(t, "every delivery to be settled", func() bool {
		info := consumerInfo(t, js)
		return info.NumAckPending == 0 && info.NumPending == 0
	})
	if info := consumerInfo(t, js); info.NumRedelivered != 0 {
		t.Fatalf("poison was redelivered %d times", info.NumRedelivered)
	}
	// And it is still in the stream, for seven days, so it can be looked at.
	stream, err := js.StreamInfo(StreamName)
	if err != nil || stream.State.Msgs != 4 {
		t.Fatalf("the stream holds %v messages (%v), want 4: terminated events are kept for inspection", stream.State.Msgs, err)
	}
}

// Replicas share the durable as a queue group: every event is recorded by exactly one of them.
func TestTwoReplicasShareTheStreamAndRecordEachEventOnce(t *testing.T) {
	_, conn, js := startBroker(t)
	if err := EnsureStream(js); err != nil {
		t.Fatalf("ensure: %v", err)
	}
	p := newTestProducers(t)
	second, err := nats.Connect(conn.ConnectedUrl())
	if err != nil {
		t.Fatalf("second connection: %v", err)
	}
	t.Cleanup(second.Close)

	store := newUniqueStore()
	startConsumer(t, conn, store, p.registry)
	startConsumer(t, second, store, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
	const events = 40
	for i := 0; i < events; i++ {
		envelope := signV2(t, v2Spec{producer: p.embedding, eventType: "llm", feature: "contextual-retrieval", jti: fmt.Sprintf("replica-%d", i)})
		if _, err := js.Publish(SubjectCostLedger, envelope); err != nil {
			t.Fatalf("publish: %v", err)
		}
	}
	waitFor(t, "every event to be recorded", func() bool { return store.count() == events })
	waitFor(t, "every event to be settled", func() bool { return consumerInfo(t, js).NumAckPending == 0 })
	time.Sleep(200 * time.Millisecond)
	if store.count() != events {
		t.Fatalf("recorded %d rows for %d events", store.count(), events)
	}
}

// ── the decisions, without a broker ──────────────────────────────────────────

type fakeDelivery struct {
	payload  []byte
	attempts uint64
	acks     int
	naks     []time.Duration
	terms    int
	ackErr   error
}

func (d *fakeDelivery) Payload() []byte           { return d.payload }
func (d *fakeDelivery) Attempts() (uint64, error) { return d.attempts, nil }
func (d *fakeDelivery) Sequence() uint64          { return 7 }
func (d *fakeDelivery) Ack() error                { d.acks++; return d.ackErr }
func (d *fakeDelivery) Term() error               { d.terms++; return nil }
func (d *fakeDelivery) NakWithDelay(w time.Duration) error {
	d.naks = append(d.naks, w)
	return nil
}

func TestEachDeliveryIsSettledTheWayItsOutcomeRequires(t *testing.T) {
	p := newTestProducers(t)
	valid := func(jti string) []byte {
		return signV2(t, v2Spec{producer: p.embedding, eventType: "llm", feature: "contextual-retrieval", jti: jti})
	}
	t.Run("recorded then acknowledged, and only then", func(t *testing.T) {
		store := &recordingStore{}
		consumer := NewSignedConsumer(store, nil, p.registry)
		d := &fakeDelivery{payload: valid("settle-ok"), attempts: 1}
		if got := consumer.handleDelivery(context.Background(), d); got != outcomePersisted {
			t.Fatalf("outcome = %s", got)
		}
		if len(store.calls) != 1 || d.acks != 1 || d.terms != 0 || len(d.naks) != 0 {
			t.Fatalf("rows=%d acks=%d terms=%d naks=%v", len(store.calls), d.acks, d.terms, d.naks)
		}
	})
	t.Run("a redelivery after a lost acknowledgement is acknowledged without a second write", func(t *testing.T) {
		store := &recordingStore{}
		consumer := NewSignedConsumer(store, nil, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
		payload := valid("settle-dup")
		first, again := &fakeDelivery{payload: payload, attempts: 1}, &fakeDelivery{payload: payload, attempts: 2}
		consumer.handleDelivery(context.Background(), first)
		if got := consumer.handleDelivery(context.Background(), again); got != outcomeDuplicate {
			t.Fatalf("outcome = %s", got)
		}
		if len(store.calls) != 1 || again.acks != 1 || again.terms != 0 {
			t.Fatalf("rows=%d acks=%d terms=%d", len(store.calls), again.acks, again.terms)
		}
	})
	t.Run("not authentic is terminated and nothing is written", func(t *testing.T) {
		store := &recordingStore{}
		consumer := NewSignedConsumer(store, nil, p.registry)
		d := &fakeDelivery{payload: []byte(`{"event_type":"llm"}`), attempts: 1}
		if got := consumer.handleDelivery(context.Background(), d); got != outcomeInvalid {
			t.Fatalf("outcome = %s", got)
		}
		if len(store.calls) != 0 || d.terms != 1 || d.acks != 0 || len(d.naks) != 0 {
			t.Fatalf("rows=%d acks=%d terms=%d naks=%v", len(store.calls), d.acks, d.terms, d.naks)
		}
	})
	t.Run("a database that is down is not acknowledged and the replay is released so the redelivery can succeed", func(t *testing.T) {
		store := &recordingStore{err: errors.New("connection refused")}
		consumer := NewSignedConsumer(store, nil, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
		payload := valid("settle-down")
		d := &fakeDelivery{payload: payload, attempts: 3}
		if got := consumer.handleDelivery(context.Background(), d); got != outcomeTransient {
			t.Fatalf("outcome = %s", got)
		}
		if d.acks != 0 || d.terms != 0 || len(d.naks) != 1 || d.naks[0] != 3*retryStep {
			t.Fatalf("acks=%d terms=%d naks=%v, want one nak after %v", d.acks, d.terms, d.naks, 3*retryStep)
		}
		store.err = nil
		retry := &fakeDelivery{payload: payload, attempts: 4}
		if got := consumer.handleDelivery(context.Background(), retry); got != outcomePersisted || retry.acks != 1 {
			t.Fatalf("the redelivery: outcome=%s acks=%d", got, retry.acks)
		}
	})
	t.Run("the pause grows with the attempts and is capped, never giving up", func(t *testing.T) {
		consumer := NewSignedConsumer(&recordingStore{err: errors.New("down")}, nil, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
		for _, attempts := range []uint64{1, 5, 1000} {
			d := &fakeDelivery{payload: valid(fmt.Sprintf("settle-cap-%d", attempts)), attempts: attempts}
			consumer.handleDelivery(context.Background(), d)
			want := min(time.Duration(attempts)*retryStep, maxRetryDelay)
			if d.terms != 0 || len(d.naks) != 1 || d.naks[0] != want {
				t.Fatalf("attempt %d: terms=%d naks=%v, want one nak after %v", attempts, d.terms, d.naks, want)
			}
		}
	})
	t.Run("a row the database refuses for what is in it is terminated, not retried for ever", func(t *testing.T) {
		for _, code := range []string{"22003", "23514", "23502"} {
			store := &recordingStore{err: &pgconn.PgError{Code: code, Message: "refused"}}
			consumer := NewSignedConsumer(store, nil, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
			d := &fakeDelivery{payload: valid("settle-refused-" + code), attempts: 1}
			if got := consumer.handleDelivery(context.Background(), d); got != outcomePermanent || d.terms != 1 || d.acks != 0 || len(d.naks) != 0 {
				t.Fatalf("code %s: outcome=%s terms=%d acks=%d naks=%v", code, got, d.terms, d.acks, d.naks)
			}
		}
		// Other classes of error are the database being unwell, not the event being wrong.
		for _, code := range []string{"08006", "40001", "57P01", "53300"} {
			store := &recordingStore{err: &pgconn.PgError{Code: code, Message: "unwell"}}
			consumer := NewSignedConsumer(store, nil, newTestRegistryOf(t, p.embedding, p.retrieval, p.graph))
			d := &fakeDelivery{payload: valid("settle-unwell-" + code), attempts: 1}
			if got := consumer.handleDelivery(context.Background(), d); got != outcomeTransient || d.terms != 0 {
				t.Fatalf("code %s: outcome=%s terms=%d", code, got, d.terms)
			}
		}
	})
	t.Run("a failed acknowledgement after the write is harmless", func(t *testing.T) {
		store := &recordingStore{}
		consumer := NewSignedConsumer(store, nil, p.registry)
		d := &fakeDelivery{payload: valid("settle-ackfail"), attempts: 1, ackErr: errors.New("broker went away")}
		if got := consumer.handleDelivery(context.Background(), d); got != outcomePersisted || len(store.calls) != 1 {
			t.Fatalf("outcome=%s rows=%d", got, len(store.calls))
		}
	})
}
