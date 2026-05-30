package nats

import (
	"context"
	"log"
	"time"

	"github.com/nats-io/nats.go/jetstream"
)

// Publisher handles publishing user service events to NATS
type Publisher struct {
	client *Client
}

// NewPublisher creates a new event publisher
func NewPublisher(client *Client) *Publisher {
	return &Publisher{
		client: client,
	}
}

// PublishUserCreated publishes a user created event
func (p *Publisher) PublishUserCreated(ctx context.Context, userID, email, name, status string, metadata map[string]interface{}) error {
	event := UserCreatedEvent{
		Type:      SubjectUserCreated,
		UserID:    userID,
		Email:     email,
		Name:      name,
		Status:    status,
		Metadata:  metadata,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectUserCreated, event); err != nil {
		log.Printf("❌ Failed to publish user created event: %v", err)
		return err
	}

	log.Printf("📤 Published user created: %s", email)
	return nil
}

// PublishUserUpdated publishes a user updated event
func (p *Publisher) PublishUserUpdated(ctx context.Context, userID, email string, changes map[string]interface{}) error {
	event := UserUpdatedEvent{
		Type:      SubjectUserUpdated,
		UserID:    userID,
		Email:     email,
		Changes:   changes,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectUserUpdated, event); err != nil {
		log.Printf("❌ Failed to publish user updated event: %v", err)
		return err
	}

	log.Printf("📤 Published user updated: %s", email)
	return nil
}

// PublishUserDeleted publishes a user deleted event
func (p *Publisher) PublishUserDeleted(ctx context.Context, userID, email string) error {
	event := UserDeletedEvent{
		Type:      SubjectUserDeleted,
		UserID:    userID,
		Email:     email,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectUserDeleted, event); err != nil {
		log.Printf("❌ Failed to publish user deleted event: %v", err)
		return err
	}

	log.Printf("📤 Published user deleted: %s", email)
	return nil
}

// PublishUserStatusChanged publishes a user status changed event
func (p *Publisher) PublishUserStatusChanged(ctx context.Context, userID, email, oldStatus, newStatus, reason string) error {
	event := UserStatusChangedEvent{
		Type:      getStatusChangeSubject(newStatus),
		UserID:    userID,
		Email:     email,
		OldStatus: oldStatus,
		NewStatus: newStatus,
		Reason:    reason,
		Timestamp: time.Now(),
	}

	subject := getStatusChangeSubject(newStatus)
	if err := p.client.PublishJetStream(ctx, subject, event); err != nil {
		log.Printf("❌ Failed to publish user status changed event: %v", err)
		return err
	}

	log.Printf("📤 Published user status changed: %s -> %s", oldStatus, newStatus)
	return nil
}

// PublishProfileUpdated publishes a user profile updated event
func (p *Publisher) PublishProfileUpdated(ctx context.Context, userID string, changes map[string]interface{}) error {
	event := UserProfileUpdatedEventOut{
		Type:      SubjectProfileUpdated,
		UserID:    userID,
		Changes:   changes,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectProfileUpdated, event); err != nil {
		log.Printf("❌ Failed to publish profile updated event: %v", err)
		return err
	}

	log.Printf("📤 Published profile updated: %s", userID)
	return nil
}

// PublishSessionCreated publishes a session created event
func (p *Publisher) PublishSessionCreated(ctx context.Context, sessionID, userID, deviceInfo, ipAddress, userAgent string, expiresAt time.Time) error {
	event := UserSessionCreatedEvent{
		Type:       SubjectSessionCreated,
		SessionID:  sessionID,
		UserID:     userID,
		DeviceInfo: deviceInfo,
		IPAddress:  ipAddress,
		UserAgent:  userAgent,
		ExpiresAt:  expiresAt,
		Timestamp:  time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectSessionCreated, event); err != nil {
		log.Printf("❌ Failed to publish session created event: %v", err)
		return err
	}

	log.Printf("📤 Published session created: %s", sessionID)
	return nil
}

// PublishActivityLogged publishes an activity logged event
func (p *Publisher) PublishActivityLogged(ctx context.Context, userID, action, resource, ipAddress, userAgent string, details map[string]interface{}) error {
	event := UserActivityLoggedEvent{
		Type:      SubjectActivityLogged,
		UserID:    userID,
		Action:    action,
		Resource:  resource,
		Details:   details,
		IPAddress: ipAddress,
		UserAgent: userAgent,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectActivityLogged, event); err != nil {
		log.Printf("❌ Failed to publish activity logged event: %v", err)
		return err
	}

	log.Printf("📤 Published activity logged: %s - %s", userID, action)
	return nil
}

// PublishRoleAssigned publishes a role assigned event
func (p *Publisher) PublishRoleAssigned(ctx context.Context, userID, roleID, roleName string) error {
	event := UserRoleAssignedEvent{
		Type:      SubjectRoleAssigned,
		UserID:    userID,
		RoleID:    roleID,
		RoleName:  roleName,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectRoleAssigned, event); err != nil {
		log.Printf("❌ Failed to publish role assigned event: %v", err)
		return err
	}

	log.Printf("📤 Published role assigned: %s -> %s", userID, roleName)
	return nil
}

// PublishRoleRemoved publishes a role removed event
func (p *Publisher) PublishRoleRemoved(ctx context.Context, userID, roleID, roleName string) error {
	event := UserRoleRemovedEvent{
		Type:      SubjectRoleRemoved,
		UserID:    userID,
		RoleID:    roleID,
		RoleName:  roleName,
		Timestamp: time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectRoleRemoved, event); err != nil {
		log.Printf("❌ Failed to publish role removed event: %v", err)
		return err
	}

	log.Printf("📤 Published role removed: %s -> %s", userID, roleName)
	return nil
}

// PublishDeviceRegistered publishes a device registered event
func (p *Publisher) PublishDeviceRegistered(ctx context.Context, userID, deviceID, deviceName, deviceType string) error {
	event := UserDeviceRegisteredEvent{
		Type:       SubjectDeviceRegistered,
		UserID:     userID,
		DeviceID:   deviceID,
		DeviceName: deviceName,
		DeviceType: deviceType,
		Timestamp:  time.Now(),
	}

	if err := p.client.PublishJetStream(ctx, SubjectDeviceRegistered, event); err != nil {
		log.Printf("❌ Failed to publish device registered event: %v", err)
		return err
	}

	log.Printf("📤 Published device registered: %s", deviceID)
	return nil
}

// Helper function to get the appropriate subject based on status
func getStatusChangeSubject(newStatus string) string {
	switch newStatus {
	case "blocked":
		return SubjectUserBlocked
	case "active":
		return SubjectUserActivated
	case "inactive":
		return SubjectUserDeactivated
	case "suspended":
		return SubjectUserSuspended
	default:
		return SubjectUserUpdated
	}
}

// EnsureUserEventsStream ensures the USER_EVENTS stream exists
func (p *Publisher) EnsureUserEventsStream(ctx context.Context) error {
	streamConfig := jetstream.StreamConfig{
		Name:       StreamUserEvents,
		Subjects:   []string{"user.>"},
		Retention:  jetstream.LimitsPolicy,
		MaxMsgs:    50000,
		MaxAge:     30 * 24 * time.Hour, // 30 days
		Storage:    jetstream.FileStorage,
		Duplicates: 1 * time.Minute,
	}

	return p.client.CreateStream(ctx, streamConfig)
}
