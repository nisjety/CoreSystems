package orchestration

import (
	"context"
	"fmt"
	"log/slog"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/protobuf/encoding/protojson"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
)

// EventPersister persists a decoded orchestration event.
//
// Implementations are responsible for any storage, projection or downstream
// fanout of the event. The subscriber emits telemetry counters around this
// call but does not interpret persistence-level errors beyond logging them.
type EventPersister interface {
	PersistOrchestrationEvent(ctx context.Context, ev *mpv1.OrchestrationEvent) error
}

// LoggingPersister is a no-op persister that logs the consumed event. It is
// used until a real session-core projection is wired in.
type LoggingPersister struct {
	logger *slog.Logger
}

// NewLoggingPersister returns a persister that simply logs each event.
func NewLoggingPersister(logger *slog.Logger) *LoggingPersister {
	if logger == nil {
		logger = slog.Default()
	}
	return &LoggingPersister{logger: logger}
}

// PersistOrchestrationEvent logs the event metadata.
func (p *LoggingPersister) PersistOrchestrationEvent(ctx context.Context, ev *mpv1.OrchestrationEvent) error {
	if ev == nil {
		return nil
	}
	p.logger.Info("orchestration event consumed",
		"event_type", EventTypeOf(ev),
		"subject", SubjectOf(ev),
	)
	return nil
}

// Subscriber decodes orchestration envelopes from NATS and forwards them to
// an EventPersister, emitting consumed-event telemetry.
type Subscriber struct {
	persister EventPersister
	logger    *slog.Logger
}

// NewSubscriber constructs a Subscriber.
func NewSubscriber(persister EventPersister, logger *slog.Logger) *Subscriber {
	if logger == nil {
		logger = slog.Default()
	}
	return &Subscriber{persister: persister, logger: logger}
}

// HandleEvent processes a single envelope payload received on a
// `mp.v1.orchestration.*` subject. It returns an error only when the caller
// should treat the message as failed (e.g. for redelivery). Counter outcomes
// are emitted for every terminal state.
func (s *Subscriber) HandleEvent(ctx context.Context, subject string, data []byte) error {
	env, err := envelope.Decode(data)
	if err != nil {
		s.emit(ctx, "", "decode_error")
		s.logger.Error("orchestration envelope decode failed", "subject", subject, "error", err)
		return fmt.Errorf("decode envelope: %w", err)
	}

	if err := env.Validate(); err != nil {
		s.emit(ctx, env.EventType, "validate_error")
		s.logger.Error("orchestration envelope validation failed",
			"subject", subject,
			"event_type", env.EventType,
			"error", err,
		)
		return fmt.Errorf("validate envelope: %w", err)
	}

	var ev mpv1.OrchestrationEvent
	if err := protojson.Unmarshal(env.Payload, &ev); err != nil {
		s.emit(ctx, env.EventType, "unmarshal_error")
		s.logger.Error("orchestration payload unmarshal failed",
			"subject", subject,
			"event_type", env.EventType,
			"error", err,
		)
		return fmt.Errorf("unmarshal payload: %w", err)
	}

	eventType := EventTypeOf(&ev)
	if eventType == "" {
		s.emit(ctx, env.EventType, "unknown_event")
		s.logger.Warn("orchestration event has no recognized type",
			"subject", subject,
			"envelope_event_type", env.EventType,
		)
		return nil
	}

	if err := s.persister.PersistOrchestrationEvent(ctx, &ev); err != nil {
		s.emit(ctx, eventType, "persist_error")
		s.logger.Error("orchestration event persist failed",
			"subject", subject,
			"event_type", eventType,
			"error", err,
		)
		return fmt.Errorf("persist event: %w", err)
	}

	s.emit(ctx, eventType, "ok")
	return nil
}

func (s *Subscriber) emit(ctx context.Context, eventType, outcome string) {
	telemetry.OrchestrationEventsConsumedTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("event_type", eventType),
		attribute.String("outcome", outcome),
	))
}
