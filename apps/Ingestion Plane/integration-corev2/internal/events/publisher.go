package events

import (
	"context"
	"encoding/json"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/integration-corev2/internal/config"
)

type Event struct {
	ID             string         `json:"id"`
	Type           string         `json:"type"`
	Source         string         `json:"source"`
	Version        string         `json:"version"`
	OrganizationID string         `json:"organizationId"`
	WorkspaceID    string         `json:"workspaceId,omitempty"`
	UserID         string         `json:"userId,omitempty"`
	ConnectionID   string         `json:"connectionId,omitempty"`
	ProviderKey    string         `json:"providerKey,omitempty"`
	Data           map[string]any `json:"data,omitempty"`
	CreatedAt      time.Time      `json:"createdAt"`
}

type Publisher interface {
	Publish(ctx context.Context, event Event) error
}

type NoopPublisher struct{}

func (NoopPublisher) Publish(context.Context, Event) error {
	return nil
}

type NATSPublisher struct {
	conn          *nats.Conn
	subjectPrefix string
	source        string
}

func NewNATSPublisher(cfg config.Config) (*NATSPublisher, error) {
	options := []nats.Option{
		nats.Name(cfg.ServiceName),
		nats.Timeout(5 * time.Second),
	}
	if cfg.NATSToken != "" {
		// The shared cross-plane velion-nats broker enforces single-token
		// authorization (see nats-shared.conf), not username/password.
		options = append(options, nats.Token(cfg.NATSToken))
	} else if cfg.NATSUsername != "" || cfg.NATSPassword != "" {
		options = append(options, nats.UserInfo(cfg.NATSUsername, cfg.NATSPassword))
	}
	conn, err := nats.Connect(cfg.NATSURL, options...)
	if err != nil {
		return nil, err
	}
	return &NATSPublisher{
		conn:          conn,
		subjectPrefix: strings.TrimSpace(cfg.NATSSubjectPrefix),
		source:        cfg.ServiceName,
	}, nil
}

func (p *NATSPublisher) Publish(_ context.Context, event Event) error {
	if event.ID == "" {
		event.ID = "evt_" + uuid.NewString()
	}
	if event.Source == "" {
		event.Source = p.source
	}
	if event.Version == "" {
		event.Version = "1"
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	payload, err := json.Marshal(event)
	if err != nil {
		return err
	}
	return p.conn.Publish(p.subject(event.Type), payload)
}

func (p *NATSPublisher) Close() error {
	if p.conn == nil {
		return nil
	}
	return p.conn.Drain()
}

func (p *NATSPublisher) subject(eventType string) string {
	eventType = strings.Trim(eventType, ". ")
	prefix := strings.Trim(p.subjectPrefix, ". ")
	if prefix == "" {
		return eventType
	}
	return prefix + "." + eventType
}
