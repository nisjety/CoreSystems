package nats

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/nats-io/nats.go"
)

type Subscriber struct {
	client  *Client
	service *billing.Service
}

func NewSubscriber(client *Client, service *billing.Service) *Subscriber {
	return &Subscriber{client: client, service: service}
}

func (s *Subscriber) Start(ctx context.Context) error {
	_, err := s.client.Subscribe("usage.>", func(msg *nats.Msg) {
		var payload struct {
			EventID    string                 `json:"event_id"`
			OrgID      string                 `json:"org_id"`
			Metric     string                 `json:"metric"`
			Quantity   float64                `json:"quantity"`
			Source     string                 `json:"source"`
			OccurredAt string                 `json:"occurred_at"`
			Metadata   map[string]interface{} `json:"metadata"`
		}

		if err := json.Unmarshal(msg.Data, &payload); err != nil {
			log.Printf("billing-core invalid usage event payload: %v", err)
			return
		}

		occurredAt := time.Now().UTC()
		if payload.OccurredAt != "" {
			if parsed, err := time.Parse(time.RFC3339, payload.OccurredAt); err == nil {
				occurredAt = parsed
			}
		}

		eventID := payload.EventID
		if eventID == "" {
			eventID = usageEventIDFromMsg(msg, payload.OrgID, payload.Metric, payload.Quantity, occurredAt, payload.Source)
		}

		if err := s.service.RecordUsage(ctx, billing.UsageEvent{
			EventID:    eventID,
			OrgID:      payload.OrgID,
			Metric:     payload.Metric,
			Quantity:   payload.Quantity,
			Source:     payload.Source,
			OccurredAt: occurredAt,
			Metadata:   payload.Metadata,
		}); err != nil {
			log.Printf("billing-core failed to process usage event: %v", err)
		}
	})
	if err != nil {
		return err
	}

	if _, err = s.client.Subscribe("organization.created", func(msg *nats.Msg) {
		payload, decodeErr := decodeEventData(msg.Data)
		if decodeErr != nil {
			log.Printf("billing-core invalid organization.created payload: %v", decodeErr)
			return
		}

		orgID := readString(payload, "organization_id", "organizationId")
		orgName := readString(payload, "name", "organization_name", "organizationName")
		if orgID == "" {
			return
		}

		if syncErr := s.service.SyncOrganization(ctx, orgID, orgName); syncErr != nil {
			log.Printf("billing-core failed to sync organization create: %v", syncErr)
		}
	}); err != nil {
		return err
	}

	if _, err = s.client.Subscribe("organization.updated", func(msg *nats.Msg) {
		payload, decodeErr := decodeEventData(msg.Data)
		if decodeErr != nil {
			log.Printf("billing-core invalid organization.updated payload: %v", decodeErr)
			return
		}

		orgID := readString(payload, "organization_id", "organizationId")
		if orgID == "" {
			return
		}
		changes := readMap(payload, "changes")
		orgName := readString(changes, "name")
		if orgName == "" {
			return
		}

		if syncErr := s.service.SyncOrganization(ctx, orgID, orgName); syncErr != nil {
			log.Printf("billing-core failed to sync organization update: %v", syncErr)
		}
	}); err != nil {
		return err
	}

	if _, err = s.client.Subscribe("organization.plan.changed", func(msg *nats.Msg) {
		payload, decodeErr := decodeEventData(msg.Data)
		if decodeErr != nil {
			log.Printf("billing-core invalid organization.plan.changed payload: %v", decodeErr)
			return
		}

		orgID := readString(payload, "organization_id", "organizationId")
		newPlan := readString(payload, "new_plan", "newPlan")
		orgName := readString(payload, "organization_name", "organizationName", "name")
		if orgID == "" || newPlan == "" {
			return
		}

		if syncErr := s.service.ApplyPlanChange(ctx, orgID, orgName, newPlan); syncErr != nil {
			log.Printf("billing-core failed to apply plan change: %v", syncErr)
		}
	}); err != nil {
		return err
	}

	if _, err = s.client.Subscribe("organization.deleted", func(msg *nats.Msg) {
		payload, decodeErr := decodeEventData(msg.Data)
		if decodeErr != nil {
			log.Printf("billing-core invalid organization.deleted payload: %v", decodeErr)
			return
		}

		orgID := readString(payload, "organization_id", "organizationId")
		if orgID == "" {
			return
		}

		if syncErr := s.service.DeactivateOrganization(ctx, orgID, "organization_deleted"); syncErr != nil {
			log.Printf("billing-core failed to deactivate deleted org account: %v", syncErr)
		}
	}); err != nil {
		return err
	}

	return err
}

func usageEventIDFromMsg(msg *nats.Msg, orgID, metric string, quantity float64, occurredAt time.Time, source string) string {
	if id := msg.Header.Get("Nats-Msg-Id"); id != "" {
		return id
	}
	raw := fmt.Sprintf("%s|%s|%s|%f|%d|%s", msg.Subject, orgID, metric, quantity, occurredAt.UnixNano(), source)
	sum := sha256.Sum256([]byte(raw))
	return "evt_" + hex.EncodeToString(sum[:])
}

func decodeEventData(body []byte) (map[string]interface{}, error) {
	var payload map[string]interface{}
	if err := json.Unmarshal(body, &payload); err != nil {
		return nil, err
	}
	if nested, ok := payload["data"].(map[string]interface{}); ok {
		return nested, nil
	}
	return payload, nil
}

func readString(payload map[string]interface{}, keys ...string) string {
	for _, key := range keys {
		if value, ok := payload[key].(string); ok && value != "" {
			return value
		}
	}
	return ""
}

func readMap(payload map[string]interface{}, key string) map[string]interface{} {
	if value, ok := payload[key].(map[string]interface{}); ok {
		return value
	}
	return map[string]interface{}{}
}
