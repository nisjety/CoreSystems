package main

import (
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

func TestCostRuntimeReceivesOfflineUsageAndRetriesWithoutProvisioningAuthority(t *testing.T) {
	broker := startModelBroker(t)
	provisionModelTopology(t, broker)
	producer, _ := connectAs(t, broker, "model-gateway-runtime", "_INBOX.MODEL_GATEWAY_RUNTIME")
	js, err := producer.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	// No Cost Core subscriber is connected when the event is published.
	ack, err := js.Publish("mp.v1.usage.org-a", []byte(`{"event_type":"USAGE_ENVELOPE","org_id":"org-a"}`))
	if err != nil || ack.Stream != "MODEL_PLANE_USAGE_EVENTS" {
		t.Fatalf("ack=%v err=%v", ack, err)
	}
	runtime, asyncErrors := connectAs(t, broker, "cost-core-runtime", "_INBOX.COST_CORE_RUNTIME")
	costJS, err := runtime.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	bind := func() *nats.Subscription {
		sub, err := costJS.QueueSubscribeSync("mp.v1.usage.*", "cost-core-usage", nats.Bind("MODEL_PLANE_USAGE_EVENTS", "cost-core-usage"), nats.ManualAck())
		if err != nil {
			t.Fatal(err)
		}
		return sub
	}
	sub := bind()
	first, err := sub.NextMsg(3 * time.Second)
	if err != nil {
		t.Fatal("offline usage lost:", err)
	}
	metadata, _ := first.Metadata()
	if err := first.NakWithDelay(25 * time.Millisecond); err != nil {
		t.Fatal(err)
	}
	_ = sub.Unsubscribe()
	sub = bind()
	defer sub.Unsubscribe()
	retried, err := sub.NextMsg(3 * time.Second)
	if err != nil {
		t.Fatal("failed write was not redelivered after restart:", err)
	}
	retryMetadata, _ := retried.Metadata()
	if retryMetadata.Sequence.Stream != metadata.Sequence.Stream || retryMetadata.NumDelivered < 2 {
		t.Fatal("retry did not retain event identity")
	}
	if err := retried.AckSync(); err != nil {
		t.Fatal(err)
	}
	info, err := costJS.ConsumerInfo("MODEL_PLANE_USAGE_EVENTS", "cost-core-usage")
	if err != nil || info.NumAckPending != 0 || info.NumPending != 0 {
		t.Fatalf("consumer info=%#v err=%v", info, err)
	}
	// The runtime may bind/ack its fixed consumer, never create another one.
	if _, err := costJS.AddConsumer("MODEL_PLANE_USAGE_EVENTS", &nats.ConsumerConfig{Durable: "forged", AckPolicy: nats.AckExplicitPolicy}); err == nil {
		t.Fatal("cost runtime created a consumer")
	}
	select {
	case <-asyncErrors:
	default:
	}
}
