package nats

import (
	"context"
	"encoding/json"
	"log"

	"github.com/nats-io/nats.go"
)

// EventHandler defines the interface for handling events
type EventHandler interface {
	HandleUserRegistered(ctx context.Context, event *UserRegisteredEvent) error
	HandleUserLogin(ctx context.Context, event *UserLoginEvent) error
	HandleUserLogout(ctx context.Context, event *UserLogoutEvent) error
	HandleUserProfileUpdated(ctx context.Context, event *UserProfileUpdatedEvent) error
	HandleSessionCreated(ctx context.Context, event *SessionCreatedEvent) error
	HandleSessionEnded(ctx context.Context, event *SessionEndedEvent) error
	HandleUserProviderLinked(ctx context.Context, event *UserProviderLinkedEvent) error
	HandleOrganizationMemberAdded(ctx context.Context, event *OrganizationMembershipEvent) error
	HandleOrganizationMemberRemoved(ctx context.Context, event *OrganizationMembershipEvent) error
}

// Subscriber handles NATS event subscriptions
type Subscriber struct {
	client  *Client
	handler EventHandler
	subs    []*nats.Subscription
}

// NewSubscriber creates a new event subscriber
func NewSubscriber(client *Client, handler EventHandler) *Subscriber {
	return &Subscriber{
		client:  client,
		handler: handler,
		subs:    make([]*nats.Subscription, 0),
	}
}

// Start starts listening to auth events
func (s *Subscriber) Start(_ context.Context) error {
	log.Println("🎧 Starting NATS event subscriber...")

	// Subscribe to auth events using core NATS (simpler than JetStream for this use case)
	if err := s.subscribeToAuthEvents(); err != nil {
		return err
	}

	log.Println("✅ NATS event subscriber started successfully")
	return nil
}

// subscribe registers handler for subject and tracks the subscription for Stop.
func (s *Subscriber) subscribe(subject string, handler nats.MsgHandler) error {
	sub, err := s.client.Subscribe(subject, handler)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub)
	return nil
}

// subscribeToAuthEvents subscribes to all auth event patterns
func (s *Subscriber) subscribeToAuthEvents() error {
	subscriptions := []struct {
		subject string
		handler nats.MsgHandler
	}{
		{SubjectAuthUserRegistered, s.handleUserRegistered},
		{SubjectAuthUserLogin, s.handleUserLogin},
		{SubjectAuthUserLogout, s.handleUserLogout},
		{SubjectAuthUserProfileUpdated, s.handleUserProfileUpdated},
		{SubjectAuthSessionCreated, s.handleSessionCreated},
		{SubjectAuthSessionEnded, s.handleSessionEnded},
		// Account linking
		{SubjectAuthUserProviderLinked, s.handleUserProviderLinked},
		// Organization membership events from both auth-core and org-core.
		{SubjectAuthOrganizationMemberAdded, s.handleOrganizationMemberAdded},
		{SubjectAuthOrganizationMemberRemoved, s.handleOrganizationMemberRemoved},
		{SubjectOrganizationMemberAdded, s.handleOrganizationMemberAdded},
		{SubjectOrganizationMemberRemoved, s.handleOrganizationMemberRemoved},
	}
	for _, subscription := range subscriptions {
		if err := s.subscribe(subscription.subject, subscription.handler); err != nil {
			return err
		}
	}

	return nil
}

// Message handlers

func (s *Subscriber) handleUserRegistered(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthUserRegistered)

	var event UserRegisteredEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal user registered event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleUserRegistered(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle user registered event: %v", err)
		return
	}

	log.Printf("✅ Processed user registered: %s", event.Email)
}

func (s *Subscriber) handleUserLogin(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthUserLogin)

	var event UserLoginEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal user login event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleUserLogin(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle user login event: %v", err)
		return
	}

	log.Printf("✅ Processed user login: %s", event.Email)
}

func (s *Subscriber) handleUserLogout(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthUserLogout)

	var event UserLogoutEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal user logout event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleUserLogout(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle user logout event: %v", err)
		return
	}

	log.Printf("✅ Processed user logout: %s", event.Email)
}

func (s *Subscriber) handleUserProfileUpdated(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthUserProfileUpdated)

	var event UserProfileUpdatedEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal user profile updated event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleUserProfileUpdated(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle user profile updated event: %v", err)
		return
	}

	log.Printf("✅ Processed user profile updated: %s", event.Email)
}

func (s *Subscriber) handleSessionCreated(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthSessionCreated)

	var event SessionCreatedEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal session created event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleSessionCreated(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle session created event: %v", err)
		return
	}

	log.Printf("✅ Processed session created: %s", event.SessionID)
}

func (s *Subscriber) handleSessionEnded(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthSessionEnded)

	var event SessionEndedEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal session ended event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleSessionEnded(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle session ended event: %v", err)
		return
	}

	log.Printf("✅ Processed session ended: %s", event.SessionID)
}

func (s *Subscriber) handleUserProviderLinked(msg *nats.Msg) {
	log.Printf("📨 Received: %s", SubjectAuthUserProviderLinked)

	var event UserProviderLinkedEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("❌ Failed to unmarshal user provider linked event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleUserProviderLinked(ctx, &event); err != nil {
		log.Printf("❌ Failed to handle user provider linked event: %v", err)
		return
	}

	log.Printf("✅ Processed provider linked: %s → %s", event.Email, event.Provider)
}

func (s *Subscriber) handleOrganizationMemberAdded(msg *nats.Msg) {
	log.Printf("📨 Received: %s", msg.Subject)

	event, err := decodeOrganizationMembershipEvent(msg.Data)
	if err != nil {
		log.Printf("❌ Failed to decode organization member added event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleOrganizationMemberAdded(ctx, event); err != nil {
		log.Printf("❌ Failed to handle organization member added event: %v", err)
		return
	}

	log.Printf("✅ Processed organization member added: user=%s org=%s", event.UserID, event.OrganizationID)
}

func (s *Subscriber) handleOrganizationMemberRemoved(msg *nats.Msg) {
	log.Printf("📨 Received: %s", msg.Subject)

	event, err := decodeOrganizationMembershipEvent(msg.Data)
	if err != nil {
		log.Printf("❌ Failed to decode organization member removed event: %v", err)
		return
	}

	ctx := context.Background()
	if err := s.handler.HandleOrganizationMemberRemoved(ctx, event); err != nil {
		log.Printf("❌ Failed to handle organization member removed event: %v", err)
		return
	}

	log.Printf("✅ Processed organization member removed: user=%s org=%s", event.UserID, event.OrganizationID)
}

func decodeOrganizationMembershipEvent(payload []byte) (*OrganizationMembershipEvent, error) {
	var raw map[string]any
	if err := json.Unmarshal(payload, &raw); err != nil {
		return nil, err
	}

	data := raw
	if nested, ok := raw["data"].(map[string]any); ok {
		data = nested
	}

	return &OrganizationMembershipEvent{
		OrganizationID:   coalesceString(data["organization_id"], data["organizationId"], raw["organization_id"], raw["organizationId"]),
		OrganizationName: coalesceString(data["organization_name"], data["organizationName"], raw["organization_name"], raw["organizationName"]),
		UserID:           coalesceString(data["user_id"], data["userId"], raw["user_id"], raw["userId"]),
		UserEmail:        coalesceString(data["user_email"], data["userEmail"], raw["user_email"], raw["userEmail"]),
		Role:             coalesceString(data["role"], raw["role"]),
	}, nil
}

func coalesceString(values ...any) string {
	for _, value := range values {
		if str, ok := value.(string); ok && str != "" {
			return str
		}
	}
	return ""
}

// Stop stops all subscriptions
func (s *Subscriber) Stop() {
	log.Println("🛑 Stopping NATS event subscriber...")

	for _, sub := range s.subs {
		if err := sub.Unsubscribe(); err != nil {
			log.Printf("⚠️  Error unsubscribing: %v", err)
		}
	}

	log.Println("✅ NATS event subscriber stopped")
}
