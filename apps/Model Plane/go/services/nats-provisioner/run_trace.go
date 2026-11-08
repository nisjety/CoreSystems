package main

import (
	"time"

	"github.com/nats-io/nats.go"
)

// The run trace topology on the Model Plane broker (WS10.S5).
//
// Quarry (quarry-edge) publishes a canonical envelope for each browser trace
// event of a Model Plane run to mp.v1.trace.run.<model_run_id> with a JetStream
// publish that waits for the PubAck. session-core drains the stream with a pull
// consumer and persists the events (WS10.S6).
//
// Rollout order matters and is why this lives in the provisioner, ahead of any
// publisher: (1) this stream, the quarry-trace-producer principal in
// deploy/nats.conf and the consumer exist with nothing publishing; (2) Quarry
// starts publishing and messages wait in the stream; (3) the session-core
// consumer drains them. A stream that loses messages after a 48 hour outage is
// by design: the trace is evidence, not a ledger, and Quarry numbers every event
// per (run, producer) so a gap stays visible to the reader.
const (
	// runTraceStreamName is the JetStream stream the trace events travel on.
	runTraceStreamName = "MODEL_PLANE_RUN_TRACE"

	// runTraceSubject carries one run id token: a run id with a dot cannot be
	// published, so a hostile producer value never widens the subject.
	runTraceSubject = "mp.v1.trace.run.*"

	// runTraceConsumerName is session-core's durable. The same name appears in
	// deploy/nats.conf (its JetStream API grants) and in session-core's
	// trace_nats.rs (WS10.S6); publishers_registry_test.go checks the latter.
	runTraceConsumerName = "session-core-trace"
)

// runTraceStream is the stream definition. Limits retention with a 48 hour age
// and a 512 MiB cap on file storage; DiscardOld drops the oldest message when
// full instead of refusing a publish.
func runTraceStream() nats.StreamConfig {
	return nats.StreamConfig{
		Name:      runTraceStreamName,
		Subjects:  []string{runTraceSubject},
		Retention: nats.LimitsPolicy,
		Storage:   nats.FileStorage,
		MaxAge:    48 * time.Hour,
		MaxBytes:  512 << 20,
		Replicas:  1,
		Discard:   nats.DiscardOld,
	}
}

// runTraceConsumers is session-core's pull consumer. It has no DeliverSubject
// on purpose: session-core fetches with $JS.API.CONSUMER.MSG.NEXT, which is the
// only consumer API its principal is granted.
func runTraceConsumers() []consumerBinding {
	return []consumerBinding{{
		stream: runTraceStreamName,
		config: nats.ConsumerConfig{
			Durable:       runTraceConsumerName,
			AckPolicy:     nats.AckExplicitPolicy,
			AckWait:       30 * time.Second,
			MaxDeliver:    5,
			FilterSubject: runTraceSubject,
			ReplayPolicy:  nats.ReplayInstantPolicy,
			DeliverPolicy: nats.DeliverAllPolicy,
		},
	}}
}
