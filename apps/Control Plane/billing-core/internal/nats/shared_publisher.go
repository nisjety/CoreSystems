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
// verevon-nats broker on the aqencia.controlplane.* subject namespace.
//
// Subject convention: aqencia.controlplane.<entity>.<verb>
//
//	aqencia.controlplane.billing.account_updated
//	aqencia.controlplane.billing.quota_exceeded
//	aqencia.controlplane.billing.invoice_created
//	aqencia.controlplane.billing.plan_changed
type SharedPublisher struct {
	conn       *nats.Conn
	js         jetstream.JetStream
	sourceName string
}

type SharedCredentials struct {
	User, Password, Token string
	AllowTokenFallback    bool
}

// NewSharedPublisher connects to shared NATS. Deployment tooling owns stream
// and consumer topology; this runtime principal can only publish.
func NewSharedPublisher(sharedURL string, credentials SharedCredentials, clientName string) (*SharedPublisher, error) {
	if sharedURL == "" {
		log.Println("ℹ️  VEREVON_NATS_URL not set — cross-plane publishing disabled (billing-core)")
		return nil, nil
	}

	opts := []nats.Option{
		nats.Name(clientName + "-shared"),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(3 * time.Second),
		nats.DisconnectErrHandler(func(nc *nats.Conn, err error) {
			if err != nil {
				log.Printf("⚠️  Shared NATS disconnected (billing-core): %v", err)
			}
		}),
		nats.ReconnectHandler(func(nc *nats.Conn) {
			log.Printf("🔄 Shared NATS reconnected (billing-core): %s", nc.ConnectedUrl())
		}),
		nats.CustomInboxPrefix("_INBOX.BILLING_SHARED"),
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
		return nil, fmt.Errorf("shared NATS connect failed (billing-core): %w", err)
	}

	js, err := jetstream.New(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("shared NATS jetstream init failed (billing-core): %w", err)
	}

	sp := &SharedPublisher{conn: conn, js: js, sourceName: clientName}
	log.Printf("✅ Connected to shared NATS (billing-core): %s", sharedURL)
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

	log.Printf("📡 SharedNATS (billing-core) → %s", subject)
	return nil
}

// Close drains and closes the shared NATS connection.
func (sp *SharedPublisher) Close() {
	if sp != nil && sp.conn != nil {
		sp.conn.Drain() //nolint:errcheck
		log.Println("🔌 Shared NATS connection closed (billing-core)")
	}
}

// ─── Typed helpers ────────────────────────────────────────────────────────────

// PublishAccountUpdated fires when an org's billing account is created or updated.
// Data Plane and Reasoning Plane subscribe to adjust feature quotas.
func (sp *SharedPublisher) PublishAccountUpdated(ctx context.Context, orgID, plan string, seats int) {
	_ = sp.Publish(ctx, "aqencia.controlplane.billing.account_updated", map[string]any{
		"org_id": orgID,
		"plan":   plan,
		"seats":  seats,
	})
}

// PublishQuotaExceeded fires when an org exceeds a usage quota.
func (sp *SharedPublisher) PublishQuotaExceeded(ctx context.Context, orgID, metric string, limit, current int64) {
	_ = sp.Publish(ctx, "aqencia.controlplane.billing.quota_exceeded", map[string]any{
		"org_id":  orgID,
		"metric":  metric,
		"limit":   limit,
		"current": current,
	})
}

// PublishInvoiceCreated fires when a new invoice is generated.
func (sp *SharedPublisher) PublishInvoiceCreated(ctx context.Context, orgID, invoiceID string, amountCents int64, currency string) {
	_ = sp.Publish(ctx, "aqencia.controlplane.billing.invoice_created", map[string]any{
		"org_id":       orgID,
		"invoice_id":   invoiceID,
		"amount_cents": amountCents,
		"currency":     currency,
	})
}

// PublishPlanChanged fires when a subscription plan changes.
func (sp *SharedPublisher) PublishPlanChanged(ctx context.Context, orgID, previousPlan, newPlan string, revisions ...int64) error {
	payload := map[string]any{
		"org_id":        orgID,
		"previous_plan": previousPlan,
		"new_plan":      newPlan,
	}
	if len(revisions) == 0 || revisions[0] < 1 {
		return sp.Publish(ctx, "aqencia.controlplane.billing.plan_changed", payload)
	}
	revision := revisions[0]
	payload["revision"] = revision
	return sp.Publish(
		ctx,
		"aqencia.controlplane.billing.plan_changed",
		payload,
		jetstream.WithMsgID(billingPlanChangeMessageID(orgID, revision)),
	)
}

func billingPlanChangeMessageID(orgID string, revision int64) string {
	return fmt.Sprintf("billing-plan:%s:%d", orgID, revision)
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
		log.Printf("❌ SharedPublisher (billing-core): plain marshal failed for %s: %v", subject, err)
		return
	}
	if err := sp.conn.Publish(subject, data); err != nil {
		log.Printf("❌ SharedPublisher (billing-core): plain publish %s failed: %v", subject, err)
		return
	}
	log.Printf("📡 SharedNATS (billing-core) plain → %s", subject)
}
