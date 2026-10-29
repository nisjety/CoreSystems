package orchestration

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/google/uuid"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/protobuf/encoding/protojson"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
)

// SchemaVersion is the canonical envelope schema version emitted by orchestrator-core.
const SchemaVersion uint32 = 1

// PublishParams carries the per-event metadata required to construct a
// canonical Envelope. Caller-supplied identifiers must already be validated.
type PublishParams struct {
	OrgID          string
	UserID         string
	CorrelationID  string
	CausationID    string
	ResourceRef    string
	IdempotencyKey string
}

// Service publishes OrchestrationEvent payloads as canonical envelopes onto
// the mp.v1.orchestration.* NATS subjects.
type Service struct {
	pub    natsx.RawPublisher
	logger *slog.Logger
}

// NewService constructs a Service. The publisher and logger must be non-nil.
func NewService(pub natsx.RawPublisher, logger *slog.Logger) *Service {
	if logger == nil {
		logger = slog.Default()
	}
	return &Service{pub: pub, logger: logger}
}

// Publish marshals the OrchestrationEvent into a canonical Envelope and
// publishes it to the subject derived from the event variant.
func (s *Service) Publish(ctx context.Context, ev *mpv1.OrchestrationEvent, p PublishParams) error {
	if ev == nil {
		return errors.New("orchestration: event is nil")
	}
	if s.pub == nil {
		s.logger.Debug("orchestration publish skipped: no publisher configured", slog.String("event_type", EventTypeOf(ev)))
		return nil
	}
	eventType := EventTypeOf(ev)
	subject := SubjectOf(ev)
	if eventType == "" || subject == "" {
		return fmt.Errorf("orchestration: unsupported event variant %T", ev.Event)
	}

	payload, err := protojson.Marshal(ev)
	if err != nil {
		return fmt.Errorf("orchestration: marshal event: %w", err)
	}

	idemHash := envelope.DeriveIdempotencyHash(Producer, eventType, p.ResourceRef, p.IdempotencyKey)

	env := envelope.Envelope{
		EventID:        uuid.NewString(),
		EventType:      eventType,
		SchemaVersion:  SchemaVersion,
		Ts:             time.Now().UTC(),
		Producer:       Producer,
		CorrelationID:  p.CorrelationID,
		CausationID:    p.CausationID,
		IdempotencyKey: idemHash,
		OrgID:          p.OrgID,
		UserID:         p.UserID,
		ResourceRef:    p.ResourceRef,
		Payload:        payload,
	}
	if err := env.Validate(); err != nil {
		return fmt.Errorf("orchestration: invalid envelope: %w", err)
	}

	data, err := env.Encode()
	if err != nil {
		return fmt.Errorf("orchestration: encode envelope: %w", err)
	}

	if err := s.pub.Publish(subject, data); err != nil {
		return fmt.Errorf("orchestration: publish %s: %w", subject, err)
	}

	telemetry.OrchestrationEventsPublishedTotal.Add(ctx, 1,
		metric.WithAttributes(attribute.String("event_type", eventType)))
	s.logger.Debug("orchestration event published",
		slog.String("event_type", eventType),
		slog.String("subject", subject),
		slog.String("event_id", env.EventID))
	return nil
}
