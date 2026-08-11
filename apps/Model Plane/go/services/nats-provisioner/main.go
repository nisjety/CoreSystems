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
	if actual.Durable != binding.config.Durable || actual.FilterSubject != binding.config.FilterSubject ||
		actual.AckPolicy != binding.config.AckPolicy || actual.AckWait != binding.config.AckWait ||
		actual.MaxDeliver != binding.config.MaxDeliver {
		return fmt.Errorf("existing config differs: got filter=%q ack=%v ack_wait=%s max_deliver=%d",
			actual.FilterSubject, actual.AckPolicy, actual.AckWait, actual.MaxDeliver)
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
