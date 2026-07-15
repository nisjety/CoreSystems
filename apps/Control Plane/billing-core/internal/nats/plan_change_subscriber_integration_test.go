package nats

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"
	natsgo "github.com/nats-io/nats.go"
)

type retryingPlanChangeApplier struct {
	mu        sync.Mutex
	calls     int
	failUntil int
	revisions []int64
	called    chan struct{}
}

func (a *retryingPlanChangeApplier) ApplyOrganizationPlanChange(
	_ context.Context,
	_, _, _ string,
	revision int64,
) (bool, error) {
	a.mu.Lock()
	a.calls++
	a.revisions = append(a.revisions, revision)
	call := a.calls
	a.mu.Unlock()
	a.called <- struct{}{}
	if call <= a.failUntil {
		return false, errors.New("fixture crash before ack")
	}
	return true, nil
}

func TestPlanChangeConsumerIsDurableRetriesBeforeAckAndDeadLettersMalformed(t *testing.T) {
	server := startPlanChangeJetStreamServer(t)
	client, err := NewClient(Config{URL: server.ClientURL(), Name: "billing-plan-test"})
	if err != nil {
		t.Fatalf("connect billing NATS client: %v", err)
	}
	t.Cleanup(client.Close)
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	legacyJS, err := client.conn.JetStream()
	if err != nil {
		t.Fatalf("open legacy JetStream context: %v", err)
	}
	if _, err := legacyJS.AddStream(&natsgo.StreamConfig{
		Name: "CONTROL_PLANE_EVENTS", Subjects: []string{"organization.>", "billing.>"},
		Retention: natsgo.LimitsPolicy, Storage: natsgo.FileStorage,
		MaxAge: 7 * 24 * time.Hour, MaxMsgs: 100000,
	}); err != nil {
		t.Fatalf("provision test stream: %v", err)
	}
	if _, err := legacyJS.AddConsumer("CONTROL_PLANE_EVENTS", &natsgo.ConsumerConfig{
		Durable:        planChangeConsumer,
		DeliverSubject: "_VELION.CONTROL.DELIVER.billing.organization-plan-changed",
		DeliverGroup:   planChangeConsumer,
		FilterSubject:  planChangeSubject,
		DeliverPolicy:  natsgo.DeliverAllPolicy,
		AckPolicy:      natsgo.AckExplicitPolicy,
		AckWait:        30 * time.Second,
		MaxDeliver:     planChangeMaxDelivery,
	}); err != nil {
		t.Fatalf("provision test consumer: %v", err)
	}

	dlq := make(chan *natsgo.Msg, 2)
	subscription, err := client.Subscribe(planChangeDLQSubject, func(message *natsgo.Msg) {
		dlq <- message
	})
	if err != nil {
		t.Fatalf("subscribe plan DLQ: %v", err)
	}
	t.Cleanup(func() { _ = subscription.Unsubscribe() })

	applier := &retryingPlanChangeApplier{failUntil: 1, called: make(chan struct{}, 16)}
	subscriber := &Subscriber{client: client, planApplier: applier}
	if err := subscriber.Start(ctx); err != nil {
		t.Fatalf("start billing subscriber: %v", err)
	}

	applier.mu.Lock()
	applier.failUntil = applier.calls + planChangeMaxDelivery
	applier.mu.Unlock()
	if err := client.Publish(ctx, planChangeSubject, map[string]any{
		"organization_id": "org-exhausted",
		"new_plan":        "standard",
		"revision":        int64(8),
	}); err != nil {
		t.Fatalf("publish exhausted plan event: %v", err)
	}
	select {
	case message := <-dlq:
		var payload map[string]any
		if err := json.Unmarshal(message.Data, &payload); err != nil {
			t.Fatalf("decode exhausted plan DLQ payload: %v", err)
		}
		if payload["reason"] != "retries_exhausted" || payload["original_subject"] != planChangeSubject {
			t.Fatalf("exhausted plan DLQ payload=%v", payload)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("exhausted plan event was not dead-lettered")
	}
	for len(applier.called) > 0 {
		<-applier.called
	}
	applier.mu.Lock()
	baseCalls := applier.calls
	applier.failUntil = baseCalls + 1
	applier.mu.Unlock()

	if err := client.Publish(ctx, planChangeSubject, map[string]any{
		"organization_id": "org-retry",
		"new_plan":        "pro",
		"revision":        int64(7),
	}); err != nil {
		t.Fatalf("publish revisioned plan event: %v", err)
	}
	for call := 0; call < 2; call++ {
		select {
		case <-applier.called:
		case <-time.After(5 * time.Second):
			t.Fatalf("timed out waiting for plan delivery %d", call+1)
		}
	}
	applier.mu.Lock()
	calls := applier.calls
	revisions := append([]int64(nil), applier.revisions...)
	applier.mu.Unlock()
	if calls != baseCalls+2 || len(revisions) != calls || revisions[calls-2] != 7 || revisions[calls-1] != 7 {
		t.Fatalf("retry calls=%d base=%d revisions=%v; want two final deliveries of revision 7", calls, baseCalls, revisions)
	}

	deadline := time.Now().Add(5 * time.Second)
	for {
		info, infoErr := legacyJS.ConsumerInfo("CONTROL_PLANE_EVENTS", planChangeConsumer)
		if infoErr == nil && info.NumAckPending == 0 && info.Delivered.Consumer >= 2 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("consumer did not settle after retry: info=%+v err=%v", info, infoErr)
		}
		time.Sleep(20 * time.Millisecond)
	}

	if err := client.Publish(ctx, planChangeSubject, map[string]any{
		"organization_id": "org-malformed",
		"new_plan":        "enterprise",
	}); err != nil {
		t.Fatalf("publish malformed plan event: %v", err)
	}
	select {
	case message := <-dlq:
		var payload map[string]any
		if err := json.Unmarshal(message.Data, &payload); err != nil {
			t.Fatalf("decode plan DLQ payload: %v", err)
		}
		if payload["reason"] != "malformed" || payload["original_subject"] != planChangeSubject {
			t.Fatalf("plan DLQ payload=%v", payload)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("malformed plan event was not dead-lettered")
	}
}

func TestPlanChangeRevisionDecoderRequiresPositiveInteger(t *testing.T) {
	for _, test := range []struct {
		raw   string
		valid bool
		want  int64
	}{
		{raw: `{"revision":1}`, valid: true, want: 1},
		{raw: `{"revision":9223372036854775807}`, valid: true, want: 9223372036854775807},
		{raw: `{"revision":0}`},
		{raw: `{"revision":-1}`},
		{raw: `{"revision":1.5}`},
		{raw: `{"revision":"2"}`},
		{raw: `{}`},
	} {
		payload, err := decodeEventData([]byte(test.raw))
		if err != nil {
			t.Fatalf("decode %s: %v", test.raw, err)
		}
		got, valid := readPositiveInt64(payload, "revision")
		if valid != test.valid || (valid && got != test.want) {
			t.Fatalf("revision %s = %d/%t; want %d/%t", test.raw, got, valid, test.want, test.valid)
		}
	}
}

func TestEventDecoderRejectsTrailingJSON(t *testing.T) {
	if _, err := decodeEventData([]byte(`{"revision":1}{"revision":2}`)); err == nil {
		t.Fatal("event decoder accepted trailing JSON")
	}
}

func startPlanChangeJetStreamServer(t *testing.T) *natsserver.Server {
	t.Helper()
	server, err := natsserver.NewServer(&natsserver.Options{
		Host:      "127.0.0.1",
		Port:      -1,
		JetStream: true,
		StoreDir:  t.TempDir(),
	})
	if err != nil {
		t.Fatalf("create NATS server: %v", err)
	}
	go server.Start()
	if !server.ReadyForConnections(5 * time.Second) {
		server.Shutdown()
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() {
		server.Shutdown()
		server.WaitForShutdown()
	})
	return server
}
