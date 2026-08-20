package nats

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/nats-io/nats.go"
)

type Subscriber struct {
	client      *Client
	service     *billing.Service
	planApplier planChangeApplier
}

const (
	planChangeSubject     = "organization.plan.changed"
	planChangeConsumer    = "billing-core-organization-plan-changed"
	planChangeDLQSubject  = "billing.dead_letter.organization_plan_changed"
	planChangeMaxDelivery = 5
)

type planChangeApplier interface {
	ApplyOrganizationPlanChange(context.Context, string, string, string, int64) (bool, error)
}

func NewSubscriber(client *Client, service *billing.Service) *Subscriber {
	return &Subscriber{client: client, service: service, planApplier: service}
}

func (s *Subscriber) Start(ctx context.Context) error {
	_, err := s.client.Subscribe("usage.>", func(msg *nats.Msg) {
		usage, err := decodeUsageMessage(msg)
		if err != nil {
			log.Printf("billing-core invalid usage event payload: %v", err)
			return
		}
		if err := s.service.RecordUsage(ctx, usage); err != nil {
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

	// D-A: mirror billing-consolidation grants. Published directly by auth-core,
	// which owns the org_group_grant tables -- unlike organization.created/
	// .updated above, which reach this bus as org-core re-broadcasts. All three
	// services share NATS_URL=nats://controlplane-nats:4222, which is what makes
	// the subject reachable.
	//
	// Plain subscribe rather than the JetStream plan-change consumer: a missed
	// grant notification is a staleness problem the next change corrects, and
	// until then the read path falls back to the org's own plan -- it can never
	// grant a tier nobody paid for. Losing an inheritance is visible to the
	// customer; wrongly granting one is not.
	if _, err = s.client.Subscribe("organization.billing_group.changed", func(msg *nats.Msg) {
		payload, decodeErr := decodeEventData(msg.Data)
		if decodeErr != nil {
			log.Printf("billing-core invalid organization.billing_group.changed payload: %v", decodeErr)
			return
		}

		orgID := readString(payload, "organization_id", "organizationId")
		if orgID == "" {
			return
		}
		hostOrgID := readString(payload, "host_organization_id", "hostOrganizationId")
		orgGroupID := readString(payload, "org_group_id", "orgGroupId")
		consolidation := readBool(payload, "billing_consolidation", "billingConsolidation")

		if applyErr := s.service.ApplyBillingGroupChange(
			ctx, orgID, hostOrgID, orgGroupID, consolidation,
		); applyErr != nil {
			log.Printf("billing-core failed to apply billing group change: %v", applyErr)
		}
	}); err != nil {
		return err
	}

	if err = s.startPlanChangeConsumer(ctx); err != nil {
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

func (s *Subscriber) startPlanChangeConsumer(ctx context.Context) error {
	if s.planApplier == nil {
		return fmt.Errorf("plan change applier is required")
	}
	js, err := s.client.conn.JetStream()
	if err != nil {
		return fmt.Errorf("open plan change JetStream context: %w", err)
	}
	_, err = js.QueueSubscribe(
		planChangeSubject,
		planChangeConsumer,
		s.handlePlanChange(ctx),
		nats.Bind("CONTROL_PLANE_EVENTS", planChangeConsumer),
	)
	if err != nil {
		return fmt.Errorf("subscribe durable organization plan changes: %w", err)
	}
	return nil
}

func (s *Subscriber) handlePlanChange(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		payload, err := decodeEventData(msg.Data)
		if err != nil {
			s.deadLetterPlanChange(ctx, msg, "malformed", err)
			return
		}
		orgID := readString(payload, "organization_id", "organizationId")
		newPlan := readString(payload, "new_plan", "newPlan")
		orgName := readString(payload, "organization_name", "organizationName", "name")
		revision, revisionOK := readPositiveInt64(payload, "revision")
		if orgID == "" || newPlan == "" || !revisionOK {
			s.deadLetterPlanChange(ctx, msg, "malformed", fmt.Errorf("org_id, new_plan, and positive revision are required"))
			return
		}

		_, err = s.planApplier.ApplyOrganizationPlanChange(ctx, orgID, orgName, newPlan, revision)
		if err == nil || errors.Is(err, billing.ErrOrganizationDeleted) {
			if ackErr := msg.Ack(); ackErr != nil {
				log.Printf("billing-core failed to ack organization plan revision %d for %s: %v", revision, orgID, ackErr)
			}
			return
		}
		metadata, metadataErr := msg.Metadata()
		if metadataErr == nil && metadata.NumDelivered >= planChangeMaxDelivery {
			s.deadLetterPlanChange(ctx, msg, "retries_exhausted", err)
			return
		}
		log.Printf("billing-core retrying organization plan revision %d for %s: %v", revision, orgID, err)
		if nakErr := msg.NakWithDelay(250 * time.Millisecond); nakErr != nil {
			log.Printf("billing-core failed to NAK organization plan revision %d for %s: %v", revision, orgID, nakErr)
		}
	}
}

func (s *Subscriber) deadLetterPlanChange(ctx context.Context, msg *nats.Msg, reason string, eventErr error) {
	deliveries := uint64(0)
	if metadata, err := msg.Metadata(); err == nil {
		deliveries = metadata.NumDelivered
	}
	payload := map[string]any{
		"original_subject": msg.Subject,
		"original_data":    msg.Data,
		"reason":           reason,
		"error":            eventErr.Error(),
		"deliveries":       deliveries,
		"failed_at":        time.Now().UTC().Format(time.RFC3339Nano),
	}
	if err := s.client.Publish(ctx, planChangeDLQSubject, payload); err != nil {
		log.Printf("billing-core failed to publish plan change DLQ: %v", err)
		if nakErr := msg.NakWithDelay(time.Second); nakErr != nil {
			log.Printf("billing-core failed to NAK plan change after DLQ failure: %v", nakErr)
		}
		return
	}
	if err := msg.Term(); err != nil {
		log.Printf("billing-core failed to terminate dead-lettered plan change: %v", err)
	}
}

func decodeUsageMessage(msg *nats.Msg) (billing.UsageEvent, error) {
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
		return billing.UsageEvent{}, fmt.Errorf("decode usage event: %w", err)
	}
	if payload.OccurredAt == "" {
		return billing.UsageEvent{}, fmt.Errorf("occurred_at is required")
	}
	occurredAt, err := time.Parse(time.RFC3339, payload.OccurredAt)
	if err != nil {
		return billing.UsageEvent{}, fmt.Errorf("occurred_at must be RFC3339: %w", err)
	}
	eventID := payload.EventID
	if eventID == "" {
		eventID = msg.Header.Get("Nats-Msg-Id")
	}
	if eventID == "" {
		sum := sha256.Sum256(msg.Data)
		eventID = "evt_" + hex.EncodeToString(sum[:])
	}
	usage := billing.UsageEvent{
		EventID:    eventID,
		OrgID:      payload.OrgID,
		Metric:     payload.Metric,
		Quantity:   payload.Quantity,
		Source:     payload.Source,
		OccurredAt: occurredAt,
		Metadata:   payload.Metadata,
	}
	if err := billing.ValidateUsageEvent(usage); err != nil {
		return billing.UsageEvent{}, err
	}
	return usage, nil
}

func decodeEventData(body []byte) (map[string]interface{}, error) {
	var payload map[string]interface{}
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	if err := decoder.Decode(&payload); err != nil {
		return nil, err
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		if err == nil {
			return nil, fmt.Errorf("multiple JSON values are not allowed")
		}
		return nil, err
	}
	if nested, ok := payload["data"].(map[string]interface{}); ok {
		return nested, nil
	}
	return payload, nil
}

func readPositiveInt64(payload map[string]interface{}, key string) (int64, bool) {
	value, exists := payload[key]
	if !exists {
		return 0, false
	}
	switch typed := value.(type) {
	case json.Number:
		revision, err := typed.Int64()
		return revision, err == nil && revision > 0
	case float64:
		revision := int64(typed)
		return revision, float64(revision) == typed && revision > 0
	case int64:
		return typed, typed > 0
	default:
		return 0, false
	}
}

func readString(payload map[string]interface{}, keys ...string) string {
	for _, key := range keys {
		if value, ok := payload[key].(string); ok && value != "" {
			return value
		}
	}
	return ""
}

// readBool defaults to false, so a payload that omits the flag is treated as
// "not granted" rather than silently granting inheritance.
func readBool(payload map[string]interface{}, keys ...string) bool {
	for _, key := range keys {
		if value, ok := payload[key].(bool); ok {
			return value
		}
	}
	return false
}

func readMap(payload map[string]interface{}, key string) map[string]interface{} {
	if value, ok := payload[key].(map[string]interface{}); ok {
		return value
	}
	return map[string]interface{}{}
}
