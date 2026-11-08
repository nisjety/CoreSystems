package main

import (
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

// Contract tests for the run trace topology (WS10.S5). Like the rest of this
// package they run against an in-process broker built from the real
// deploy/nats.conf and provisioned by the real manifest.

const (
	quarryTraceUser = "quarry-trace-producer"
	quarryTraceBox  = "_INBOX.QUARRY_TRACE_PRODUCER"
)

// expectPermissionViolation waits for the broker's asynchronous refusal. A
// refused publish or subscribe is dropped on the server and surfaces only as an
// error on the connection's handler, never as a return value.
func expectPermissionViolation(t *testing.T, asyncErrors <-chan error, what string) {
	t.Helper()
	select {
	case err := <-asyncErrors:
		if !strings.Contains(strings.ToLower(err.Error()), "permissions violation") {
			t.Fatalf("%s: got an error that is not a permission violation: %v", what, err)
		}
	case <-time.After(3 * time.Second):
		t.Fatalf("%s: the broker accepted it, but the principal must be refused", what)
	}
}

func TestRunTraceStreamAndConsumerAreTheContract(t *testing.T) {
	stream := runTraceStream()
	if stream.Name != "MODEL_PLANE_RUN_TRACE" {
		t.Fatalf("stream name = %q", stream.Name)
	}
	if len(stream.Subjects) != 1 || stream.Subjects[0] != "mp.v1.trace.run.*" {
		t.Fatalf("subjects = %v, want exactly mp.v1.trace.run.* (one run id token)", stream.Subjects)
	}
	if stream.Retention != nats.LimitsPolicy || stream.Storage != nats.FileStorage ||
		stream.MaxAge != 48*time.Hour || stream.MaxBytes != 512<<20 ||
		stream.Replicas != 1 || stream.Discard != nats.DiscardOld {
		t.Fatalf("stream is not limits/file/48h/512MiB/1 replica/discard-old: %+v", stream)
	}

	consumers := runTraceConsumers()
	if len(consumers) != 1 {
		t.Fatalf("expected exactly session-core's consumer, got %d", len(consumers))
	}
	c := consumers[0]
	if c.stream != "MODEL_PLANE_RUN_TRACE" || c.config.Durable != "session-core-trace" {
		t.Fatalf("binding = %s/%s", c.stream, c.config.Durable)
	}
	if c.config.DeliverSubject != "" {
		t.Fatalf("session-core-trace must be a pull consumer, has deliver subject %q", c.config.DeliverSubject)
	}
	if c.config.AckPolicy != nats.AckExplicitPolicy || c.config.AckWait != 30*time.Second ||
		c.config.MaxDeliver != 5 || c.config.FilterSubject != "mp.v1.trace.run.*" ||
		c.config.ReplayPolicy != nats.ReplayInstantPolicy || c.config.DeliverPolicy != nats.DeliverAllPolicy {
		t.Fatalf("consumer is not explicit-ack/30s/max 5/instant/all: %+v", c.config)
	}
}

func TestQuarryTracePrincipalCanOnlyPublishTraceSubjects(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)

	t.Run("a trace publish is accepted and captured by the trace stream", func(t *testing.T) {
		connection, asyncErrors := connectAs(t, instance, quarryTraceUser, quarryTraceBox)
		js, err := connection.JetStream(nats.MaxWait(5 * time.Second))
		if err != nil {
			t.Fatal(err)
		}
		message := nats.NewMsg("mp.v1.trace.run.smoke")
		message.Data = []byte(`{"contract":"run-trace-probe"}`)
		message.Header.Set(nats.MsgIdHdr, "run-trace-probe:1")
		ack, err := js.PublishMsg(message)
		if err != nil {
			t.Fatalf("a trace publish got no acknowledgement: %v", err)
		}
		if ack.Stream != "MODEL_PLANE_RUN_TRACE" {
			t.Fatalf("captured by %q, want MODEL_PLANE_RUN_TRACE", ack.Stream)
		}
		select {
		case err := <-asyncErrors:
			t.Fatalf("unexpected async error on an allowed publish: %v", err)
		default:
		}
	})

	refusedPublishes := []string{
		"mp.v1.run.smoke.event",       // the run event stream
		"tools.completions.smoke",     // tool completions
		"verevon.audit.v2.model.x",    // audit observability
		"mp.v1.usage.smoke",           // usage ledger
		"mp.v1.orchestration.run",     // orchestration bus
		"mp.v1.trace.run.smoke.extra", // a dotted run id must not widen the subject
		"mp.v1.trace.runs.smoke",      // a look-alike prefix
		"$JS.API.STREAM.INFO.MODEL_PLANE_RUN_TRACE",
		"$JS.API.CONSUMER.DELETE.MODEL_PLANE_RUN_TRACE.session-core-trace",
		"$JS.API.STREAM.PURGE.MODEL_PLANE_RUN_TRACE",
	}
	for _, subject := range refusedPublishes {
		subject := subject
		t.Run("publish refused: "+subject, func(t *testing.T) {
			connection, asyncErrors := connectAs(t, instance, quarryTraceUser, quarryTraceBox)
			if err := connection.Publish(subject, []byte("x")); err != nil {
				t.Fatalf("local publish error: %v", err)
			}
			_ = connection.Flush()
			expectPermissionViolation(t, asyncErrors, "publish to "+subject)
		})
	}

	for _, subject := range []string{"mp.v1.>", "mp.v1.trace.run.*", "_INBOX.>", "_INBOX.SESSION_CORE_RUNTIME.>"} {
		subject := subject
		t.Run("subscribe refused: "+subject, func(t *testing.T) {
			connection, asyncErrors := connectAs(t, instance, quarryTraceUser, quarryTraceBox)
			if _, err := connection.Subscribe(subject, func(*nats.Msg) {}); err != nil {
				t.Fatalf("local subscribe error: %v", err)
			}
			_ = connection.Flush()
			expectPermissionViolation(t, asyncErrors, "subscribe to "+subject)
		})
	}
}

// session-core binds, fetches and acknowledges, and nothing more.
func TestSessionCoreCanDrainTheTraceStreamButNotAdministerIt(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)

	producer, _ := connectAs(t, instance, quarryTraceUser, quarryTraceBox)
	producerJS, err := producer.JetStream(nats.MaxWait(5 * time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := producerJS.Publish("mp.v1.trace.run.drain", []byte(`{"n":1}`), nats.MsgId("drain:1")); err != nil {
		t.Fatalf("seed publish: %v", err)
	}

	consumer, asyncErrors := connectAs(t, instance, sessionCoreUser, sessionCoreBox)
	js, err := consumer.JetStream(nats.MaxWait(5 * time.Second))
	if err != nil {
		t.Fatal(err)
	}
	sub, err := js.PullSubscribe("mp.v1.trace.run.*", "session-core-trace",
		nats.Bind("MODEL_PLANE_RUN_TRACE", "session-core-trace"))
	if err != nil {
		t.Fatalf("session-core cannot bind its trace consumer: %v", err)
	}
	messages, err := sub.Fetch(1, nats.MaxWait(5*time.Second))
	if err != nil || len(messages) != 1 {
		t.Fatalf("session-core cannot fetch from its trace consumer: %v (%d messages)", err, len(messages))
	}
	if err := messages[0].AckSync(); err != nil {
		t.Fatalf("session-core cannot acknowledge a trace message: %v", err)
	}
	info, err := js.ConsumerInfo("MODEL_PLANE_RUN_TRACE", "session-core-trace")
	if err != nil {
		t.Fatalf("session-core cannot read its consumer info: %v", err)
	}
	if info.NumPending != 0 || info.NumAckPending != 0 {
		t.Fatalf("after one ack nothing may be pending: %+v", info)
	}

	// No administration: it cannot create, delete or purge anything.
	if _, err := js.AddConsumer("MODEL_PLANE_RUN_TRACE", &nats.ConsumerConfig{Durable: "intruder", AckPolicy: nats.AckExplicitPolicy}); err == nil {
		t.Fatal("session-core must not be able to create a consumer on the trace stream")
	}
	if err := js.DeleteConsumer("MODEL_PLANE_RUN_TRACE", "session-core-trace"); err == nil {
		t.Fatal("session-core must not be able to delete its consumer")
	}
	if err := js.PurgeStream("MODEL_PLANE_RUN_TRACE"); err == nil {
		t.Fatal("session-core must not be able to purge the trace stream")
	}
	// And it cannot publish into the stream: that is the producer's grant.
	_ = consumer.Publish("mp.v1.trace.run.forged", []byte("x"))
	_ = consumer.Flush()
	expectViolationAmongErrors(t, asyncErrors)
}

// expectViolationAmongErrors drains the error channel: the refused admin calls
// above each leave a violation behind, so at least one must be there.
func expectViolationAmongErrors(t *testing.T, asyncErrors <-chan error) {
	t.Helper()
	deadline := time.After(3 * time.Second)
	for {
		select {
		case err := <-asyncErrors:
			if strings.Contains(strings.ToLower(err.Error()), "permissions violation") {
				return
			}
		case <-deadline:
			t.Fatal("expected a permissions violation for the refused publish")
		}
	}
}
