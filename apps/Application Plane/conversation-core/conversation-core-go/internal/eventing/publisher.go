package eventing

import (
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/nats-io/nats.go"
)

const (
	StreamName          = "VELION_APPLICATION"
	ModelStreamName     = "VELION_MODEL"
	IngestionStreamName = "VELION_INGESTION"
)

type Publisher struct {
	js nats.JetStreamContext
}

func NewPublisher(js nats.JetStreamContext) *Publisher {
	return &Publisher{js: js}
}

func (p *Publisher) EnsureStream() error {
	if p == nil || p.js == nil {
		return nil
	}
	_, err := p.js.StreamInfo(StreamName)
	if err == nil {
		return nil
	}
	log.Printf("conversation-core-go: creating JetStream stream %s", StreamName)
	_, err = p.js.AddStream(&nats.StreamConfig{
		Name:      StreamName,
		Subjects:  []string{"velion.application.>"},
		Retention: nats.LimitsPolicy,
		MaxAge:    14 * 24 * time.Hour,
		MaxMsgs:   200_000,
		Storage:   nats.FileStorage,
	})
	return err
}

// EnsureModelStream creates the JetStream stream covering the model subject
// namespace (velion.model.>) so the model-proposed subject is durable. The
// application stream is left unchanged because the executed and send_failed
// events stay in the application namespace (velion.application.>).
func (p *Publisher) EnsureModelStream() error {
	if p == nil || p.js == nil {
		return nil
	}
	_, err := p.js.StreamInfo(ModelStreamName)
	if err == nil {
		return nil
	}
	log.Printf("conversation-core-go: creating JetStream stream %s", ModelStreamName)
	_, err = p.js.AddStream(&nats.StreamConfig{
		Name:      ModelStreamName,
		Subjects:  []string{"velion.model.>"},
		Retention: nats.LimitsPolicy,
		MaxAge:    14 * 24 * time.Hour,
		MaxMsgs:   200_000,
		Storage:   nats.FileStorage,
	})
	return err
}

// EnsureIngestionStream creates the JetStream stream covering the Ingestion
// Plane subject namespace (velion.ingestion.>). integration-corev2 publishes
// webhook_received (and other) events via plain core-NATS Publish, not
// js.Publish — but a JetStream stream captures any message published to a
// matching subject regardless of which API the publisher used, so this
// stream is what makes those events durably consumable (survives a
// conversation-core restart between publish and delivery) rather than
// fire-and-forget core-NATS pub/sub.
func (p *Publisher) EnsureIngestionStream() error {
	if p == nil || p.js == nil {
		return nil
	}
	_, err := p.js.StreamInfo(IngestionStreamName)
	if err == nil {
		return nil
	}
	log.Printf("conversation-core-go: creating JetStream stream %s", IngestionStreamName)
	_, err = p.js.AddStream(&nats.StreamConfig{
		Name:      IngestionStreamName,
		Subjects:  []string{"velion.ingestion.>"},
		Retention: nats.LimitsPolicy,
		MaxAge:    14 * 24 * time.Hour,
		MaxMsgs:   200_000,
		Storage:   nats.FileStorage,
	})
	return err
}

func (p *Publisher) Publish(ctx context.Context, subject string, payload any) error {
	if p == nil || p.js == nil {
		return nil
	}
	bytes, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	_, err = p.js.Publish(subject, bytes)
	return err
}
