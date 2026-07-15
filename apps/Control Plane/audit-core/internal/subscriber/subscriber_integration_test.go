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
	audits         chan *events.AuditEvent
	usage          chan *events.UsageEvent
	auditCalls     atomic.Int32
	usageCalls     atomic.Int32
	failAudit      atomic.Bool
	retryUsage     bool
	duplicateAudit bool
	duplicateUsage bool
}

func (s *recordingStore) InsertAuditFromStream(_ context.Context, ev *events.AuditEvent, _, _ string, _ uint64) (bool, error) {
	s.auditCalls.Add(1)
	if s.failAudit.Load() {
		return false, errors.New("persistent store failure")
	}
	s.audits <- ev
	return !s.duplicateAudit, nil
}

func (s *recordingStore) InsertUsageFromStream(_ context.Context, ev *events.UsageEvent, _, _ string, _ uint64) (bool, error) {
	if s.usageCalls.Add(1) == 1 && s.retryUsage {
		return false, errors.New("temporary store failure")
	}
	s.usage <- ev
	return !s.duplicateUsage, nil
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
		audits:     make(chan *events.AuditEvent, 1),
		usage:      make(chan *events.UsageEvent, 1),
		retryUsage: true,
	}
	if got := New(nc, st).consumerName("audit"); got != "audit-core-control-v3-audit" {
		t.Fatalf("primary control-bus consumer name = %q", got)
	}
	if got := New(nc, st, "extra-1").consumerName("audit"); got != "audit-core-extra-1-v3-audit" {
		t.Fatalf("extra-bus consumer name = %q", got)
	}
	provisionScopedObservability(t, nc, "control", "control")
	sub := New(nc, st)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatalf("start subscriber: %v", err)
	}

	js, err := nc.JetStream()
	if err != nil {
		t.Fatalf("JetStream context: %v", err)
	}
	auditPayload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-session-1", OccurredAt: time.Now().UTC(), OrgID: "org-test",
		Plane: "control", Producer: "auth-core", Event: "signed_in",
	})
	if _, err := js.Publish("velion.audit.v2.control.auth-core.signed_in", auditPayload); err != nil {
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
		EventID: "usage-control-org-test-tokens", OccurredAt: time.Now().UTC(), OrgID: "org-test",
		Plane: "control", Producer: "billing-core", Op: "tokens",
	})
	if _, err := js.Publish("velion.usage.v2.control.billing-core.tokens", usagePayload); err != nil {
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

	if _, err := js.Publish("velion.audit.v2.control.auth-core.malformed", []byte("not-json")); err != nil {
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

	if _, err := js.Publish("velion.usage.v2.control.billing-core.malformed", []byte("not-json-usage")); err != nil {
		t.Fatalf("publish malformed usage: %v", err)
	}
	waitForDeadLetter(t, js, "usage", "malformed", "not-json-usage")

	tamperedPayload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-forged-plane", OccurredAt: time.Now().UTC(), OrgID: "org-test",
		Plane: "application", Producer: "auth-core", Event: "forged_plane",
	})
	if _, err := js.Publish("velion.audit.v2.control.auth-core.forged_plane", tamperedPayload); err != nil {
		t.Fatalf("publish plane-tampered audit: %v", err)
	}
	waitForDeadLetter(t, js, "audit", "authority_mismatch", string(tamperedPayload))

	st.failAudit.Store(true)
	exhaustedPayload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-store-down", OccurredAt: time.Now().UTC(), OrgID: "org-test",
		Plane: "control", Producer: "auth-core", Event: "store_down",
	})
	if _, err := js.Publish("velion.audit.v2.control.auth-core.store_down", exhaustedPayload); err != nil {
		t.Fatalf("publish audit for exhausted retry: %v", err)
	}
	waitForDeadLetter(t, js, "audit", "delivery_exhausted", string(exhaustedPayload))
	if attempts := st.auditCalls.Load(); attempts < maxDeliveries+1 {
		t.Fatalf("audit calls = %d; want initial success plus %d failed deliveries", attempts, maxDeliveries)
	}
}

func TestDurableV2UsageWithoutStableEventIDIsDeadLetteredBeforeStore(t *testing.T) {
	_, nc := startCoverageNATS(t)
	provisionScopedObservability(t, nc, "control", "control")
	st := &recordingStore{
		audits: make(chan *events.AuditEvent, 1),
		usage:  make(chan *events.UsageEvent, 1),
	}
	sub := New(nc, st)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	payload, _ := json.Marshal(events.UsageEvent{
		OccurredAt: time.Now().UTC(), OrgID: "org-test", Plane: "control", Producer: "billing-core", Op: "tokens",
	})
	if _, err := js.Publish("velion.usage.v2.control.billing-core.tokens", payload); err != nil {
		t.Fatal(err)
	}
	waitForDeadLetter(t, js, "usage", "malformed", string(payload))
	if calls := st.usageCalls.Load(); calls != 0 {
		t.Fatalf("store calls = %d; missing logical identity must fail before persistence", calls)
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
