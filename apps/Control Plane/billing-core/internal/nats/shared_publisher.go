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
//	aqencia.controlplane.billing.account_updated
//	aqencia.controlplane.billing.quota_exceeded
//	aqencia.controlplane.billing.invoice_created
//	aqencia.controlplane.billing.plan_changed
type SharedPublisher struct {
	conn       *nats.Conn
	js         jetstream.JetStream
	sourceName string
}

// NewSharedPublisher connects to shared NATS and ensures the
// AQENCIA_CONTROLPLANE stream exists. Returns nil without error when
// sharedURL is empty (shared publishing safely disabled).
func NewSharedPublisher(sharedURL, token, clientName string) (*SharedPublisher, error) {
	if sharedURL == "" {
		log.Println("ℹ️  VELION_NATS_URL not set — cross-plane publishing disabled (billing-core)")
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
	}

	if token != "" {
		opts = append(opts, nats.Token(token))
		log.Println("🔐 Shared NATS (billing-core): using token authentication")
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

	if err := sp.ensureStream(context.Background()); err != nil {
		log.Printf("⚠️  Shared NATS (billing-core): stream setup: %v", err)
	}

	log.Printf("✅ Connected to shared NATS (billing-core): %s", sharedURL)
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
			return nil
		}
		return err
	}
	log.Println("✅ Shared NATS (billing-core): AQENCIA_CONTROLPLANE stream ready")
	return nil
}

// Publish sends a cross-plane event. Fire-and-forget: never fails caller.
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
		log.Printf("❌ SharedPublisher (billing-core): marshal failed for %s: %v", subject, err)
		return
	}

	if _, err := sp.js.Publish(ctx, subject, data); err != nil {
		log.Printf("❌ SharedPublisher (billing-core): publish %s failed: %v", subject, err)
		return
	}

	log.Printf("📡 SharedNATS (billing-core) → %s", subject)
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
	sp.Publish(ctx, "aqencia.controlplane.billing.account_updated", map[string]any{
		"org_id": orgID,
		"plan":   plan,
		"seats":  seats,
	})
}

// PublishQuotaExceeded fires when an org exceeds a usage quota.
func (sp *SharedPublisher) PublishQuotaExceeded(ctx context.Context, orgID, metric string, limit, current int64) {
	sp.Publish(ctx, "aqencia.controlplane.billing.quota_exceeded", map[string]any{
		"org_id":  orgID,
		"metric":  metric,
		"limit":   limit,
		"current": current,
	})
}

// PublishInvoiceCreated fires when a new invoice is generated.
func (sp *SharedPublisher) PublishInvoiceCreated(ctx context.Context, orgID, invoiceID string, amountCents int64, currency string) {
	sp.Publish(ctx, "aqencia.controlplane.billing.invoice_created", map[string]any{
		"org_id":       orgID,
		"invoice_id":   invoiceID,
		"amount_cents": amountCents,
		"currency":     currency,
	})
}

// PublishPlanChanged fires when a subscription plan changes.
func (sp *SharedPublisher) PublishPlanChanged(ctx context.Context, orgID, previousPlan, newPlan string) {
	sp.Publish(ctx, "aqencia.controlplane.billing.plan_changed", map[string]any{
		"org_id":        orgID,
		"previous_plan": previousPlan,
		"new_plan":      newPlan,
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
		log.Printf("❌ SharedPublisher (billing-core): plain marshal failed for %s: %v", subject, err)
		return
	}
	if err := sp.conn.Publish(subject, data); err != nil {
		log.Printf("❌ SharedPublisher (billing-core): plain publish %s failed: %v", subject, err)
		return
	}
	log.Printf("📡 SharedNATS (billing-core) plain → %s", subject)
}
