package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

const modelActionProposedDurable = "conversation-core-model-action-proposed"

// ActionProposer is the narrow surface the model-proposed consumer needs:
// queue a model-proposed action into the HITL review queue. *conversation.Service
// satisfies it (with kind-allowlist validation).
type ActionProposer interface {
	CreateAIAction(ctx context.Context, input conversation.CreateAIActionInput) (*conversation.AIAction, error)
}

// ModelActionProposedConsumer bridges the Model Plane → Application Plane: when a
// model (or hook) publishes velion.model.action.proposed, it queues a suggested
// AIAction into the HITL review queue so a human can approve it end-to-end. This
// closes the propose leg of propose→approve→act.
//
// Plane rule: this consumer lives in conversation-core (Application Plane). It
// only enqueues a suggestion; nothing is executed until a human approves and the
// AIActionExecutor runs.
type ModelActionProposedConsumer struct {
	consumer *DurableConsumer
	service  ActionProposer
}

func NewModelActionProposedConsumer(js nats.JetStreamContext, service ActionProposer) *ModelActionProposedConsumer {
	return &ModelActionProposedConsumer{
		consumer: NewDurableConsumer(js, "model-action-proposed"),
		service:  service,
	}
}

// Start binds the durable consumer on the model action proposed subject. The
// model stream must exist first (see eventing.EnsureModelStream).
func (c *ModelActionProposedConsumer) Start(_ context.Context) error {
	return c.consumer.Bind(conversation.SubjectModelActionProposed, modelActionProposedDurable, c.handle)
}

// Stop drains the subscription.
func (c *ModelActionProposedConsumer) Stop() { c.consumer.Stop() }

func (c *ModelActionProposedConsumer) handle(msg *nats.Msg) {
	var ev conversation.LifecycleEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		// Poison message — ack to avoid an infinite redelivery loop.
		log.Printf("[cc-go/model-action-proposed] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	switch c.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/model-action-proposed] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[cc-go/model-action-proposed] ack: %v", err)
		}
	}
}

// process decodes one proposed event and enqueues a suggested AIAction. Malformed
// or invalid events ack (terminal no-op); only a transient store error retries.
// It is the testable core (no NATS required).
func (c *ModelActionProposedConsumer) process(ctx context.Context, ev conversation.LifecycleEvent) outcome {
	orgID := strings.TrimSpace(ev.OrgID)
	conversationID := strings.TrimSpace(ev.ConversationID)
	if conversationID == "" {
		conversationID = stringFromData(ev.Data, "conversation_id")
	}
	kind := stringFromData(ev.Data, "kind")
	if orgID == "" || conversationID == "" || kind == "" {
		log.Printf("[cc-go/model-action-proposed] malformed event (missing org_id/conversation_id/kind); skipping")
		return outcomeAck
	}

	_, err := c.service.CreateAIAction(ctx, conversation.CreateAIActionInput{
		OrgID:          orgID,
		ConversationID: conversationID,
		Kind:           kind,
		Payload:        mapFromData(ev.Data, "payload"),
		CreatedBy:      "model-plane",
	})
	if err != nil {
		// Validation errors (e.g. disallowed kind) are terminal — acking avoids a
		// poison-redelivery loop. Only an infrastructure/store error retries.
		if conversation.IsInvalidInput(err) {
			log.Printf("[cc-go/model-action-proposed] invalid proposed action (org=%s kind=%s): %v", orgID, kind, err)
			return outcomeAck
		}
		log.Printf("[cc-go/model-action-proposed] create ai_action (org=%s kind=%s): %v", orgID, kind, err)
		return outcomeRetry
	}
	return outcomeAck
}
