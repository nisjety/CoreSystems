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

func validDeliveryPayload() []byte {
	return []byte(`{"event_id":"gdpr:fanout:child-1","operation_id":"gdpr:operation-1","subject_type":"user","subject_id":"user-a","org_id":"org-1"}`)
}

func TestProcessDeliveryACKsOnlyAfterSuccessfulHandling(t *testing.T) {
	delivery := &fakeDelivery{payload: validDeliveryPayload(), attempts: 1}
	repo := &fakeTransferrer{ret: 2}
	if err := processDelivery(context.Background(), delivery, repo, &fakePublisher{}, &fakeDLQPublisher{}, newConsumerHealth()); err != nil {
		t.Fatalf("process delivery: %v", err)
	}
	if delivery.acks != 1 || delivery.naks != 0 {
		t.Fatalf("acks=%d naks=%d, want one ACK", delivery.acks, delivery.naks)
	}
}

func TestProcessDeliveryNAKsTransientFailureBeforeTerminalThreshold(t *testing.T) {
	delivery := &fakeDelivery{payload: validDeliveryPayload(), attempts: terminalDeliveryAttempt - 1}
	repo := &fakeTransferrer{err: errors.New("postgres unavailable")}
	dlq := &fakeDLQPublisher{}
	if err := processDelivery(context.Background(), delivery, repo, &fakePublisher{}, dlq, newConsumerHealth()); err == nil {
		t.Fatal("transient failure must be observable")
	}
	if delivery.acks != 0 || delivery.naks != 1 || dlq.calls != 0 {
		t.Fatalf("acks=%d naks=%d dlq=%d", delivery.acks, delivery.naks, dlq.calls)
	}
}

func TestProcessDeliveryMovesPoisonToDLQBeforeACK(t *testing.T) {
	delivery := &fakeDelivery{payload: []byte(`{"subject_type":"user"}`), attempts: 1}
	dlq := &fakeDLQPublisher{ack: &nats.PubAck{Stream: controlSharedStream, Sequence: 9}}
	if err := processDelivery(context.Background(), delivery, &fakeTransferrer{}, &fakePublisher{}, dlq, newConsumerHealth()); err != nil {
		t.Fatalf("poison DLQ: %v", err)
	}
	if delivery.acks != 1 || dlq.calls != 1 || dlq.msg == nil || dlq.msg.Subject != erasureDLQSubject {
		t.Fatalf("acks=%d dlq=%d msg=%+v", delivery.acks, dlq.calls, dlq.msg)
	}
}

func TestProcessDeliveryPreservesMalformedJSONAsDLQEvidence(t *testing.T) {
	delivery := &fakeDelivery{payload: []byte(`{"subject_type":`), attempts: 1}
	dlq := &fakeDLQPublisher{ack: &nats.PubAck{Stream: controlSharedStream, Sequence: 10}}
	if err := processDelivery(context.Background(), delivery, &fakeTransferrer{}, &fakePublisher{}, dlq, newConsumerHealth()); err != nil {
		t.Fatalf("malformed poison DLQ: %v", err)
	}
	if delivery.acks != 1 || dlq.calls != 1 {
		t.Fatalf("malformed evidence was stranded: acks=%d dlq=%d", delivery.acks, dlq.calls)
	}
}

func TestProcessDeliveryNeverACKsBeforeValidDLQPubAck(t *testing.T) {
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
			delivery := &fakeDelivery{payload: validDeliveryPayload(), attempts: terminalDeliveryAttempt}
			dlq := &fakeDLQPublisher{ack: test.ack, err: test.err}
			err := processDelivery(context.Background(), delivery, &fakeTransferrer{err: errors.New("permanent db failure")}, &fakePublisher{}, dlq, newConsumerHealth())
			if err == nil || delivery.acks != 0 || delivery.naks != 1 {
				t.Fatalf("err=%v acks=%d naks=%d", err, delivery.acks, delivery.naks)
			}
		})
	}
}

type channelTransferrer struct{ calls chan [3]string }

func (transferrer *channelTransferrer) TransferOwnership(_ context.Context, orgID, from, to string) (int64, error) {
	transferrer.calls <- [3]string{orgID, from, to}
	return 1, nil
}

func TestStartSubscriberBindsPreprovisionedDurableACKsAndReportsHealth(t *testing.T) {
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
			ErasureRequestedSubject, erasureDLQSubject, OwnershipTransferredSubject,
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

	repo := &channelTransferrer{calls: make(chan [3]string, 1)}
	consumer, err := StartSubscriber(nc, repo)
	if err != nil {
		t.Fatalf("start durable consumer: %v", err)
	}
	t.Cleanup(func() { _ = consumer.Close() })
	if _, err := js.Publish(ErasureRequestedSubject, validDeliveryPayload(), nats.MsgId("gdpr:fanout:child-1")); err != nil {
		t.Fatal(err)
	}
	select {
	case call := <-repo.calls:
		if call != [3]string{"org-1", "user-a", systemAccount} {
			t.Fatalf("ownership transfer call=%v", call)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("durable consumer did not process the erasure")
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

func TestStartSubscriberFailsClosedWithoutDependencies(t *testing.T) {
	if _, err := StartSubscriber(nil, &fakeTransferrer{}); err == nil {
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
