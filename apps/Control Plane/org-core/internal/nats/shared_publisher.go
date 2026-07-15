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
//	aqencia.controlplane.org.created
//	aqencia.controlplane.org.updated
//	aqencia.controlplane.org.deleted
//	aqencia.controlplane.org.member_added
//	aqencia.controlplane.org.member_removed
//	aqencia.controlplane.org.plan_changed
type SharedPublisher struct {
	conn       *nats.Conn
	js         jetstream.JetStream
	sourceName string
}

type SharedCredentials struct {
	User, Password, Token string
	AllowTokenFallback    bool
}

// NewSharedPublisher connects to the shared NATS broker. Deployment tooling
// owns stream and consumer topology; this runtime principal can only publish.
func NewSharedPublisher(sharedURL string, credentials SharedCredentials, clientName string) (*SharedPublisher, error) {
	if sharedURL == "" {
		log.Println("ℹ️  VELION_NATS_URL not set — cross-plane publishing disabled (org-core)")
		return nil, nil
	}

	opts := []nats.Option{
		nats.Name(clientName + "-shared"),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(3 * time.Second),
		nats.DisconnectErrHandler(func(nc *nats.Conn, err error) {
			if err != nil {
				log.Printf("⚠️  Shared NATS disconnected (org-core): %v", err)
			}
		}),
		nats.ReconnectHandler(func(nc *nats.Conn) {
			log.Printf("🔄 Shared NATS reconnected (org-core): %s", nc.ConnectedUrl())
		}),
		nats.CustomInboxPrefix("_INBOX.ORG_SHARED"),
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
		return nil, fmt.Errorf("shared NATS connect failed (org-core): %w", err)
	}

	js, err := jetstream.New(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("shared NATS jetstream init failed (org-core): %w", err)
	}

	sp := &SharedPublisher{conn: conn, js: js, sourceName: clientName}
	log.Printf("✅ Connected to shared NATS (org-core): %s", sharedURL)
	return sp, nil
}

// Publish sends a cross-plane event and returns only after a JetStream PubAck.
func (sp *SharedPublisher) Publish(ctx context.Context, subject string, payload map[string]any, opts ...jetstream.PublishOpt) error {
	if sp == nil || sp.conn == nil || !sp.conn.IsConnected() {
		return fmt.Errorf("shared NATS publisher unavailable")
	}

	enriched := make(map[string]any, len(payload)+2)
	for k, v := range payload {
		enriched[k] = v
	}
	enriched["_source"] = sp.sourceName
	enriched["_published_at"] = time.Now().UTC().Format(time.RFC3339Nano)

	data, err := json.Marshal(enriched)
	if err != nil {
		return fmt.Errorf("marshal shared NATS event %s: %w", subject, err)
	}

	ack, err := sp.js.Publish(ctx, subject, data, opts...)
	if err != nil {
		return fmt.Errorf("publish shared NATS event %s: %w", subject, err)
	}
	if ack == nil || ack.Stream == "" || ack.Sequence == 0 {
		return fmt.Errorf("invalid shared NATS PubAck for %s", subject)
	}

	log.Printf("📡 SharedNATS (org-core) → %s", subject)
	return nil
}

// Close drains and closes the shared NATS connection.
func (sp *SharedPublisher) Close() {
	if sp != nil && sp.conn != nil {
		sp.conn.Drain() //nolint:errcheck
		log.Println("🔌 Shared NATS connection closed (org-core)")
	}
}

// ─── Typed helpers ────────────────────────────────────────────────────────────

// PublishOrgCreated publishes aqencia.controlplane.org.created.
// All planes subscribe to this to provision org-level resources.
func (sp *SharedPublisher) PublishOrgCreated(ctx context.Context, orgID, name, slug, plan string, metadata map[string]any) {
	_ = sp.Publish(ctx, "aqencia.controlplane.org.created", map[string]any{
		"org_id":   orgID,
		"org_name": name,
		"slug":     slug,
		"plan":     plan,
		"metadata": metadata,
	})
}

// PublishOrgUpdated publishes aqencia.controlplane.org.updated.
func (sp *SharedPublisher) PublishOrgUpdated(ctx context.Context, orgID string, changes map[string]any) {
	_ = sp.Publish(ctx, "aqencia.controlplane.org.updated", map[string]any{
		"org_id":  orgID,
		"changes": changes,
	})
}

// PublishOrgDeleted publishes aqencia.controlplane.org.deleted.
func (sp *SharedPublisher) PublishOrgDeleted(ctx context.Context, orgID, name string) {
	_ = sp.Publish(ctx, "aqencia.controlplane.org.deleted", map[string]any{
		"org_id":   orgID,
		"org_name": name,
	})
}

// PublishPlanChanged publishes aqencia.controlplane.org.plan_changed.
// Reasoning Plane subscribes to this to adjust feature availability.
func (sp *SharedPublisher) PublishPlanChanged(ctx context.Context, orgID, orgName, previousPlan, newPlan, changedBy, reason string, revision int64) error {
	return sp.Publish(ctx, "aqencia.controlplane.org.plan_changed", map[string]any{
		"org_id":        orgID,
		"org_name":      orgName,
		"previous_plan": previousPlan,
		"new_plan":      newPlan,
		"changed_by":    changedBy,
		"change_reason": reason,
		"revision":      revision,
	}, jetstream.WithMsgID(planChangeMessageID(orgID, revision)))
}

func planChangeMessageID(orgID string, revision int64) string {
	return fmt.Sprintf("organization-plan:%s:%d", orgID, revision)
}

// PublishMemberAdded publishes aqencia.controlplane.org.member_added.
func (sp *SharedPublisher) PublishMemberAdded(ctx context.Context, orgID, orgName, userID, userEmail, role string) {
	_ = sp.Publish(ctx, "aqencia.controlplane.org.member_added", map[string]any{
		"org_id":     orgID,
		"org_name":   orgName,
		"user_id":    userID,
		"user_email": userEmail,
		"role":       role,
	})
}

// PublishMemberRemoved publishes aqencia.controlplane.org.member_removed.
func (sp *SharedPublisher) PublishMemberRemoved(ctx context.Context, orgID, userID string) {
	_ = sp.Publish(ctx, "aqencia.controlplane.org.member_removed", map[string]any{
		"org_id":  orgID,
		"user_id": userID,
	})
}

// PublishPlain sends a plain NATS core message (not JetStream) to a subject.
// Used for notification-core subjects (notifications.*) which are subscribed
// to by notification-core via plain conn.Subscribe, not JetStream.
func (sp *SharedPublisher) PublishPlain(subject string, payload map[string]any) {
	if sp == nil || sp.conn == nil || !sp.conn.IsConnected() {
		return
	}
	data, err := json.Marshal(payload)
	if err != nil {
		log.Printf("❌ SharedPublisher (org-core): plain marshal failed for %s: %v", subject, err)
		return
	}
	if err := sp.conn.Publish(subject, data); err != nil {
		log.Printf("❌ SharedPublisher (org-core): plain publish %s failed: %v", subject, err)
		return
	}
	log.Printf("📡 SharedNATS (org-core) plain → %s", subject)
}
