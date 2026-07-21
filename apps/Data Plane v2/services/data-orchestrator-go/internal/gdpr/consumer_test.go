package gdpr

import (
	"context"
	"errors"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
)

type fakeDelivery struct {
	payload  []byte
	attempts uint64
	acks     int
	naks     int
}

func (d *fakeDelivery) Payload() []byte                   { return d.payload }
func (d *fakeDelivery) DeliveryAttempts() (uint64, error) { return d.attempts, nil }
func (d *fakeDelivery) Ack() error                        { d.acks++; return nil }
func (d *fakeDelivery) NakWithDelay(time.Duration) error  { d.naks++; return nil }

type fakeDLQPublisher struct {
	ack   *nats.PubAck
	err   error
	msg   *nats.Msg
	calls int
}

func (p *fakeDLQPublisher) PublishMsg(msg *nats.Msg, _ ...nats.PubOpt) (*nats.PubAck, error) {
	p.calls++
	p.msg = msg
	return p.ack, p.err
}

func validOrgDeliveryPayload() []byte {
	return []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":"org-1","requested_by":"admin","ts":"2026-07-20T00:00:00Z"}`)
}

func TestProcessOrgPurgeDeliveryACKsOnlyAfterSuccessfulPurge(t *testing.T) {
	delivery := &fakeDelivery{payload: validOrgDeliveryPayload(), attempts: 1}
	repo := &fakeOrgPurger{}
	if err := processOrgPurgeDelivery(context.Background(), delivery, repo, &fakeDLQPublisher{}, newConsumerHealth()); err != nil {
		t.Fatalf("process org purge delivery: %v", err)
	}
	if delivery.acks != 1 || delivery.naks != 0 {
		t.Fatalf("acks=%d naks=%d, want one ACK", delivery.acks, delivery.naks)
	}
	if len(repo.calls) != 1 || repo.calls[0] != "org-1" {
		t.Fatalf("purge calls = %v", repo.calls)
	}
}

func TestProcessOrgPurgeDeliveryNAKsTransientFailureBeforeTerminalThreshold(t *testing.T) {
	delivery := &fakeDelivery{payload: validOrgDeliveryPayload(), attempts: terminalDeliveryAttempt - 1}
	repo := &fakeOrgPurger{err: errors.New("postgres unavailable")}
	dlq := &fakeDLQPublisher{}
	if err := processOrgPurgeDelivery(context.Background(), delivery, repo, dlq, newConsumerHealth()); err == nil {
		t.Fatal("transient failure must be observable")
	}
	if delivery.acks != 0 || delivery.naks != 1 || dlq.calls != 0 {
		t.Fatalf("acks=%d naks=%d dlq=%d", delivery.acks, delivery.naks, dlq.calls)
	}
}

func TestProcessOrgPurgeDeliveryMovesPoisonToDLQBeforeACK(t *testing.T) {
	delivery := &fakeDelivery{payload: []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":""}`), attempts: 1}
	dlq := &fakeDLQPublisher{ack: &nats.PubAck{Stream: controlSharedStream, Sequence: 9}}
	if err := processOrgPurgeDelivery(context.Background(), delivery, &fakeOrgPurger{}, dlq, newConsumerHealth()); err != nil {
		t.Fatalf("poison DLQ: %v", err)
	}
	if delivery.acks != 1 || dlq.calls != 1 || dlq.msg == nil || dlq.msg.Subject != erasureDLQSubject {
		t.Fatalf("acks=%d dlq=%d msg=%+v", delivery.acks, dlq.calls, dlq.msg)
	}
}

func TestProcessOrgPurgeDeliveryPreservesMalformedJSONAsDLQEvidence(t *testing.T) {
	delivery := &fakeDelivery{payload: []byte(`{"subject_type":`), attempts: 1}
	dlq := &fakeDLQPublisher{ack: &nats.PubAck{Stream: controlSharedStream, Sequence: 10}}
	if err := processOrgPurgeDelivery(context.Background(), delivery, &fakeOrgPurger{}, dlq, newConsumerHealth()); err != nil {
		t.Fatalf("malformed poison DLQ: %v", err)
	}
	if delivery.acks != 1 || dlq.calls != 1 {
		t.Fatalf("malformed evidence was stranded: acks=%d dlq=%d", delivery.acks, dlq.calls)
	}
}

// A non-organization event (e.g. a per-user erasure fan-out delivered to
// this consumer over the shared subject) must ACK as a clean no-op — no DLQ
// write, since it isn't poison, just not this consumer's concern.
func TestProcessOrgPurgeDeliverySkipsNonOrganizationSubjectsCleanly(t *testing.T) {
	delivery := &fakeDelivery{payload: []byte(`{"event_id":"gdpr:fanout:child-1","subject_type":"user","subject_id":"user-a","org_id":"org-1"}`), attempts: 1}
	repo := &fakeOrgPurger{}
	dlq := &fakeDLQPublisher{}
	if err := processOrgPurgeDelivery(context.Background(), delivery, repo, dlq, newConsumerHealth()); err != nil {
		t.Fatalf("skip: %v", err)
	}
	if delivery.acks != 1 || dlq.calls != 0 || len(repo.calls) != 0 {
		t.Fatalf("acks=%d dlq=%d purge=%v, want a clean ACK no-op", delivery.acks, dlq.calls, repo.calls)
	}
}

func TestProcessOrgPurgeDeliveryNeverACKsBeforeValidDLQPubAck(t *testing.T) {
	for _, test := range []struct {
		name string
		ack  *nats.PubAck
		err  error
	}{
		{name: "transport failure", err: errors.New("broker unavailable")},
		{name: "missing ack"},
		{name: "wrong stream", ack: &nats.PubAck{Stream: "OTHER", Sequence: 1}},
		{name: "zero sequence", ack: &nats.PubAck{Stream: controlSharedStream}},
	} {
		t.Run(test.name, func(t *testing.T) {
			delivery := &fakeDelivery{payload: validOrgDeliveryPayload(), attempts: terminalDeliveryAttempt}
			dlq := &fakeDLQPublisher{ack: test.ack, err: test.err}
			err := processOrgPurgeDelivery(context.Background(), delivery, &fakeOrgPurger{err: errors.New("permanent db failure")}, dlq, newConsumerHealth())
			if err == nil || delivery.acks != 0 || delivery.naks != 1 {
				t.Fatalf("err=%v acks=%d naks=%d", err, delivery.acks, delivery.naks)
			}
		})
	}
}

type channelOrgPurger struct{ calls chan string }

func (p *channelOrgPurger) HardPurgeByOrg(_ context.Context, orgID string) error {
	p.calls <- orgID
	return nil
}

func TestStartOrgPurgeSubscriberBindsPreprovisionedDurableACKsAndReportsHealth(t *testing.T) {
	instance, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

	nc, err := nats.Connect(instance.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name: controlSharedStream,
		Subjects: []string{
			ErasureRequestedSubject, erasureDLQSubject,
		},
		Storage: nats.MemoryStorage,
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddConsumer(controlSharedStream, &nats.ConsumerConfig{
		Durable: durableConsumerName, DeliverSubject: durableDeliverySubject,
		DeliverGroup: durableConsumerName, FilterSubject: ErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: time.Second, MaxDeliver: consumerMaxDeliver,
	}); err != nil {
		t.Fatal(err)
	}

	repo := &channelOrgPurger{calls: make(chan string, 1)}
	consumer, err := StartOrgPurgeSubscriber(nc, repo)
	if err != nil {
		t.Fatalf("start durable org-purge consumer: %v", err)
	}
	t.Cleanup(func() { _ = consumer.Close() })
	if _, err := js.Publish(ErasureRequestedSubject, validOrgDeliveryPayload(), nats.MsgId("gdpr:org-fanout:org-1")); err != nil {
		t.Fatal(err)
	}
	select {
	case orgID := <-repo.calls:
		if orgID != "org-1" {
			t.Fatalf("purged org=%q, want org-1", orgID)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("durable org-purge consumer did not process the erasure")
	}

	deadline := time.Now().Add(3 * time.Second)
	for {
		health := consumer.Health(context.Background())
		if health.Acknowledged == 1 && health.AckPending == 0 && health.Status == "healthy" {
			if health.NumOutstanding() != 0 || health.LastSuccessAt == "" {
				t.Fatalf("health missing convergence evidence: %+v", health)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("consumer did not report acknowledged health: %+v", health)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestStartOrgPurgeSubscriberFailsClosedWithoutDependencies(t *testing.T) {
	if _, err := StartOrgPurgeSubscriber(nil, &fakeOrgPurger{}); err == nil {
		t.Fatal("nil NATS connection was accepted")
	}
	var consumer *Consumer
	if err := consumer.Close(); err != nil {
		t.Fatalf("nil consumer close: %v", err)
	}
	if health := consumer.Health(context.Background()); health.Status != "unhealthy" {
		t.Fatalf("nil consumer health=%+v", health)
	}
}
