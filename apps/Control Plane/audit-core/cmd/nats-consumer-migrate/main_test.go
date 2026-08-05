package main

import (
	"context"
	"strings"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

const testStream = "TEST_STREAM"

// targetDurable is a real org-erasure durable name (a PULL consumer: no
// DeliverSubject/DeliverGroup, filtered on GDPRErasureRequestedSubject).
// migrate's recreate step looks names up in provisioner's fixed config map, so
// an arbitrary/synthetic durable name would always fail with "not a known
// org-erasure consumer durable name" -- exercising the real behavior needs a
// real name.
const targetDurable = provisioner.RetrievalEngineOrgErasureConsumerName

func newTestJetStream(t *testing.T) nats.JetStreamContext {
	t.Helper()
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name:     testStream,
		Subjects: []string{"legacy.subject", provisioner.GDPRErasureRequestedSubject},
		Storage:  nats.FileStorage,
	}); err != nil {
		t.Fatal(err)
	}
	return js
}

func assertConverged(t *testing.T, js nats.JetStreamContext) {
	t.Helper()
	info, err := js.ConsumerInfo(testStream, targetDurable)
	if err != nil {
		t.Fatalf("consumer was not recreated: %v", err)
	}
	if info.Config.FilterSubject != provisioner.GDPRErasureRequestedSubject {
		t.Fatalf("recreated consumer has the wrong filter: %+v", info.Config)
	}
}

func TestMigrateDeletesAStaleConsumerThenRecreatesFromSource(t *testing.T) {
	js := newTestJetStream(t)
	// A stale, pre-rename-shaped consumer: same durable name, old filter.
	if _, err := js.AddConsumer(testStream, &nats.ConsumerConfig{
		Durable:       targetDurable,
		FilterSubject: "legacy.subject",
		AckPolicy:     nats.AckExplicitPolicy,
	}); err != nil {
		t.Fatal(err)
	}

	if err := migrate(context.Background(), js, testStream, targetDurable); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	assertConverged(t, js)
}

func TestMigrateCreatesTheConsumerWhenItDoesNotExistYet(t *testing.T) {
	// Reproduces the state right after an operator deletes a mismatched
	// consumer out-of-band (or a prior migrate run was interrupted between
	// delete and recreate): ConsumerInfo returns ErrConsumerNotFound, and
	// migrate must still converge to the wanted config rather than treating
	// "already gone" as "nothing to do".
	js := newTestJetStream(t)

	if err := migrate(context.Background(), js, testStream, targetDurable); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	assertConverged(t, js)
}

func TestMigrateRefusesAConsumerWithPendingMessages(t *testing.T) {
	js := newTestJetStream(t)
	if _, err := js.AddConsumer(testStream, &nats.ConsumerConfig{
		Durable:       targetDurable,
		FilterSubject: "legacy.subject",
		AckPolicy:     nats.AckExplicitPolicy,
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := js.Publish("legacy.subject", []byte("payload")); err != nil {
		t.Fatal(err)
	}

	err := migrate(context.Background(), js, testStream, targetDurable)
	if err == nil {
		t.Fatal("expected migrate to refuse a consumer with a pending message")
	}
	if !strings.Contains(err.Error(), "pending") {
		t.Fatalf("expected a pending-message refusal, got: %v", err)
	}
	info, infoErr := js.ConsumerInfo(testStream, targetDurable)
	if infoErr != nil {
		t.Fatalf("consumer was deleted despite the refusal: %v", infoErr)
	}
	if info.Config.FilterSubject != "legacy.subject" {
		t.Fatalf("refused consumer was mutated: %+v", info.Config)
	}
}

func TestMigrateRefusesAConsumerWithUnackedDelivery(t *testing.T) {
	js := newTestJetStream(t)
	if _, err := js.Publish("legacy.subject", []byte("payload")); err != nil {
		t.Fatal(err)
	}
	sub, err := js.PullSubscribe("legacy.subject", targetDurable, nats.AckWait(time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	msgs, err := sub.Fetch(1, nats.MaxWait(2*time.Second))
	if err != nil || len(msgs) != 1 {
		t.Fatalf("fetch: %v (got %d messages)", err, len(msgs))
	}
	// Deliberately do not Ack: the message is now ack-pending, not merely
	// pending, exercising the OTHER half of the safety gate.

	err = migrate(context.Background(), js, testStream, targetDurable)
	if err == nil {
		t.Fatal("expected migrate to refuse a consumer with an unacked delivery")
	}
	if !strings.Contains(err.Error(), "ack-pending") {
		t.Fatalf("expected an ack-pending refusal, got: %v", err)
	}
}

func TestMigrateRejectsAnUnknownDurableName(t *testing.T) {
	js := newTestJetStream(t)
	err := migrate(context.Background(), js, testStream, "not-a-real-org-erasure-consumer")
	if err == nil {
		t.Fatal("expected an error for a durable name outside the known org-erasure set")
	}
	if !strings.Contains(err.Error(), "not a known org-erasure consumer") {
		t.Fatalf("expected an unknown-durable error, got: %v", err)
	}
}

func TestRunValidatesItsInputsBeforeConnecting(t *testing.T) {
	for name, args := range map[string][5]string{
		"missing stream":   {"", "d", "nats://x:4222", "u", "p"},
		"missing durable":  {"s", "", "nats://x:4222", "u", "p"},
		"missing url":      {"s", "d", "", "u", "p"},
		"missing user":     {"s", "d", "nats://x:4222", "", "p"},
		"missing password": {"s", "d", "nats://x:4222", "u", ""},
	} {
		t.Run(name, func(t *testing.T) {
			if err := run(args[0], args[1], args[2], args[3], args[4], "_INBOX.TEST"); err == nil {
				t.Fatal("expected a validation error")
			}
		})
	}
}
