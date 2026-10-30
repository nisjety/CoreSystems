// Package audit persists leads-core audit intent before dispatching it to the
// producer-scoped Audit Core JetStream contract.
package audit

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/natsauth"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
)

const (
	SubjectLeadExport       = "verevon.audit.v2.application.leads-core.lead_export"
	SubjectProviderLeadSync = "verevon.audit.v2.application.leads-core.provider_lead_sync"
	maxDispatchAttempts     = 20
)

type NatsPublisher struct {
	conn   *nats.Conn
	js     nats.JetStreamContext
	pool   *pgxpool.Pool
	cancel context.CancelFunc
	wg     sync.WaitGroup
}

type outboxRow struct {
	eventID string
	subject string
	payload []byte
	attempt int
}

func Connect(url, user, password, name string, pool *pgxpool.Pool) (*NatsPublisher, error) {
	if pool == nil {
		return nil, fmt.Errorf("leads audit outbox database is required")
	}
	opts := []nats.Option{
		nats.Name(name), nats.Timeout(5 * time.Second),
		nats.CustomInboxPrefix("_INBOX.APPLICATION_LEADS"),
		nats.MaxReconnects(-1), nats.ReconnectWait(time.Second),
	}
	credential, err := natsauth.Select(user, password)
	if err != nil {
		return nil, err
	}
	opts = append(opts, nats.UserInfo(credential.User, credential.Password))
	conn, err := nats.Connect(url, opts...)
	if err != nil {
		return nil, err
	}
	js, err := conn.JetStream()
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("open leads audit JetStream: %w", err)
	}
	workerContext, cancel := context.WithCancel(context.Background())
	publisher := &NatsPublisher{conn: conn, js: js, pool: pool, cancel: cancel}
	publisher.wg.Add(1)
	go publisher.run(workerContext)
	return publisher, nil
}

func (p *NatsPublisher) Close() {
	if p == nil {
		return
	}
	if p.cancel != nil {
		p.cancel()
	}
	p.wg.Wait()
	if p.conn != nil {
		p.conn.Close()
	}
}

func (p *NatsPublisher) PublishLeadExport(ctx context.Context, event leads.LeadExportAudit) {
	eventID, err := newEventID("export")
	if err != nil {
		log.Printf("leads-core: create lead_export audit identity: %v", err)
		return
	}
	p.enqueueAndDispatch(ctx, eventID, SubjectLeadExport, buildLeadExportPayload(eventID, time.Now().UTC(), event))
}

func (p *NatsPublisher) PublishProviderLeadSync(ctx context.Context, event providerleads.SyncAudit) {
	eventID, err := newEventID("sync")
	if err != nil {
		log.Printf("leads-core: create provider_lead_sync audit identity: %v", err)
		return
	}
	p.enqueueAndDispatch(ctx, eventID, SubjectProviderLeadSync, buildProviderLeadSyncPayload(eventID, time.Now().UTC(), event))
}

func buildLeadExportPayload(eventID string, occurredAt time.Time, event leads.LeadExportAudit) map[string]any {
	userID := event.UserID
	if userID == "" {
		userID = "internal-service"
	}
	return map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.Format(time.RFC3339Nano),
		"org_id": event.OrgID, "user_id": userID, "plane": "application", "producer": "leads-core",
		"event": "lead_export", "subject": "lead_list:" + event.ListID,
		"resource_id": event.ListID, "outcome": "ok",
		"details": map[string]any{"list_name": event.ListName, "count": event.Count},
	}
}

func buildProviderLeadSyncPayload(eventID string, occurredAt time.Time, event providerleads.SyncAudit) map[string]any {
	outcome := event.Outcome
	if outcome == "" {
		outcome = "ok"
	}
	return map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.Format(time.RFC3339Nano),
		"org_id": event.OrgID, "user_id": "internal-service", "plane": "application", "producer": "leads-core",
		"event": "provider_lead_sync", "subject": "provider_leads:" + event.ProviderKey,
		"resource_id": event.ProviderKey, "outcome": outcome,
		"details": map[string]any{
			"provider_key": event.ProviderKey, "connections": event.Connections, "forms": event.Forms,
			"leads_fetched": event.LeadsFetched, "leads_upserted": event.LeadsUpserted, "skipped": event.Skipped,
		},
	}
}

func newEventID(kind string) (string, error) {
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return "", err
	}
	return "lead:" + kind + ":" + hex.EncodeToString(random[:]), nil
}

func (p *NatsPublisher) enqueueAndDispatch(ctx context.Context, eventID, subject string, payload map[string]any) {
	if p == nil || p.pool == nil || p.js == nil {
		return
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		log.Printf("leads-core: encode audit outbox event %s: %v", eventID, err)
		return
	}
	if _, err := p.pool.Exec(ctx, `
		INSERT INTO leads_audit_outbox (event_id, subject, payload)
		VALUES ($1, $2, $3::jsonb)
		ON CONFLICT (event_id) DO NOTHING
	`, eventID, subject, encoded); err != nil {
		log.Printf("leads-core: persist audit outbox event %s: %v", eventID, err)
		return
	}
	if _, err := p.dispatchOne(ctx); err != nil {
		log.Printf("leads-core: immediate audit dispatch deferred: %v", err)
	}
}

func (p *NatsPublisher) run(ctx context.Context) {
	defer p.wg.Done()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		if err := p.drain(ctx); err != nil && !errors.Is(err, context.Canceled) {
			log.Printf("leads-core: audit outbox worker: %v", err)
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

func (p *NatsPublisher) drain(ctx context.Context) error {
	for index := 0; index < 50; index++ {
		found, err := p.dispatchOne(ctx)
		if err != nil || !found {
			return err
		}
	}
	return nil
}

func (p *NatsPublisher) dispatchOne(ctx context.Context) (bool, error) {
	row := outboxRow{}
	err := p.pool.QueryRow(ctx, `
		WITH candidate AS (
		  SELECT event_id
		  FROM leads_audit_outbox
		  WHERE published_at IS NULL
		    AND terminal_at IS NULL
		    AND next_attempt_at <= now()
		    AND (processing_at IS NULL OR processing_at < now() - interval '1 minute')
		  ORDER BY next_attempt_at, created_at
		  FOR UPDATE SKIP LOCKED
		  LIMIT 1
		)
		UPDATE leads_audit_outbox AS outbox
		SET processing_at = now(), attempts = outbox.attempts + 1
		FROM candidate
		WHERE outbox.event_id = candidate.event_id
		RETURNING outbox.event_id, outbox.subject, outbox.payload, outbox.attempts
	`).Scan(&row.eventID, &row.subject, &row.payload, &row.attempt)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, err
	}

	message := nats.NewMsg(row.subject)
	message.Data = row.payload
	message.Header.Set(nats.MsgIdHdr, row.eventID)
	_, publishErr := p.js.PublishMsg(message, nats.Context(ctx))
	if publishErr == nil {
		_, err = p.pool.Exec(ctx, `
			UPDATE leads_audit_outbox
			SET published_at = now(), processing_at = NULL, last_error = NULL
			WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL
		`, row.eventID, row.attempt)
		return true, err
	}

	_, updateErr := p.pool.Exec(ctx, `
		UPDATE leads_audit_outbox
		SET processing_at = NULL,
		    last_error = left($3, 2000),
		    terminal_at = CASE WHEN attempts >= $4 THEN now() ELSE terminal_at END,
		    next_attempt_at = now() + make_interval(secs => LEAST(300, attempts * attempts))
		WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL
	`, row.eventID, row.attempt, publishErr.Error(), maxDispatchAttempts)
	if updateErr != nil {
		return true, fmt.Errorf("publish %s: %v; record retry: %w", row.eventID, publishErr, updateErr)
	}
	return true, fmt.Errorf("publish %s: %w", row.eventID, publishErr)
}
