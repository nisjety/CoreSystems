package eventing

import (
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/nats-io/nats.go"
)

const (
	StreamName      = "VELION_APPLICATION"
	ModelStreamName = "VELION_MODEL"
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
