package billing

import (
	"context"
	"fmt"
	"math"
	"time"

	zlog "github.com/rs/zerolog/log"
)

type RetryProcessorConfig struct {
	PollInterval time.Duration
	BatchSize    int
	MaxAttempts  int
	BaseBackoff  time.Duration
}

func (s *Service) StartRetryProcessor(ctx context.Context, cfg RetryProcessorConfig) {
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = 5 * time.Second
	}
	if cfg.BatchSize <= 0 {
		cfg.BatchSize = 50
	}
	if cfg.MaxAttempts <= 0 {
		cfg.MaxAttempts = 8
	}
	if cfg.BaseBackoff <= 0 {
		cfg.BaseBackoff = 5 * time.Second
	}

	ticker := time.NewTicker(cfg.PollInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := s.processRetryBatch(ctx, cfg); err != nil {
				zlog.Error().Err(err).Msg("billing-core retry processor batch failed")
			}
		}
	}
}

func (s *Service) processRetryBatch(ctx context.Context, cfg RetryProcessorConfig) error {
	jobs, err := s.repo.ClaimDueRetryJobs(ctx, cfg.BatchSize)
	if err != nil {
		return err
	}

	for _, job := range jobs {
		if err := s.processRetryJob(ctx, job, cfg); err != nil {
			zlog.Error().Err(err).Int64("job_id", int64(job.ID)).Str("kind", string(job.Kind)).Msg("billing-core retry job failed")
		}
	}

	return nil
}

func (s *Service) processRetryJob(ctx context.Context, job RetryJob, cfg RetryProcessorConfig) error {
	err := s.executeRetryJob(ctx, job)
	if err == nil {
		return s.repo.MarkRetryJobSucceeded(ctx, job.ID)
	}

	nextAttempt := job.AttemptCount + 1
	if nextAttempt >= cfg.MaxAttempts {
		if markErr := s.repo.MarkRetryJobDeadLetter(ctx, job.ID, nextAttempt, err.Error()); markErr != nil {
			return markErr
		}
		s.publishDeadLetter(ctx, job, nextAttempt, err)
		return nil
	}

	backoff := s.retryBackoff(cfg.BaseBackoff, nextAttempt)
	if markErr := s.repo.MarkRetryJobPending(ctx, job.ID, nextAttempt, err.Error(), time.Now().UTC().Add(backoff)); markErr != nil {
		return markErr
	}

	return nil
}

func (s *Service) executeRetryJob(ctx context.Context, job RetryJob) error {
	switch job.Kind {
	case RetryJobKindLagoUsage:
		if s.invoiceAdapter == nil {
			return fmt.Errorf("invoice adapter is not configured")
		}
		usage, err := usageFromPayload(job.Payload)
		if err != nil {
			return err
		}
		return s.invoiceAdapter.ReportUsage(ctx, usage)
	case RetryJobKindStripeCharge:
		invoice, err := invoiceFromPayload(job.Payload)
		if err != nil {
			return err
		}
		return s.paymentAdapter.ChargeInvoice(ctx, invoice)
	default:
		return fmt.Errorf("unknown retry job kind: %s", job.Kind)
	}
}

func (s *Service) publishDeadLetter(ctx context.Context, job RetryJob, attempts int, err error) {
	if s.publisher == nil {
		return
	}

	_ = s.publisher.Publish(ctx, "billing.dlq", map[string]any{
		"job_id":         job.ID,
		"kind":           job.Kind,
		"dedupe_key":     job.DedupeKey,
		"attempts":       attempts,
		"last_error":     err.Error(),
		"payload":        job.Payload,
		"dead_letter_at": time.Now().UTC().Format(time.RFC3339),
	})
}

func (s *Service) retryBackoff(base time.Duration, attempt int) time.Duration {
	if attempt <= 0 {
		attempt = 1
	}
	factor := math.Pow(2, float64(attempt-1))
	backoff := time.Duration(float64(base) * factor)
	max := 10 * time.Minute
	if backoff > max {
		return max
	}
	return backoff
}

func usageFromPayload(payload map[string]interface{}) (UsageEvent, error) {
	orgID, ok := payload["org_id"].(string)
	if !ok || orgID == "" {
		return UsageEvent{}, fmt.Errorf("retry payload missing org_id")
	}
	metric, ok := payload["metric"].(string)
	if !ok || metric == "" {
		return UsageEvent{}, fmt.Errorf("retry payload missing metric")
	}
	quantity, ok := payload["quantity"].(float64)
	if !ok {
		return UsageEvent{}, fmt.Errorf("retry payload missing quantity")
	}
	eventID, _ := payload["event_id"].(string)
	if err := ValidateUsageEventID(eventID); err != nil {
		return UsageEvent{}, fmt.Errorf("retry payload invalid event_id: %w", err)
	}
	source, _ := payload["source"].(string)
	if source == "" {
		source = "unknown"
	}
	occurredRaw, _ := payload["occurred_at"].(string)
	occurredAt, err := time.Parse(time.RFC3339, occurredRaw)
	if err != nil {
		return UsageEvent{}, fmt.Errorf("retry payload invalid occurred_at")
	}

	metadata := map[string]interface{}{}
	if raw, ok := payload["metadata"]; ok {
		if parsed, ok := raw.(map[string]interface{}); ok {
			metadata = parsed
		}
	}

	return UsageEvent{
		EventID:    eventID,
		OrgID:      orgID,
		Metric:     metric,
		Quantity:   quantity,
		Source:     source,
		OccurredAt: occurredAt,
		Metadata:   metadata,
	}, nil
}

func invoiceFromPayload(payload map[string]interface{}) (Invoice, error) {
	invoiceID, _ := payload["invoice_id"].(string)
	orgID, ok := payload["org_id"].(string)
	if !ok || orgID == "" {
		return Invoice{}, fmt.Errorf("retry payload missing org_id")
	}
	provider, _ := payload["provider"].(string)
	if provider == "" {
		provider = "stripe"
	}
	status, _ := payload["status"].(string)
	if status == "" {
		status = "open"
	}
	currency, _ := payload["currency"].(string)
	if currency == "" {
		currency = "NOK"
	}
	amount, ok := payload["amount_cents"].(float64)
	if !ok {
		return Invoice{}, fmt.Errorf("retry payload missing amount_cents")
	}
	metadata := map[string]interface{}{}
	if raw, ok := payload["metadata"]; ok {
		if parsed, ok := raw.(map[string]interface{}); ok {
			metadata = parsed
		}
	}

	now := time.Now().UTC()
	issuedAt := now
	if raw, ok := payload["issued_at"].(string); ok && raw != "" {
		if parsed, err := time.Parse(time.RFC3339, raw); err == nil {
			issuedAt = parsed
		}
	}
	dueAt := issuedAt.Add(7 * 24 * time.Hour)
	if raw, ok := payload["due_at"].(string); ok && raw != "" {
		if parsed, err := time.Parse(time.RFC3339, raw); err == nil {
			dueAt = parsed
		}
	}

	return Invoice{
		InvoiceID:   invoiceID,
		OrgID:       orgID,
		Provider:    provider,
		AmountCents: int64(amount),
		Currency:    currency,
		Status:      status,
		IssuedAt:    issuedAt,
		DueAt:       dueAt,
		Metadata:    metadata,
	}, nil
}
