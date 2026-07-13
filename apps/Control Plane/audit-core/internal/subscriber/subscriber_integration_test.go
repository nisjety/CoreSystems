package subscriber

import (
	"context"
	"encoding/json"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

type recordingStore struct {
	audits     chan *events.AuditEvent
	usage      chan *events.UsageEvent
	auditCalls atomic.Int32
	usageCalls atomic.Int32
	failAudit  atomic.Bool
}

func (s *recordingStore) InsertAuditFromStream(_ context.Context, ev *events.AuditEvent, _ string, _ uint64) (bool, error) {
	s.auditCalls.Add(1)
	if s.failAudit.Load() {
		return false, errors.New("persistent store failure")
	}
	s.audits <- ev
	return true, nil
}

func (s *recordingStore) InsertUsageFromStream(_ context.Context, ev *events.UsageEvent, _ string, _ uint64) (bool, error) {
	if s.usageCalls.Add(1) == 1 {
		return false, errors.New("temporary store failure")
	}
	s.usage <- ev
	return true, nil
}

func TestDurableSubscriberAcknowledgesRetriesAndDeadLetters(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{
		JetStream: true,
		StoreDir:  t.TempDir(),
		Port:      -1,
	})
	if err != nil {
		t.Fatalf("create NATS server: %v", err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() {
		natsServer.Shutdown()
		natsServer.WaitForShutdown()
	})

	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatalf("connect NATS: %v", err)
	}
	t.Cleanup(nc.Close)

	st := &recordingStore{
		audits: make(chan *events.AuditEvent, 1),
		usage:  make(chan *events.UsageEvent, 1),
	}
	if got := New(nc, st, "extra-1").consumerName("audit"); got != "audit-core-extra-1-audit" {
		t.Fatalf("extra-bus consumer name = %q", got)
	}
	sub := New(nc, st)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatalf("start subscriber: %v", err)
	}

	js, err := nc.JetStream()
	if err != nil {
		t.Fatalf("JetStream context: %v", err)
	}
	auditPayload, _ := json.Marshal(events.AuditEvent{
		OccurredAt: time.Now().UTC(), OrgID: "org-test", Plane: "control", Event: "signed_in",
	})
	if _, err := js.Publish("velion.audit.v1.control.signed_in", auditPayload); err != nil {
		t.Fatalf("publish audit: %v", err)
	}
	select {
	case persisted := <-st.audits:
		if persisted.OrgID != "org-test" {
			t.Fatalf("persisted org = %q", persisted.OrgID)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("audit event was not persisted")
	}

	usagePayload, _ := json.Marshal(events.UsageEvent{
		OccurredAt: time.Now().UTC(), OrgID: "org-test", Plane: "model", Op: "tokens",
	})
	if _, err := js.Publish("velion.usage.v1.model.tokens", usagePayload); err != nil {
		t.Fatalf("publish usage: %v", err)
	}
	select {
	case <-st.usage:
		if st.usageCalls.Load() < 2 {
			t.Fatalf("usage attempts = %d; want retry", st.usageCalls.Load())
		}
	case <-time.After(5 * time.Second):
		t.Fatal("usage event was not retried and persisted")
	}

	if _, err := js.Publish("velion.audit.v1.control.malformed", []byte("not-json")); err != nil {
		t.Fatalf("publish malformed audit: %v", err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		message, err := js.GetLastMsg(streamName, "velion.dlq.audit-core.audit")
		if err == nil {
			if string(message.Data) != "not-json" {
				t.Fatalf("dead-letter payload = %q", message.Data)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("malformed event was not dead-lettered: %v", err)
		}
		time.Sleep(10 * time.Millisecond)
	}

	if err := ensureStream(js); err != nil {
		t.Fatalf("update existing stream: %v", err)
	}
	if _, err := js.Publish("velion.usage.v1.model.malformed", []byte("not-json-usage")); err != nil {
		t.Fatalf("publish malformed usage: %v", err)
	}
	waitForDeadLetter(t, js, "usage", "malformed", "not-json-usage")

	st.failAudit.Store(true)
	exhaustedPayload, _ := json.Marshal(events.AuditEvent{
		OccurredAt: time.Now().UTC(), OrgID: "org-test", Plane: "control", Event: "store_down",
	})
	if _, err := js.Publish("velion.audit.v1.control.store_down", exhaustedPayload); err != nil {
		t.Fatalf("publish audit for exhausted retry: %v", err)
	}
	waitForDeadLetter(t, js, "audit", "delivery_exhausted", string(exhaustedPayload))
	if attempts := st.auditCalls.Load(); attempts < maxDeliveries+1 {
		t.Fatalf("audit calls = %d; want initial success plus %d failed deliveries", attempts, maxDeliveries)
	}
}

func waitForDeadLetter(t *testing.T, js nats.JetStreamContext, kind, reason, payload string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		message, err := js.GetLastMsg(streamName, "velion.dlq.audit-core."+kind)
		if err == nil && message.Header.Get("Velion-Dead-Letter-Reason") == reason {
			if string(message.Data) != payload {
				t.Fatalf("dead-letter payload = %q", message.Data)
			}
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("%s event was not dead-lettered for %s: %v", kind, reason, err)
		}
		time.Sleep(10 * time.Millisecond)
	}
}
