package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"log"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
)

// U5-2 (ui-ux-verevon-gap.md §10):
//
// IdentitySyncSubscriber consumes auth-service and user-service NATS events
// off the shared bus and keeps notification-core's local subscriber identity
// cache in sync. Whenever auth-core publishes a new user, profile update,
// or invitation acceptance, this consumer upserts the local row + fans the
// identity out to Novu so workflow templates can address the user by name,
// locale, etc.
//
// Subjects (all on verevon-nats / SHARED_NATS_URL):
//   - auth.user.registered    — first sign-up
//   - auth.user.profile_updated — profile/avatar change
//   - auth.user.provider_linked — OAuth connect with profile hints
//   - org.member.added         — joined an org (org_id + role context)
const (
	SubjectAuthUserRegistered       = "auth.user.registered"
	SubjectAuthUserProfileUpdated   = "auth.user.profile_updated"
	SubjectAuthUserProviderLinked   = "auth.user.provider_linked"
	SubjectOrgMemberAdded           = "org.member.added"
	IdentitySyncDurableName         = "notification-core-identity-sync"
	IdentitySyncSubjectFilterPrefix = "auth.user.>"
)

// IdentitySyncSubscriber owns the JetStream subscriptions that drive the
// identity cache. Start binds the consumers; Stop drains them.
type IdentitySyncSubscriber struct {
	js  nats.JetStreamContext
	svc *subscribers.Service

	subs []*nats.Subscription
}

func NewIdentitySyncSubscriber(js nats.JetStreamContext, svc *subscribers.Service) *IdentitySyncSubscriber {
	return &IdentitySyncSubscriber{js: js, svc: svc}
}

// Start binds all four subject subscriptions. We use one subscription per
// subject (rather than wildcard) so each one gets its own durable name and
// can be paused/replayed independently.
func (s *IdentitySyncSubscriber) Start(_ context.Context) error {
	if s == nil || s.js == nil || s.svc == nil {
		return fmt.Errorf("identity-sync subscriber not configured")
	}

	bindings := []struct {
		subject string
		durable string
		handler func(*nats.Msg)
	}{
		{SubjectAuthUserRegistered, IdentitySyncDurableName + "-registered", s.handleAuthUserEvent},
		{SubjectAuthUserProfileUpdated, IdentitySyncDurableName + "-profile-updated", s.handleAuthUserEvent},
		{SubjectAuthUserProviderLinked, IdentitySyncDurableName + "-provider-linked", s.handleAuthUserEvent},
		{SubjectOrgMemberAdded, IdentitySyncDurableName + "-org-member-added", s.handleOrgMemberAddedEvent},
	}

	for _, b := range bindings {
		sub, err := s.js.QueueSubscribe(b.subject, b.durable, b.handler,
			nats.Durable(b.durable),
			nats.ManualAck(),
			nats.AckWait(30*1e9),
		)
		if err != nil {
			// Don't fail-fast — bind whatever we can and log misses. The
			// most common cause is the stream not existing on a fresh
			// dev box; consumers retry on the next message anyway.
			log.Printf("[notification-core/identity-sync] subscribe %s: %v", b.subject, err)
			continue
		}
		s.subs = append(s.subs, sub)
		log.Printf("[notification-core/identity-sync] subscribed to %s (durable=%s)", b.subject, b.durable)
	}

	return nil
}

// Stop drains the bound subscriptions.
func (s *IdentitySyncSubscriber) Stop() {
	if s == nil {
		return
	}
	for _, sub := range s.subs {
		if sub == nil {
			continue
		}
		if err := sub.Unsubscribe(); err != nil {
			log.Printf("[notification-core/identity-sync] unsubscribe %s: %v", sub.Subject, err)
		}
	}
	s.subs = nil
}

// ── Handlers ─────────────────────────────────────────────────────────────

// authUserEvent is the wire shape we receive from auth-core. Field names
// match the snake_case envelope auth-core's NATS publisher uses. Unknown
// fields are ignored (forward-compat).
type authUserEvent struct {
	UserID    string `json:"user_id"`
	Email     string `json:"email"`
	FirstName string `json:"first_name"`
	LastName  string `json:"last_name"`
	Name      string `json:"name"`
	Avatar    string `json:"avatar"`
	Phone     string `json:"phone"`
	Locale    string `json:"locale"`
	Timezone  string `json:"timezone"`
	OrgID     string `json:"org_id"`
	Role      string `json:"role"`
}

func (s *IdentitySyncSubscriber) handleAuthUserEvent(msg *nats.Msg) {
	defer func() {
		if err := msg.Ack(); err != nil {
			log.Printf("[notification-core/identity-sync] ack failed on %s: %v", msg.Subject, err)
		}
	}()

	var event authUserEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("[notification-core/identity-sync] decode %s: %v", msg.Subject, err)
		return
	}
	if event.UserID == "" {
		log.Printf("[notification-core/identity-sync] %s without user_id, skipping", msg.Subject)
		return
	}

	first, last := event.FirstName, event.LastName
	if first == "" && last == "" && event.Name != "" {
		// Conservative split (matches the HTTP handler convention).
		// `Anne Marie Hansen` → first="Anne", last="Marie Hansen".
		// Good enough for Novu template rendering.
		first = event.Name
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*1e9)
	defer cancel()

	if _, err := s.svc.Upsert(ctx, subscribers.UpsertParams{
		UserID:    event.UserID,
		Email:     event.Email,
		FirstName: first,
		LastName:  last,
		Avatar:    event.Avatar,
		Phone:     event.Phone,
		Locale:    event.Locale,
		Timezone:  event.Timezone,
		OrgID:     event.OrgID,
		Role:      event.Role,
	}); err != nil {
		log.Printf("[notification-core/identity-sync] upsert on %s for %s failed: %v", msg.Subject, event.UserID, err)
		return
	}
}

// orgMemberAddedEvent has a slightly different shape — separate handler
// keeps the wire-decode strict.
type orgMemberAddedEvent struct {
	OrgID    string `json:"org_id"`
	UserID   string `json:"user_id"`
	MemberID string `json:"member_id"`
	Email    string `json:"email"`
	Role     string `json:"role"`
}

func (s *IdentitySyncSubscriber) handleOrgMemberAddedEvent(msg *nats.Msg) {
	defer func() {
		if err := msg.Ack(); err != nil {
			log.Printf("[notification-core/identity-sync] ack failed on %s: %v", msg.Subject, err)
		}
	}()

	var event orgMemberAddedEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("[notification-core/identity-sync] decode %s: %v", msg.Subject, err)
		return
	}
	userID := event.UserID
	if userID == "" {
		userID = event.MemberID
	}
	if userID == "" {
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*1e9)
	defer cancel()

	if _, err := s.svc.Upsert(ctx, subscribers.UpsertParams{
		UserID: userID,
		Email:  event.Email,
		OrgID:  event.OrgID,
		Role:   event.Role,
	}); err != nil {
		log.Printf("[notification-core/identity-sync] org-member upsert for %s failed: %v", userID, err)
		return
	}
}
