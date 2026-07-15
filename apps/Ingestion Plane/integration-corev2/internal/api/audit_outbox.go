package api

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/controlplane"
	"github.com/triodelab/integration-corev2/internal/store"
)

const maxIntegrationAuditAttempts = 20
const maxTerminalAuditRequeue = 100

type integrationAuditStore interface {
	ClaimAuditEvent(context.Context) (store.AuditEvent, bool, error)
	CompleteAuditEvent(context.Context, string, int) error
	FailAuditEvent(context.Context, string, int, time.Time, string, bool) error
	AuditOutboxStats(context.Context) (store.AuditOutboxStats, error)
	RequeueTerminalAuditEvents(context.Context, []string) (int, error)
}

type integrationAuditRecorder interface {
	RecordAudit(context.Context, controlplane.AuditEvent) error
}

type AuditDispatcher interface {
	DispatchOne(context.Context) (bool, error)
}

type AuditOutboxHealth struct {
	store.AuditOutboxStats
	Degraded bool
}

type AuditOutboxMonitor interface {
	Status(context.Context) (AuditOutboxHealth, error)
	RequeueTerminal(context.Context, []string) (int, error)
}

// AuditOutbox durably drains integration_audit_events to Audit Core. The local
// row is also the immutable retry/idempotency record.
type AuditOutbox struct {
	store    integrationAuditStore
	recorder integrationAuditRecorder
	logger   *zerolog.Logger
	cancel   context.CancelFunc
	wg       sync.WaitGroup
}

func NewAuditOutbox(store integrationAuditStore, recorder integrationAuditRecorder, logger *zerolog.Logger) *AuditOutbox {
	return &AuditOutbox{store: store, recorder: recorder, logger: logger}
}

func (o *AuditOutbox) Start() {
	if o == nil || o.cancel != nil {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	o.cancel = cancel
	o.wg.Add(1)
	go o.run(ctx)
}

func (o *AuditOutbox) Close() {
	if o == nil || o.cancel == nil {
		return
	}
	o.cancel()
	o.wg.Wait()
}

func (o *AuditOutbox) DispatchOne(ctx context.Context) (bool, error) {
	if o == nil || o.store == nil || o.recorder == nil {
		return false, fmt.Errorf("integration audit outbox is not configured")
	}
	event, found, err := o.store.ClaimAuditEvent(ctx)
	if err != nil || !found {
		return found, err
	}
	payload := controlplane.AuditEvent{
		EventID:    event.ID,
		OccurredAt: event.CreatedAt,
		OrgID:      event.OrganizationID,
		UserID:     event.UserID,
		Event:      event.EventType,
		Subject:    auditSubject(event),
		ResourceID: event.ConnectionID,
		Outcome:    "ok",
		Details:    auditDetails(event),
		RequestID:  event.RequestID,
	}
	publishErr := o.recorder.RecordAudit(ctx, payload)
	if publishErr == nil {
		if err := o.store.CompleteAuditEvent(ctx, event.ID, event.Attempts); err != nil {
			return true, fmt.Errorf("complete audit event %s: %w", event.ID, err)
		}
		return true, nil
	}

	terminal := event.Attempts >= maxIntegrationAuditAttempts
	nextAttempt := time.Now().UTC().Add(integrationAuditRetryDelay(event.Attempts))
	if err := o.store.FailAuditEvent(ctx, event.ID, event.Attempts, nextAttempt, publishErr.Error(), terminal); err != nil {
		return true, fmt.Errorf("publish audit event %s: %v; record retry: %w", event.ID, publishErr, err)
	}
	return true, fmt.Errorf("publish audit event %s: %w", event.ID, publishErr)
}

func (o *AuditOutbox) Status(ctx context.Context) (AuditOutboxHealth, error) {
	if o == nil || o.store == nil {
		return AuditOutboxHealth{}, fmt.Errorf("integration audit outbox is not configured")
	}
	stats, err := o.store.AuditOutboxStats(ctx)
	if err != nil {
		return AuditOutboxHealth{}, err
	}
	return AuditOutboxHealth{AuditOutboxStats: stats, Degraded: stats.Terminal > 0}, nil
}

func (o *AuditOutbox) RequeueTerminal(ctx context.Context, eventIDs []string) (int, error) {
	if o == nil || o.store == nil {
		return 0, fmt.Errorf("integration audit outbox is not configured")
	}
	bounded, err := validateTerminalAuditEventIDs(eventIDs)
	if err != nil {
		return 0, err
	}
	return o.store.RequeueTerminalAuditEvents(ctx, bounded)
}

func validateTerminalAuditEventIDs(eventIDs []string) ([]string, error) {
	if len(eventIDs) < 1 || len(eventIDs) > maxTerminalAuditRequeue {
		return nil, fmt.Errorf("event ids must contain between 1 and %d entries", maxTerminalAuditRequeue)
	}
	bounded := make([]string, len(eventIDs))
	for index, eventID := range eventIDs {
		bounded[index] = strings.TrimSpace(eventID)
		if bounded[index] == "" || len(bounded[index]) > 256 {
			return nil, fmt.Errorf("event id at index %d is invalid", index)
		}
	}
	return bounded, nil
}

func integrationAuditRetryDelay(attempt int) time.Duration {
	if attempt < 1 {
		attempt = 1
	}
	seconds := attempt * attempt
	if seconds > 300 {
		seconds = 300
	}
	return time.Duration(seconds) * time.Second
}

func (o *AuditOutbox) run(ctx context.Context) {
	defer o.wg.Done()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		for index := 0; index < 50; index++ {
			found, err := o.DispatchOne(ctx)
			if err != nil {
				if !errors.Is(err, context.Canceled) && o.logger != nil {
					o.logger.Warn().Err(err).Msg("integration audit outbox dispatch deferred")
				}
				break
			}
			if !found {
				break
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
