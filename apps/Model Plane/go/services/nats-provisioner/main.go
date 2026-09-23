// nats-provisioner owns the Model Plane JetStream topology.
//
// Runtime services deliberately have bind/pull/ack permissions only. This
// one-shot service is the deployment authority that creates the exact streams
// and consumers those services bind to, then exits successfully. Existing
// objects are checked for contract drift rather than silently updated.
package main

import (
	"errors"
	"fmt"
	"log/slog"
	"os"
	"time"

	"github.com/nats-io/nats.go"
)

type consumerBinding struct {
	stream string
	config nats.ConsumerConfig
}

func streamConfigs() []nats.StreamConfig {
	return []nats.StreamConfig{
		{
			Name:      "TOOLS_COMPLETIONS",
			Subjects:  []string{"tools.completions.*"},
			Retention: nats.WorkQueuePolicy,
			Storage:   nats.FileStorage,
			MaxAge:    24 * time.Hour,
			Replicas:  1,
			Discard:   nats.DiscardOld,
		},
		{
			Name:      "MP_ORCHESTRATION_EVENTS",
			Subjects:  []string{"mp.v1.orchestration.>"},
			Retention: nats.LimitsPolicy,
			Storage:   nats.FileStorage,
			MaxAge:    24 * time.Hour,
			Replicas:  1,
			Discard:   nats.DiscardOld,
		},
		{
			Name:      "MODEL_PLANE_RUN_EVENTS",
			Subjects:  []string{"mp.v1.run.*.event"},
			Retention: nats.LimitsPolicy,
			Storage:   nats.FileStorage,
			MaxAge:    48 * time.Hour,
			Replicas:  1,
			Discard:   nats.DiscardOld,
		},
	}
}

func consumerBindings() []consumerBinding {
	return []consumerBinding{
		{
			stream: "TOOLS_COMPLETIONS",
			config: nats.ConsumerConfig{
				Durable:       "session-core-tools",
				AckPolicy:     nats.AckExplicitPolicy,
				AckWait:       30 * time.Second,
				MaxDeliver:    5,
				FilterSubject: "tools.completions.*",
				ReplayPolicy:  nats.ReplayInstantPolicy,
				DeliverPolicy: nats.DeliverAllPolicy,
			},
		},
		{
			stream: "MP_ORCHESTRATION_EVENTS",
			config: nats.ConsumerConfig{
				Durable:       "session-core-orchestration",
				AckPolicy:     nats.AckNonePolicy,
				MaxDeliver:    -1,
				FilterSubject: "mp.v1.orchestration.>",
				ReplayPolicy:  nats.ReplayInstantPolicy,
				DeliverPolicy: nats.DeliverAllPolicy,
			},
		},
		{
			// DeliverSubject is required on every consumer in this file that a Go
			// consumer binds to via QueueSubscribe/Subscribe + nats.Bind (both
			// consumers on MODEL_PLANE_RUN_EVENTS, below): a JetStream consumer with
			// no DeliverSubject is a PULL consumer server-side, and nats.go's
			// push-style Subscribe/QueueSubscribe refuses to bind to one ("must use
			// pull subscribe to bind to pull based consumer") -- verified directly
			// against nats.go v1.37.0 (the pinned version) on 2026-09-17, reproducing
			// a LATENT BUG in capability-core-run-watch-notify's config that predates
			// this comment: it had shipped with no DeliverSubject and would have
			// failed to bind the moment capability-core's NATS connection ever
			// succeeded, which it had not (a separate connect-retry bug, fixed the
			// same day, meant this had never actually been exercised live).
			// AUTO-2 ("notify me when a run finishes"): capability-core's
			// internal/runwatch.Notifier binds this durable with manual ack (see
			// its package doc) and calls notification-core for every user who
			// watched a run once that run's terminal event arrives. Explicit ack
			// (not AckNonePolicy, unlike session-core-orchestration above)
			// because the side effect is an outbound cross-plane HTTP call that
			// can fail transiently — a Nak'd message must be redelivered rather
			// than silently dropped. DeliverNewPolicy: a fresh deployment does
			// not need to replay 48h of run history to find watchers that could
			// not have existed yet.
			stream: "MODEL_PLANE_RUN_EVENTS",
			config: nats.ConsumerConfig{
				Durable:        "capability-core-run-watch-notify",
				DeliverSubject: "_VEREVON.MODEL.DELIVER.capability.run_watch_notify",
				// A queue-bound push consumer (Notifier.Run subscribes via
				// QueueSubscribe) also needs DeliverGroup set server-side, or
				// the client's queue subscribe is refused with "cannot create
				// a queue subscription for a consumer without a deliver
				// group" -- verified directly in the same 2026-09-17 repro
				// that found the DeliverSubject gap above. Matches the queue
				// name Notifier.Run passes to QueueSubscribe (its own
				// Durable), by convention.
				DeliverGroup:  "capability-core-run-watch-notify",
				AckPolicy:     nats.AckExplicitPolicy,
				AckWait:       30 * time.Second,
				MaxDeliver:    5,
				FilterSubject: "mp.v1.run.*.event",
				ReplayPolicy:  nats.ReplayInstantPolicy,
				DeliverPolicy: nats.DeliverNewPolicy,
			},
		},
		{
			// G7 skill-learning review: capability-core's
			// internal/sessionreview.RunConsumer binds this durable with manual
			// ack and turns each RUN_COMPLETED into an LLM review (fetch
			// transcript -> review -> UpsertAgentSkill).
			//
			// This durable did not exist before 2026-09-17: RunConsumer instead
			// used a plain core-NATS nc.Subscribe, which receives a message only
			// while actively connected, with no persistence and no redelivery.
			// session-core's publisher (a real JetStream publish, acked) had
			// been durably delivering onto this stream regardless -- 88 of 88
			// publishes acked with none stuck -- but every one was lost the
			// moment capability-core was not live and subscribed at that exact
			// instant (a restart, a redeploy, a brief reconnect), which is the
			// ordinary case for a service's lifecycle, not a rare fault. The
			// result: agent_skills stayed empty despite 147 completed runs.
			//
			// DeliverAllPolicy, unlike the run-watch notify durable above: that
			// one deliberately skips history because a watcher could not have
			// registered before this deploy existed. Here the opposite is true --
			// the whole point of adding this durable is to recover the reviews
			// that were silently missed, so it replays everything the stream
			// still retains (MaxAge 48h) on first bind.
			//
			// AckWait is longer than the other two run-event consumers': this
			// handler's side effect is a real LLM review call (transcript fetch
			// + inference-core Infer + UpsertAgentSkill), not a cheap local
			// read/write or a single outbound HTTP call, so 30s risks a live
			// review being Nak'd by its own timeout mid-flight and redelivered
			// as a duplicate attempt.
			stream: "MODEL_PLANE_RUN_EVENTS",
			config: nats.ConsumerConfig{
				Durable:        "capability-core-skill-review",
				DeliverSubject: "_VEREVON.MODEL.DELIVER.capability.skill_review",
				// See the identical DeliverGroup note on
				// capability-core-run-watch-notify above: RunConsumer also
				// binds via QueueSubscribe, so this needs a matching
				// DeliverGroup too.
				DeliverGroup:  "capability-core-skill-review",
				AckPolicy:     nats.AckExplicitPolicy,
				AckWait:       120 * time.Second,
				MaxDeliver:    5,
				FilterSubject: "mp.v1.run.*.event",
				ReplayPolicy:  nats.ReplayInstantPolicy,
				DeliverPolicy: nats.DeliverAllPolicy,
			},
		},
	}
}

func main() {
	url := getenv("NATS_URL", "nats://nats:4222")
	user := getenv("NATS_USER", "observability-provisioner-model")
	password := os.Getenv("NATS_PASSWORD")
	if password == "" {
		slog.Error("NATS_PASSWORD is required")
		os.Exit(2)
	}

	var nc *nats.Conn
	var err error
	for attempt := 1; attempt <= 60; attempt++ {
		nc, err = nats.Connect(
			url,
			nats.UserInfo(user, password),
			nats.Timeout(3*time.Second),
			nats.CustomInboxPrefix("_INBOX.PROVISIONER_MODEL"),
		)
		if err == nil {
			break
		}
		slog.Warn("NATS not ready; retrying", "attempt", attempt, "error", err)
		time.Sleep(time.Second)
	}
	if err != nil {
		slog.Error("NATS connection failed", "error", err)
		os.Exit(1)
	}
	defer nc.Drain()
	js, err := nc.JetStream()
	if err != nil {
		slog.Error("JetStream context failed", "error", err)
		os.Exit(1)
	}

	for _, spec := range streamConfigs() {
		if err := ensureStream(js, spec); err != nil {
			slog.Error("stream contract failed", "stream", spec.Name, "error", err)
			os.Exit(1)
		}
	}
	for _, binding := range consumerBindings() {
		if err := ensureConsumer(js, binding); err != nil {
			slog.Error("consumer contract failed", "stream", binding.stream, "durable", binding.config.Durable, "error", err)
			os.Exit(1)
		}
	}
	slog.Info("Model Plane JetStream topology ready")
}

func ensureStream(js nats.JetStreamContext, expected nats.StreamConfig) error {
	info, err := js.StreamInfo(expected.Name)
	if errors.Is(err, nats.ErrStreamNotFound) {
		_, err = js.AddStream(&expected)
		return err
	}
	if err != nil {
		return err
	}
	actual := info.Config
	if actual.Name != expected.Name || !sameStrings(actual.Subjects, expected.Subjects) ||
		actual.Retention != expected.Retention || actual.Storage != expected.Storage ||
		actual.MaxAge != expected.MaxAge || actual.Discard != expected.Discard {
		return fmt.Errorf("existing config differs: got subjects=%v retention=%v storage=%v max_age=%s discard=%v",
			actual.Subjects, actual.Retention, actual.Storage, actual.MaxAge, actual.Discard)
	}
	return nil
}

func ensureConsumer(js nats.JetStreamContext, binding consumerBinding) error {
	info, err := js.ConsumerInfo(binding.stream, binding.config.Durable)
	if errors.Is(err, nats.ErrConsumerNotFound) {
		_, err = js.AddConsumer(binding.stream, &binding.config)
		return err
	}
	if err != nil {
		return err
	}
	actual := info.Config

	// A consumer whose DeliverSubject/DeliverGroup has drifted from the
	// current contract (empty because it predates push delivery entirely, or
	// non-empty but under an earlier naming convention) is safe to delete and
	// recreate PROVIDED it has zero prior deliveries: a consumer object only
	// tracks delivery position/ack state for ITS OWN prior deliveries, so
	// info.Delivered.Consumer == 0 means no client has ever actually received
	// a message through this exact durable — there is no progress to lose.
	// The underlying messages stay on the stream regardless; only this one
	// durable's (empty) delivery record is discarded. Semantic identity
	// fields (Durable/FilterSubject/AckPolicy/MaxDeliver) must still match,
	// so this never masks a deliberate operator change to what the consumer
	// actually does — only to how its messages get delivered.
	deliverySubjectDrift := actual.DeliverSubject != binding.config.DeliverSubject ||
		actual.DeliverGroup != binding.config.DeliverGroup
	if deliverySubjectDrift && info.Delivered.Consumer == 0 &&
		actual.Durable == binding.config.Durable && actual.FilterSubject == binding.config.FilterSubject &&
		actual.AckPolicy == binding.config.AckPolicy && actual.MaxDeliver == binding.config.MaxDeliver {
		slog.Warn("consumer's delivery subject/group predates current contract and has zero prior deliveries; recreating (safe: nothing to lose)",
			"stream", binding.stream, "durable", binding.config.Durable,
			"old_deliver_subject", actual.DeliverSubject, "new_deliver_subject", binding.config.DeliverSubject,
			"old_deliver_group", actual.DeliverGroup, "new_deliver_group", binding.config.DeliverGroup)
		if err := js.DeleteConsumer(binding.stream, binding.config.Durable); err != nil {
			return fmt.Errorf("recreate %s/%s: delete stale consumer: %w", binding.stream, binding.config.Durable, err)
		}
		_, err := js.AddConsumer(binding.stream, &binding.config)
		return err
	}

	if actual.Durable != binding.config.Durable || actual.FilterSubject != binding.config.FilterSubject ||
		actual.AckPolicy != binding.config.AckPolicy || actual.AckWait != binding.config.AckWait ||
		actual.MaxDeliver != binding.config.MaxDeliver || actual.DeliverSubject != binding.config.DeliverSubject ||
		actual.DeliverGroup != binding.config.DeliverGroup {
		return fmt.Errorf("existing config differs: got filter=%q ack=%v ack_wait=%s max_deliver=%d deliver_subject=%q deliver_group=%q",
			actual.FilterSubject, actual.AckPolicy, actual.AckWait, actual.MaxDeliver, actual.DeliverSubject, actual.DeliverGroup)
	}
	return nil
}

func sameStrings(left, right []string) bool {
	if len(left) != len(right) {
		return false
	}
	for index := range left {
		if left[index] != right[index] {
			return false
		}
	}
	return true
}

func getenv(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}
