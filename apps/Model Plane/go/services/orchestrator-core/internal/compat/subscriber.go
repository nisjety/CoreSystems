// Package compat provides a compatibility subscriber that consumes legacy NATS
// subjects and republishes them as new mp.v1 envelopes.
//
// This is only active in the dev profile to support incremental cutover.
package compat

import (
	"context"
	"encoding/json"
	"log/slog"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
)

// Publisher is the interface for publishing to NATS (abstracted for testing).
type Publisher interface {
	Publish(subject string, data []byte) error
}

// Subscriber consumes legacy subjects and republishes as new envelopes.
type Subscriber struct {
	publisher Publisher
	logger    *slog.Logger
}

// NewSubscriber creates a new compatibility subscriber.
func NewSubscriber(publisher Publisher, logger *slog.Logger) *Subscriber {
	return &Subscriber{
		publisher: publisher,
		logger:    logger,
	}
}

// producerCompatAdapter is the canonical Producer tag on envelopes the compat
// subscriber republishes. Used as a self-loop guard so that when the attached
// Publisher is in ModeDualWrite (v1 → v1 + legacy fan-out), the mirrored legacy
// arrival of our own republish is ignored instead of re-translated forever.
const producerCompatAdapter = "compat-adapter"

// HandleLegacyMessage translates a legacy NATS message to a new envelope and republishes it.
//
// Self-loop guard (PR-6): when ModeDualWrite is active, a v1 publish fans out
// to the legacy mirror subject. The compat subscriber listens on that mirror;
// without this guard, it would re-translate its own prior republish and feed
// it back into the publisher, creating an infinite loop. We detect that case
// by inspecting the incoming payload's `producer` field and skipping.
func (s *Subscriber) HandleLegacyMessage(ctx context.Context, legacySubject string, data []byte) error {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "HandleLegacyMessage")))

	newSubject := natsx.TranslateLegacySubject(legacySubject)
	if newSubject == legacySubject {
		s.logger.Debug("no mapping for legacy subject", "subject", legacySubject)
		telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "unmapped")))
		return nil
	}

	// Parse legacy payload to extract correlation fields.
	var legacyPayload map[string]interface{}
	if err := json.Unmarshal(data, &legacyPayload); err != nil {
		s.logger.Warn("failed to parse legacy payload", "subject", legacySubject, "error", err)
		telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "parse_error")))
		return err
	}

	// Self-loop guard (two forms):
	//   1. Own mirror: producer == "compat-adapter" means we already republished
	//      this payload; we're seeing it again via dual-write fan-out.
	//   2. V1 traffic seen via legacy mirror: schema_version > 0 means the
	//      payload is already a new-format envelope. In ModeDualWrite, any
	//      v1-native emitter's publish fans out to the legacy mirror, and we
	//      must NOT re-translate that — doing so would deliver the event twice
	//      to v1 consumers.
	if getStringField(legacyPayload, "producer", "") == producerCompatAdapter {
		s.logger.Debug("dropping self-produced mirror", "subject", legacySubject)
		telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "self_loop_skipped")))
		return nil
	}
	if sv, ok := legacyPayload["schema_version"].(float64); ok && sv > 0 {
		s.logger.Debug("dropping v1-encoded payload seen via legacy mirror",
			"subject", legacySubject, "schema_version", sv)
		telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "v1_mirror_skipped")))
		return nil
	}

	// Build new envelope preserving lineage.
	env := &envelope.Envelope{
		EventID:       getStringField(legacyPayload, "event_id", "compat-"+time.Now().Format("20060102150405")),
		EventType:     getStringField(legacyPayload, "event_type", legacySubject),
		SchemaVersion: 1,
		Ts:            time.Now().UTC(),
		Producer:      producerCompatAdapter,
		CorrelationID: getStringField(legacyPayload, "correlation_id", ""),
		CausationID:   getStringField(legacyPayload, "causation_id", ""),
		OrgID:         getStringField(legacyPayload, "org_id", ""),
		UserID:        getStringField(legacyPayload, "user_id", ""),
		ResourceRef:   getStringField(legacyPayload, "run_id", ""),
		Payload:       data,
	}

	encoded, err := env.Encode()
	if err != nil {
		return err
	}

	if err := s.publisher.Publish(newSubject, encoded); err != nil {
		s.logger.Error("failed to republish", "new_subject", newSubject, "error", err)
		telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "publish_error")))
		return err
	}

	s.logger.Info("republished legacy event", "from", legacySubject, "to", newSubject)
	telemetry.LegacyEventsTranslatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "translated")))
	return nil
}

func getStringField(m map[string]interface{}, key, fallback string) string {
	if v, ok := m[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return fallback
}
