package main

import (
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
)

// The audit observability topology on the Model Plane broker.
//
// audit-core (Control Plane) reads this plane's audit and usage events from this
// broker through two push consumers bound to one stream,
// VEREVON_CONTROL_OBSERVABILITY. session-core publishes its governed tool
// actions ("Used by AI?") to verevon.audit.v2.model.session-core.* with a
// JetStream publish that waits for the stream's PubAck.
//
// Until this file existed the stream was created only by the Control Plane's own
// optional provisioner (audit-extra-nats-provisioner), logging in here with its
// own copy of this broker's provisioner password. The two copies had drifted, so
// the broker refused that login on every run ("authentication error", seen in
// the broker log on each launcher run since 2026-09-14); because the bus is
// marked optional that provisioner logged a skip and exited 0, and nothing
// noticed. The stream was never created: every publish failed with "no stream
// found for given subject", and session-core's outbox ran 4,992 tool_action rows
// out of attempts (2026-09-04 to 2026-10-01) without one reaching audit-core.
//
// The stream and the two audit-core durables are therefore part of THIS
// manifest. This provisioner runs with the credential the broker itself was
// started with (one variable, one value) and session-core does not start until
// it has exited successfully, so the topology either exists or the stack stays
// down. manifest_contract_test.go publishes every JetStream publisher's real
// subject through this manifest, so a subject without a stream fails a test, not
// production.
//
// The values below must equal what audit-core's provisioner converges on this
// bus (audit-core/internal/provisioner, Provision for plane "model"): both can
// act on the same stream, and a difference would make each undo the other.
// TestObservabilityTopologyMatchesAuditCoreProvisioner reads that file and
// fails on any drift.
const (
	auditObservabilityStreamName = "VEREVON_CONTROL_OBSERVABILITY"

	// auditObservabilityPlane is the plane name audit-core uses for this bus; it
	// names the durables and delivery subjects below.
	auditObservabilityPlane = "model"

	// auditCoreDLQSubject is where audit-core parks events it cannot persist.
	auditCoreDLQSubject = "verevon.dlq.audit-core.>"
)

// auditObservabilityStream is the stream audit and usage events travel on.
// Subject order is part of the contract (ensureStream compares it).
func auditObservabilityStream() nats.StreamConfig {
	return nats.StreamConfig{
		Name: auditObservabilityStreamName,
		Subjects: []string{
			fmt.Sprintf("verevon.audit.v1.%s.>", auditObservabilityPlane),
			fmt.Sprintf("verevon.usage.v1.%s.>", auditObservabilityPlane),
			fmt.Sprintf("verevon.audit.v2.%s.>", auditObservabilityPlane),
			fmt.Sprintf("verevon.usage.v2.%s.>", auditObservabilityPlane),
			auditCoreDLQSubject,
		},
		Retention: nats.LimitsPolicy,
		Storage:   nats.FileStorage,
		MaxAge:    30 * 24 * time.Hour,
		MaxMsgs:   2_000_000,
		Replicas:  1,
		Discard:   nats.DiscardOld,
	}
}

// auditObservabilityConsumers are audit-core's two push/queue durables on that
// stream (audit and usage). audit-core binds them with nats.Bind and cannot
// create them: its broker principal may only bind, acknowledge and publish to
// its dead-letter subjects.
func auditObservabilityConsumers() []consumerBinding {
	kinds := []string{"audit", "usage"}
	bindings := make([]consumerBinding, 0, len(kinds))
	for _, kind := range kinds {
		durable := fmt.Sprintf("audit-core-%s-v3-%s", auditObservabilityPlane, kind)
		bindings = append(bindings, consumerBinding{
			stream: auditObservabilityStreamName,
			config: nats.ConsumerConfig{
				Durable:        durable,
				DeliverSubject: fmt.Sprintf("_VEREVON.AUDIT.DELIVER.%s.%s-v2", auditObservabilityPlane, kind),
				DeliverGroup:   durable,
				AckPolicy:      nats.AckExplicitPolicy,
				AckWait:        30 * time.Second,
				MaxDeliver:     5,
				FilterSubject:  fmt.Sprintf("verevon.%s.v2.%s.>", kind, auditObservabilityPlane),
				ReplayPolicy:   nats.ReplayInstantPolicy,
				DeliverPolicy:  nats.DeliverAllPolicy,
			},
		})
	}
	return bindings
}
