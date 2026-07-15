package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

// SharedPublisher publishes cross-plane domain events to the shared
// velion-nats broker on the aqencia.controlplane.* subject namespace.
//
// Subject convention: aqencia.controlplane.<entity>.<verb>
//
//	aqencia.controlplane.user.registered
//	aqencia.controlplane.user.updated
//	aqencia.controlplane.user.deleted
//	aqencia.controlplane.user.provider_linked
//	aqencia.controlplane.user.provider_ready_for_integration
type SharedPublisher struct {
	conn       *nats.Conn
	js         jetstream.JetStream
	gdprJS     gdprJetStreamPublisher
	sourceName string
}

type gdprJetStreamPublisher interface {
	PublishMsg(context.Context, *nats.Msg, ...jetstream.PublishOpt) (*jetstream.PubAck, error)
}

type SharedCredentials struct {
	User, Password, Token string
	AllowTokenFallback    bool
}

// NewSharedPublisher connects to the shared NATS broker and ensures the
// AQENCIA_CONTROLPLANE JetStream stream exists. Returns nil without error
// when sharedURL is empty (shared publishing disabled).
func NewSharedPublisher(sharedURL string, credentials SharedCredentials, clientName string) (*SharedPublisher, error) {
	if sharedURL == "" {
		log.Println("ℹ️  VELION_NATS_URL not set — cross-plane publishing disabled")
		return nil, nil
	}

	opts := []nats.Option{
		nats.Name(clientName + "-shared"),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(3 * time.Second),
		nats.DisconnectErrHandler(func(nc *nats.Conn, err error) {
			if err != nil {
				log.Printf("⚠️  Shared NATS disconnected: %v", err)
			}
		}),
		nats.ReconnectHandler(func(nc *nats.Conn) {
			log.Printf("🔄 Shared NATS reconnected: %s", nc.ConnectedUrl())
		}),
		nats.CustomInboxPrefix("_INBOX.USER_SHARED"),
	}
	user, password, token := strings.TrimSpace(credentials.User), strings.TrimSpace(credentials.Password), strings.TrimSpace(credentials.Token)
	if (user == "") != (password == "") {
		return nil, fmt.Errorf("shared NATS user/password must be configured together")
	}
	if user != "" {
		if len(password) < 32 {
			return nil, fmt.Errorf("shared NATS password must contain at least 32 characters")
		}
		opts = append(opts, nats.UserInfo(user, password))
	} else if token != "" {
		if !credentials.AllowTokenFallback {
			return nil, fmt.Errorf("shared NATS token fallback requires explicit enablement")
		}
		opts = append(opts, nats.Token(token))
	} else {
		return nil, fmt.Errorf("shared NATS scoped credentials are required")
	}

	conn, err := nats.Connect(sharedURL, opts...)
	if err != nil {
		return nil, fmt.Errorf("shared NATS connect failed: %w", err)
	}

	js, err := jetstream.New(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("shared NATS jetstream init failed: %w", err)
	}

	sp := &SharedPublisher{conn: conn, js: js, gdprJS: js, sourceName: clientName}
	log.Printf("✅ Connected to shared NATS: %s", sharedURL)
	return sp, nil
}

// Publish sends a cross-plane event to the shared JetStream.
// Fire-and-forget: logs errors but never panics.
func (sp *SharedPublisher) Publish(ctx context.Context, subject string, payload map[string]any) {
	if sp == nil || sp.conn == nil || !sp.conn.IsConnected() {
		return
	}

	enriched := make(map[string]any, len(payload)+2)
	for k, v := range payload {
		enriched[k] = v
	}
	enriched["_source"] = sp.sourceName
	enriched["_published_at"] = time.Now().UTC().Format(time.RFC3339Nano)

	data, err := json.Marshal(enriched)
	if err != nil {
		log.Printf("❌ SharedPublisher: marshal failed for %s: %v", subject, err)
		return
	}

	if _, err := sp.js.Publish(ctx, subject, data); err != nil {
		log.Printf("❌ SharedPublisher: publish %s failed: %v", subject, err)
		return
	}

	log.Printf("📡 SharedNATS → %s", subject)
}

// Close drains and closes the shared NATS connection.
func (sp *SharedPublisher) Close() {
	if sp != nil && sp.conn != nil {
		sp.conn.Drain() //nolint:errcheck
		log.Println("🔌 Shared NATS connection closed (user-core)")
	}
}

// ─── Typed helpers ────────────────────────────────────────────────────────────

// PublishUserRegistered publishes aqencia.controlplane.user.registered.
// Called by the event handler when a new user is created from auth events.
func (sp *SharedPublisher) PublishUserRegistered(ctx context.Context, userID, email, name, provider string) {
	sp.Publish(ctx, "aqencia.controlplane.user.registered", map[string]any{
		"user_id":  userID,
		"email":    email,
		"name":     name,
		"provider": provider,
	})
}

// PublishUserUpdated publishes aqencia.controlplane.user.updated.
func (sp *SharedPublisher) PublishUserUpdated(ctx context.Context, userID, email string, changes map[string]any) {
	sp.Publish(ctx, "aqencia.controlplane.user.updated", map[string]any{
		"user_id": userID,
		"email":   email,
		"changes": changes,
	})
}

// PublishUserDeleted publishes aqencia.controlplane.user.deleted.
func (sp *SharedPublisher) PublishUserDeleted(ctx context.Context, userID, email string) {
	sp.Publish(ctx, "aqencia.controlplane.user.deleted", map[string]any{
		"user_id": userID,
		"email":   email,
	})
}

// PublishProviderLinked publishes aqencia.controlplane.user.provider_linked.
// Critical event: Ingestion Plane subscribes to this to provision M365 sync.
func (sp *SharedPublisher) PublishProviderLinked(ctx context.Context, userID, email, provider, tenantID string) {
	sp.Publish(ctx, "aqencia.controlplane.user.provider_linked", map[string]any{
		"user_id":   userID,
		"email":     email,
		"provider":  provider,
		"tenant_id": tenantID,
	})
}

// PublishResourceGrantsChanged publishes aqencia.controlplane.acl.resource_grants.changed.
//
// This is the generalized grant-change event. Retrieval (Data Plane) subscribes
// to it to evict the affected (subject_id, org_id) authorization-cache key on
// revoke so a revoke takes effect within one query (the TTL is the backstop).
// action is "grant" or "revoke".
func (sp *SharedPublisher) PublishResourceGrantsChanged(
	ctx context.Context,
	grantID, orgID, resourceType, resourceID, subjectType, subjectID, role, action string,
) {
	sp.Publish(ctx, "aqencia.controlplane.acl.resource_grants.changed", map[string]any{
		"grant_id":      grantID,
		"org_id":        orgID,
		"resource_type": resourceType,
		"resource_id":   resourceID,
		"subject_type":  subjectType,
		"subject_id":    subjectID,
		"role":          role,
		"action":        action,
	})
}

// PublishDocumentAclChanged is the document-scoped back-compat wrapper around
// PublishResourceGrantsChanged. permission_level is normalized to a role.
func (sp *SharedPublisher) PublishDocumentAclChanged(ctx context.Context, aclID, orgID, documentID, userID, permissionLevel, action string) {
	role := "view"
	switch strings.ToLower(strings.TrimSpace(permissionLevel)) {
	case "write", "edit", "admin", "owner":
		role = "edit"
	}
	sp.PublishResourceGrantsChanged(ctx, aclID, orgID, "document", documentID, "user", userID, role, action)
}

// PublishPlain sends a plain NATS core message (not JetStream) to a subject.
// Used for notification-core subjects (notifications.*) which are subscribed
// via plain conn.Subscribe, not JetStream consumers.
func (sp *SharedPublisher) PublishPlain(subject string, payload map[string]any) {
	if sp == nil || sp.conn == nil || !sp.conn.IsConnected() {
		return
	}
	data, err := json.Marshal(payload)
	if err != nil {
		log.Printf("❌ SharedPublisher (user-core): plain marshal failed for %s: %v", subject, err)
		return
	}
	if err := sp.conn.Publish(subject, data); err != nil {
		log.Printf("❌ SharedPublisher (user-core): plain publish %s failed: %v", subject, err)
		return
	}
	log.Printf("📡 SharedNATS (user-core) plain → %s", subject)
}

// PublishGDPRErasure publishes a locally outboxed erasure intent and waits
// until the shared broker has accepted the write. The operation ledger keeps
// retrying this method after transport failures; Nats-Msg-Id gives durable
// stream configurations a stable deduplication key while plain subscribers
// continue receiving the established subject.
func (sp *SharedPublisher) PublishGDPRErasure(ctx context.Context, eventID string, payload []byte) error {
	if sp == nil || sp.gdprJS == nil {
		return fmt.Errorf("shared NATS is unavailable")
	}
	if strings.TrimSpace(eventID) == "" || !json.Valid(payload) {
		return fmt.Errorf("invalid GDPR fan-out event")
	}
	message := nats.NewMsg("velion.gdpr.erasure.requested")
	message.Header.Set("Nats-Msg-Id", eventID)
	message.Data = append([]byte(nil), payload...)
	ack, err := sp.gdprJS.PublishMsg(ctx, message)
	if err != nil {
		return fmt.Errorf("publish GDPR fan-out: %w", err)
	}
	if ack == nil || ack.Stream != "AQENCIA_CONTROLPLANE" || ack.Sequence == 0 {
		return fmt.Errorf("publish GDPR fan-out: invalid JetStream PubAck")
	}
	return nil
}

// PublishProviderReadyForIntegration publishes aqencia.controlplane.user.provider_ready_for_integration.
// This gives downstream planes enough org-scoped context to attach a provider link without a second lookup.
func (sp *SharedPublisher) PublishProviderReadyForIntegration(
	ctx context.Context,
	userID string,
	email string,
	provider string,
	providerAccountID string,
	tenantID string,
	microsoftTenantID string,
	scopesGranted []string,
	tokenRef string,
	orgID string,
	role string,
	onboardingStatus string,
) {
	sp.Publish(ctx, "aqencia.controlplane.user.provider_ready_for_integration", map[string]any{
		"user_id":             userID,
		"email":               email,
		"provider":            provider,
		"provider_account_id": providerAccountID,
		"tenant_id":           tenantID,
		"microsoft_tenant_id": microsoftTenantID,
		"scopes_granted":      scopesGranted,
		"token_ref":           tokenRef,
		"org_id":              orgID,
		"role":                role,
		"onboarding_status":   onboardingStatus,
	})
}
