package nats

import (
	"context"
	"encoding/json"
	"log"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/nats-io/nats.go"
)

type BridgeSubscriber struct {
	client     *Client
	publisher  *Publisher
	orgService *orgcore.Service
}

func NewBridgeSubscriber(client *Client, publisher *Publisher, orgService *orgcore.Service) *BridgeSubscriber {
	return &BridgeSubscriber{client: client, publisher: publisher, orgService: orgService}
}

func (s *BridgeSubscriber) Start(ctx context.Context) error {
	_, err := s.client.Subscribe("auth.>", func(msg *nats.Msg) {
		s.handleAuthEvent(ctx, msg)
	})
	if err != nil {
		return err
	}

	log.Println("NATS bridge subscriber listening on auth.>")
	return nil
}

func (s *BridgeSubscriber) handleAuthEvent(ctx context.Context, msg *nats.Msg) {
	var payload map[string]any
	if err := json.Unmarshal(msg.Data, &payload); err != nil {
		log.Printf("skip invalid auth event payload: %v", err)
		return
	}

	subject := msg.Subject
	data := payload
	if nested, ok := payload["data"].(map[string]any); ok {
		data = nested
	}

	switch subject {
	case "auth.user.registered":
		_ = s.publisher.Publish(ctx, SubjectUserCreated, data)
	case "auth.user.profile_updated":
		_ = s.publisher.Publish(ctx, SubjectUserUpdated, data)
	case "auth.user.deleted":
		_ = s.publisher.Publish(ctx, SubjectUserDeleted, data)
	case "auth.session.created":
		_ = s.publisher.Publish(ctx, SubjectSessionCreated, data)
	case "auth.session.ended", "auth.user.logout":
		_ = s.publisher.Publish(ctx, SubjectSessionEnded, data)
	case "auth.organization.created":
		// Auth Core's transactional outbox reconciles canonical state directly
		// through the internal HTTP API. NATS is notification-only: mutating here
		// would create a second, unordered authority capable of restoring stale
		// organization or membership state after a deletion.
		_ = s.publisher.Publish(ctx, SubjectOrganizationCreated, data)
	case "auth.organization.member_added":
		_ = s.publisher.Publish(ctx, SubjectOrganizationMemberAdded, data)
	case "auth.organization.member_removed":
		_ = s.publisher.Publish(ctx, SubjectOrganizationMemberRemoved, data)
	}
}
