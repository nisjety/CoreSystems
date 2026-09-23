package users

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"
	"sync"
	"time"
)

const maxAuditDispatchAttempts = 20

// AuditOutboxRow is the immutable delivery intent persisted before an audit
// event is offered to JetStream.
type AuditOutboxRow struct {
	EventID  string
	Subject  string
	Payload  []byte
	Attempts int
}

type auditOutboxStore interface {
	EnqueueAudit(context.Context, AuditOutboxRow) error
	ClaimAudit(context.Context) (AuditOutboxRow, bool, error)
	CompleteAudit(context.Context, string, int) error
	FailAudit(context.Context, string, int, time.Time, string, bool) error
}

// AuditPublisher waits for a JetStream PubAck and pins Nats-Msg-Id to eventID.
type AuditPublisher interface {
	PublishJetStreamWithMsgID(ctx context.Context, subject, eventID string, payload []byte) error
}

type auditDispatchDeferredError struct{ cause error }

func (e *auditDispatchDeferredError) Error() string {
	return "audit dispatch deferred: " + e.cause.Error()
}
func (e *auditDispatchDeferredError) Unwrap() error { return e.cause }

type auditOutbox struct {
	store     auditOutboxStore
	publisher AuditPublisher
	cancel    context.CancelFunc
	wg        sync.WaitGroup
}

func newAuditOutbox(store auditOutboxStore, publisher AuditPublisher) *auditOutbox {
	return &auditOutbox{store: store, publisher: publisher}
}

func (o *auditOutbox) Start(publisher AuditPublisher) {
	if o == nil || publisher == nil || o.cancel != nil {
		return
	}
	o.publisher = publisher
	workerContext, cancel := context.WithCancel(context.Background())
	o.cancel = cancel
	o.wg.Add(1)
	go o.run(workerContext)
}

func (o *auditOutbox) Close() {
	if o == nil || o.cancel == nil {
		return
	}
	o.cancel()
	o.wg.Wait()
}

func (o *auditOutbox) EnqueueAndDispatch(ctx context.Context, row AuditOutboxRow) error {
	if o == nil || o.store == nil {
		return fmt.Errorf("audit outbox store is unavailable")
	}
	if err := validateAuditOutboxRow(row); err != nil {
		return err
	}
	if err := o.store.EnqueueAudit(ctx, row); err != nil {
		return fmt.Errorf("persist audit event %s: %w", row.EventID, err)
	}
	if o.publisher == nil {
		return &auditDispatchDeferredError{cause: errors.New("publisher is unavailable")}
	}
	if _, err := o.DispatchOne(ctx); err != nil {
		return &auditDispatchDeferredError{cause: err}
	}
	return nil
}

func validateAuditOutboxRow(row AuditOutboxRow) error {
	if strings.TrimSpace(row.EventID) == "" || len(row.EventID) > 128 {
		return fmt.Errorf("audit event ID must contain 1-128 characters")
	}
	if !strings.HasPrefix(row.Subject, "verevon.audit.v2.control.user-core.") {
		return fmt.Errorf("audit subject %q is outside user-core authority", row.Subject)
	}
	if !json.Valid(row.Payload) {
		return fmt.Errorf("audit payload must be valid JSON")
	}
	return nil
}

func (o *auditOutbox) DispatchOne(ctx context.Context) (bool, error) {
	row, found, err := o.store.ClaimAudit(ctx)
	if err != nil || !found {
		return found, err
	}
	if o.publisher == nil {
		err = errors.New("audit publisher is unavailable")
	} else {
		err = o.publisher.PublishJetStreamWithMsgID(ctx, row.Subject, row.EventID, row.Payload)
	}
	if err == nil {
		if completeErr := o.store.CompleteAudit(ctx, row.EventID, row.Attempts); completeErr != nil {
			return true, fmt.Errorf("complete audit event %s: %w", row.EventID, completeErr)
		}
		return true, nil
	}

	terminal := row.Attempts >= maxAuditDispatchAttempts
	nextAttempt := time.Now().UTC().Add(auditRetryDelay(row.Attempts))
	if failErr := o.store.FailAudit(ctx, row.EventID, row.Attempts, nextAttempt, err.Error(), terminal); failErr != nil {
		return true, fmt.Errorf("publish audit event %s: %v; record retry: %w", row.EventID, err, failErr)
	}
	return true, fmt.Errorf("publish audit event %s: %w", row.EventID, err)
}

func auditRetryDelay(attempt int) time.Duration {
	if attempt < 1 {
		attempt = 1
	}
	seconds := min(attempt*attempt, 300)
	return time.Duration(seconds) * time.Second
}

func (o *auditOutbox) run(ctx context.Context) {
	defer o.wg.Done()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		for range 50 {
			found, err := o.DispatchOne(ctx)
			if err != nil {
				if !errors.Is(err, context.Canceled) {
					log.Printf("user-core audit outbox: %v", err)
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
