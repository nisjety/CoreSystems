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
	sourceName string
}

// NewSharedPublisher connects to the shared NATS broker and ensures the
// AQENCIA_CONTROLPLANE JetStream stream exists. Returns nil without error
// when sharedURL is empty (shared publishing disabled).
func NewSharedPublisher(sharedURL, token, clientName string) (*SharedPublisher, error) {
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
	}

	if token != "" {
		opts = append(opts, nats.Token(token))
		log.Println("🔐 Shared NATS: using token authentication")
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

	sp := &SharedPublisher{conn: conn, js: js, sourceName: clientName}

	// Best-effort stream creation — auth-core may have already created it
	if err := sp.ensureStream(context.Background()); err != nil {
		log.Printf("⚠️  Shared NATS: AQENCIA_CONTROLPLANE stream setup: %v", err)
	}

	log.Printf("✅ Connected to shared NATS: %s", sharedURL)
	return sp, nil
}

func (sp *SharedPublisher) ensureStream(ctx context.Context) error {
	_, err := sp.js.CreateStream(ctx, jetstream.StreamConfig{
		Name:       "AQENCIA_CONTROLPLANE",
		Subjects:   []string{"aqencia.controlplane.>"},
		Retention:  jetstream.LimitsPolicy,
		MaxMsgs:    100_000,
		MaxAge:     14 * 24 * time.Hour,
		Storage:    jetstream.FileStorage,
		Duplicates: 60 * time.Second,
	})
	if err != nil {
		lower := strings.ToLower(err.Error())
		if strings.Contains(lower, "stream name already in use") ||
			strings.Contains(lower, "subjects overlap") ||
			strings.Contains(lower, "err_code=10058") ||
			strings.Contains(lower, "err_code=10065") {
			return nil // already exists — not an error
		}
		return err
	}
	log.Println("✅ Shared NATS: AQENCIA_CONTROLPLANE stream ready")
	return nil
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

// PublishDocumentAclChanged publishes aqencia.controlplane.acl.document.changed.
func (sp *SharedPublisher) PublishDocumentAclChanged(ctx context.Context, aclID, orgID, documentID, userID, permissionLevel, action string) {
	sp.Publish(ctx, "aqencia.controlplane.acl.document.changed", map[string]any{
		"acl_id":           aclID,
		"org_id":           orgID,
		"document_id":      documentID,
		"user_id":          userID,
		"permission_level": permissionLevel,
		"action":           action,
	})
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
