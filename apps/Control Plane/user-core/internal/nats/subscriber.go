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
func (s *Subscriber) Start(ctx context.Context) error {
	log.Println("🎧 Starting NATS event subscriber...")

	// Subscribe to auth events using core NATS (simpler than JetStream for this use case)
	if err := s.subscribeToAuthEvents(); err != nil {
		return err
	}

	log.Println("✅ NATS event subscriber started successfully")
	return nil
}

// subscribeToAuthEvents subscribes to all auth event patterns
func (s *Subscriber) subscribeToAuthEvents() error {
	// Subscribe to user registered events
	sub1, err := s.client.Subscribe(SubjectAuthUserRegistered, s.handleUserRegistered)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub1)

	// Subscribe to user login events
	sub2, err := s.client.Subscribe(SubjectAuthUserLogin, s.handleUserLogin)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub2)

	// Subscribe to user logout events
	sub3, err := s.client.Subscribe(SubjectAuthUserLogout, s.handleUserLogout)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub3)

	// Subscribe to user profile updated events
	sub4, err := s.client.Subscribe(SubjectAuthUserProfileUpdated, s.handleUserProfileUpdated)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub4)

	// Subscribe to session created events
	sub5, err := s.client.Subscribe(SubjectAuthSessionCreated, s.handleSessionCreated)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub5)

	// Subscribe to session ended events
	sub6, err := s.client.Subscribe(SubjectAuthSessionEnded, s.handleSessionEnded)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub6)

	// Subscribe to provider linked events (account linking)
	sub7, err := s.client.Subscribe(SubjectAuthUserProviderLinked, s.handleUserProviderLinked)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub7)

	// Subscribe to organization membership events from both auth-core and org-core.
	sub8, err := s.client.Subscribe(SubjectAuthOrganizationMemberAdded, s.handleOrganizationMemberAdded)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub8)

	sub9, err := s.client.Subscribe(SubjectAuthOrganizationMemberRemoved, s.handleOrganizationMemberRemoved)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub9)

	sub10, err := s.client.Subscribe(SubjectOrganizationMemberAdded, s.handleOrganizationMemberAdded)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub10)

	sub11, err := s.client.Subscribe(SubjectOrganizationMemberRemoved, s.handleOrganizationMemberRemoved)
	if err != nil {
		return err
	}
	s.subs = append(s.subs, sub11)

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
	var raw map[string]interface{}
	if err := json.Unmarshal(payload, &raw); err != nil {
		return nil, err
	}

	data := raw
	if nested, ok := raw["data"].(map[string]interface{}); ok {
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

func coalesceString(values ...interface{}) string {
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
