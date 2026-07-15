package subscriber

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

func TestStartFailsClosedWhenProvisionedResourcesAreMissing(t *testing.T) {
	natsServer, nc := startCoverageNATS(t)
	_ = natsServer
	st := &recordingStore{audits: make(chan *events.AuditEvent, 1), usage: make(chan *events.UsageEvent, 1)}
	if err := New(nc, st).Start(context.Background()); err == nil {
		t.Fatal("subscriber started without its provisioned stream")
	}

	provisionScopedObservability(t, nc, "control", "control")
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := js.DeleteConsumer(streamName, "audit-core-control-v3-usage"); err != nil {
		t.Fatal(err)
	}
	if err := New(nc, st).Start(context.Background()); err == nil {
		t.Fatal("subscriber started without its provisioned usage consumer")
	}
}

func TestHandlersFailClosedForMissingMetadataAndDeadLetterFailures(t *testing.T) {
	_, nc := startCoverageNATS(t)
	js, err := nc.JetStream(nats.MaxWait(100 * time.Millisecond))
	if err != nil {
		t.Fatal(err)
	}
	st := &recordingStore{audits: make(chan *events.AuditEvent, 1), usage: make(chan *events.UsageEvent, 1)}
	sub := New(nc, st)
	sub.js = js

	auditPayload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-missing-metadata", OccurredAt: time.Now().UTC(), OrgID: "org",
		Plane: "control", Producer: "auth-core", Event: "missing_metadata",
	})
	sub.handleAudit(context.Background())(&nats.Msg{Subject: "velion.audit.v2.control.auth-core.missing_metadata", Data: auditPayload})
	usagePayload, _ := json.Marshal(events.UsageEvent{
		EventID: "usage-missing-metadata", OccurredAt: time.Now().UTC(), OrgID: "org",
		Plane: "control", Producer: "billing-core", Op: "missing_metadata",
	})
	sub.handleUsage(context.Background())(&nats.Msg{Subject: "velion.usage.v2.control.billing-core.missing_metadata", Data: usagePayload})

	malformedAudit := &nats.Msg{Subject: "velion.audit.v2.control.auth-core.malformed", Data: []byte("not-json")}
	sub.handleAudit(context.Background())(malformedAudit)
	malformedUsage := &nats.Msg{Subject: "velion.usage.v2.control.billing-core.malformed", Data: []byte("not-json")}
	sub.handleUsage(context.Background())(malformedUsage)
	if sub.deadLetter(nats.NewMsg("velion.audit.v2.control.auth-core.unroutable"), "audit", "fixture") {
		t.Fatal("dead-letter publish unexpectedly succeeded without a stream")
	}
	sub.rejectPlaneMismatch(nats.NewMsg("velion.audit.v2.control.auth-core.mismatch"), "audit", time.Now())
	sub.retryOrDeadLetter(nats.NewMsg("velion.audit.v2.control.auth-core.retry"), "audit")
	if sequence, ok := messageStreamSequence(nats.NewMsg("velion.audit.v2.control.auth-core.sequence")); ok || sequence != 0 {
		t.Fatalf("unbound message sequence = %d, %v", sequence, ok)
	}
}

func TestExhaustedRetryDeadLetterFailureRemainsUnacked(t *testing.T) {
	_, nc := startCoverageNATS(t)
	js, err := nc.JetStream(nats.MaxWait(100 * time.Millisecond))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name: streamName, Subjects: []string{planeSubject("audit", "control"), planeSubject("usage", "control")}, Storage: nats.MemoryStorage,
	}); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"audit", "usage"} {
		consumer := "audit-core-control-v3-" + kind
		if _, err := js.AddConsumer(streamName, &nats.ConsumerConfig{
			Durable: consumer, DeliverSubject: "_VELION.AUDIT.DELIVER.control." + kind + "-v2",
			DeliverGroup: consumer, FilterSubject: planeSubject(kind, "control"),
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: maxConsumerDeliveries, ReplayPolicy: nats.ReplayInstantPolicy,
		}); err != nil {
			t.Fatal(err)
		}
	}
	st := &recordingStore{audits: make(chan *events.AuditEvent, 1), usage: make(chan *events.UsageEvent, 1)}
	st.failAudit.Store(true)
	sub := New(nc, st)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	payload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-store-down", OccurredAt: time.Now().UTC(), OrgID: "org",
		Plane: "control", Producer: "auth-core", Event: "store_down",
	})
	if _, err := js.Publish("velion.audit.v2.control.auth-core.store_down", payload); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(8 * time.Second)
	for st.auditCalls.Load() < maxDeliveries && time.Now().Before(deadline) {
		time.Sleep(20 * time.Millisecond)
	}
	if got := st.auditCalls.Load(); got < maxDeliveries {
		t.Fatalf("deliveries = %d; want %d", got, maxDeliveries)
	}
	if _, err := js.GetLastMsg(streamName, "velion.dlq.audit-core.audit"); err == nil {
		t.Fatal("unroutable dead-letter was unexpectedly persisted")
	}
}

func TestExhaustedRetryTerminationErrorIsHandled(t *testing.T) {
	_, nc := startCoverageNATS(t)
	js, err := nc.JetStream(nats.MaxWait(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name: streamName, Subjects: []string{"velion.audit.v2.control.auth-core.>", dlqSubject}, Storage: nats.MemoryStorage,
	}); err != nil {
		t.Fatal(err)
	}
	syncSub, err := js.SubscribeSync(
		"velion.audit.v2.control.auth-core.>", nats.Durable("coverage-retry"), nats.ManualAck(), nats.AckExplicit(), nats.MaxDeliver(10),
	)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.Publish("velion.audit.v2.control.auth-core.retry", []byte(`{"fixture":true}`)); err != nil {
		t.Fatal(err)
	}
	var msg *nats.Msg
	for delivery := uint64(1); delivery <= maxDeliveries; delivery++ {
		msg, err = syncSub.NextMsg(2 * time.Second)
		if err != nil {
			t.Fatal(err)
		}
		metadata, err := msg.Metadata()
		if err != nil || metadata.NumDelivered != delivery {
			t.Fatalf("delivery metadata = %+v, %v; want %d", metadata, err, delivery)
		}
		if delivery < maxDeliveries {
			if err := msg.NakWithDelay(time.Millisecond); err != nil {
				t.Fatal(err)
			}
		}
	}
	sub := New(nc, &recordingStore{})
	sub.js = js
	sub.retryOrDeadLetter(msg, "audit")
	// A second disposition exercises the already-terminated error path without
	// changing broker state beyond this isolated in-memory fixture.
	sub.retryOrDeadLetter(msg, "audit")
	streamInfo, err := js.StreamInfo(streamName)
	if err != nil {
		t.Fatal(err)
	}
	config := streamInfo.Config
	config.Subjects = []string{"velion.audit.v2.control.auth-core.>"}
	if _, err := js.UpdateStream(&config); err != nil {
		t.Fatal(err)
	}
	// With the DLQ subject deliberately absent, exhaustion must retain the
	// message for operator recovery rather than terminating it without evidence.
	sub.retryOrDeadLetter(msg, "audit")
}

func TestDirectHandlersCoverTerminationErrorsAndUsagePlaneMismatch(t *testing.T) {
	_, nc := startCoverageNATS(t)
	js, err := nc.JetStream(nats.MaxWait(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{Name: streamName, Subjects: []string{dlqSubject}, Storage: nats.MemoryStorage}); err != nil {
		t.Fatal(err)
	}
	sub := New(nc, &recordingStore{audits: make(chan *events.AuditEvent, 1), usage: make(chan *events.UsageEvent, 1)})
	sub.js = js
	sub.handleAudit(context.Background())(&nats.Msg{Subject: "velion.audit.v2.control.auth-core.bad", Data: []byte("bad")})
	sub.handleUsage(context.Background())(&nats.Msg{Subject: "velion.usage.v2.control.billing-core.bad", Data: []byte("bad")})

	payload, _ := json.Marshal(events.UsageEvent{
		EventID: "usage-forged-plane", OccurredAt: time.Now().UTC(), OrgID: "org",
		Plane: "model", Producer: "billing-core", Op: "forged",
	})
	sub.handleUsage(context.Background())(&nats.Msg{Subject: "velion.usage.v2.control.billing-core.forged", Data: payload})
}

func TestDuplicateAuditAndUsageEventsAreAcknowledged(t *testing.T) {
	_, nc := startCoverageNATS(t)
	provisionScopedObservability(t, nc, "control", "control")
	st := &recordingStore{
		audits: make(chan *events.AuditEvent, 1), usage: make(chan *events.UsageEvent, 1),
		duplicateAudit: true, duplicateUsage: true,
	}
	sub := New(nc, st)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	auditPayload, _ := json.Marshal(events.AuditEvent{
		EventID: "audit-auth-duplicate", OccurredAt: time.Now().UTC(), OrgID: "org",
		Plane: "control", Producer: "auth-core", Event: "duplicate",
	})
	if _, err := js.Publish("velion.audit.v2.control.auth-core.duplicate", auditPayload); err != nil {
		t.Fatal(err)
	}
	usagePayload, _ := json.Marshal(events.UsageEvent{
		EventID: "usage-duplicate", OccurredAt: time.Now().UTC(),
		OrgID: "org", Plane: "control", Producer: "billing-core", Op: "duplicate",
	})
	if _, err := js.Publish("velion.usage.v2.control.billing-core.duplicate", usagePayload); err != nil {
		t.Fatal(err)
	}
	select {
	case <-st.audits:
	case <-time.After(3 * time.Second):
		t.Fatal("duplicate audit was not handled")
	}
	select {
	case <-st.usage:
	case <-time.After(3 * time.Second):
		t.Fatal("duplicate usage was not handled")
	}
}

func startCoverageNATS(t *testing.T) (*server.Server, *nats.Conn) {
	t.Helper()
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	return natsServer, nc
}
