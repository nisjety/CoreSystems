package delivery

import (
	"context"
	"log/slog"
	"math"
	"time"
)

// Sender transmits a single delivery record to its destination. A nil error
// means the delivery succeeded and the record is finalised. A non-nil error
// triggers retry/backoff. Implementations should be idempotent where possible
// because the outbox has at-least-once semantics.
type Sender interface {
	Send(ctx context.Context, rec Record) error
}

// SenderFunc adapts a function to the Sender interface.
type SenderFunc func(ctx context.Context, rec Record) error

// Send calls the wrapped function.
func (f SenderFunc) Send(ctx context.Context, rec Record) error { return f(ctx, rec) }

// Config tunes the worker's draining and backoff behaviour.
type Config struct {
	// PollInterval is how often the worker scans the store for due records.
	PollInterval time.Duration
	// BatchSize is the maximum number of records claimed per poll.
	BatchSize int
	// BaseBackoff is the first retry delay; subsequent delays grow
	// exponentially (BaseBackoff * 2^attempt) capped at MaxBackoff.
	BaseBackoff time.Duration
	// MaxBackoff caps the exponential backoff.
	MaxBackoff time.Duration
	// SendTimeout bounds a single Sender.Send call.
	SendTimeout time.Duration
}

// DefaultConfig returns sensible production defaults.
func DefaultConfig() Config {
	return Config{
		PollInterval: 500 * time.Millisecond,
		BatchSize:    32,
		BaseBackoff:  time.Second,
		MaxBackoff:   5 * time.Minute,
		SendTimeout:  10 * time.Second,
	}
}

// Worker drains the outbox Store, invoking the Sender for each due record and
// applying retry/backoff or dead-lettering based on the outcome.
type Worker struct {
	store  Store
	sender Sender
	cfg    Config
}

// NewWorker constructs a Worker. Zero-valued config fields are filled from
// DefaultConfig so callers can override only what they need.
func NewWorker(store Store, sender Sender, cfg Config) *Worker {
	def := DefaultConfig()
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = def.PollInterval
	}
	if cfg.BatchSize <= 0 {
		cfg.BatchSize = def.BatchSize
	}
	if cfg.BaseBackoff <= 0 {
		cfg.BaseBackoff = def.BaseBackoff
	}
	if cfg.MaxBackoff <= 0 {
		cfg.MaxBackoff = def.MaxBackoff
	}
	if cfg.SendTimeout <= 0 {
		cfg.SendTimeout = def.SendTimeout
	}
	return &Worker{store: store, sender: sender, cfg: cfg}
}

// Run drains the outbox until ctx is cancelled. It is intended to run in its
// own goroutine. Run blocks until ctx.Done().
func (w *Worker) Run(ctx context.Context) {
	ticker := time.NewTicker(w.cfg.PollInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			slog.Info("delivery worker stopping")
			return
		case <-ticker.C:
			w.drainOnce(ctx)
		}
	}
}

// drainOnce claims and processes one batch of due records. Exported-ish via
// RunOnce for tests; kept unexported here, RunOnce is the test entry point.
func (w *Worker) drainOnce(ctx context.Context) {
	records, err := w.store.ClaimDue(ctx, time.Now().UTC(), w.cfg.BatchSize)
	if err != nil {
		slog.Error("delivery claim failed", "error", err)
		return
	}
	for _, rec := range records {
		w.process(ctx, rec)
	}
}

// RunOnce performs a single drain pass and returns the number of records it
// attempted. Intended for deterministic testing.
func (w *Worker) RunOnce(ctx context.Context) int {
	records, err := w.store.ClaimDue(ctx, time.Now().UTC(), w.cfg.BatchSize)
	if err != nil {
		slog.Error("delivery claim failed", "error", err)
		return 0
	}
	for _, rec := range records {
		w.process(ctx, rec)
	}
	return len(records)
}

func (w *Worker) process(ctx context.Context, rec Record) {
	sendCtx, cancel := context.WithTimeout(ctx, w.cfg.SendTimeout)
	defer cancel()

	err := w.sender.Send(sendCtx, rec)
	if err == nil {
		if mErr := w.store.MarkDelivered(ctx, rec.ID); mErr != nil {
			slog.Error("mark delivered failed", "id", rec.ID, "error", mErr)
		}
		slog.Debug("delivery succeeded", "id", rec.ID, "channel", rec.Channel, "attempts", rec.Attempts+1)
		return
	}

	// rec.Attempts is the count of prior attempts; this attempt makes it
	// Attempts+1. When that reaches MaxAttempts the record is dead-lettered.
	nextAttempt := rec.Attempts + 1
	if rec.MaxAttempts > 0 && nextAttempt >= rec.MaxAttempts {
		if mErr := w.store.MarkDead(ctx, rec.ID, err.Error()); mErr != nil {
			slog.Error("mark dead failed", "id", rec.ID, "error", mErr)
		}
		slog.Warn("delivery dead-lettered",
			"id", rec.ID, "channel", rec.Channel, "attempts", nextAttempt, "error", err)
		return
	}

	delay := w.backoff(nextAttempt)
	when := time.Now().UTC().Add(delay)
	if mErr := w.store.MarkRetry(ctx, rec.ID, err.Error(), when); mErr != nil {
		slog.Error("mark retry failed", "id", rec.ID, "error", mErr)
	}
	slog.Warn("delivery failed, will retry",
		"id", rec.ID, "channel", rec.Channel, "attempt", nextAttempt,
		"retry_in", delay.String(), "error", err)
}

// backoff computes the delay before the given attempt number using bounded
// exponential growth: BaseBackoff * 2^(attempt-1), capped at MaxBackoff.
func (w *Worker) backoff(attempt int) time.Duration {
	if attempt <= 1 {
		return w.cfg.BaseBackoff
	}
	// Guard against overflow on large attempt counts.
	exp := float64(attempt - 1)
	mult := math.Pow(2, exp)
	d := time.Duration(float64(w.cfg.BaseBackoff) * mult)
	if d <= 0 || d > w.cfg.MaxBackoff {
		return w.cfg.MaxBackoff
	}
	return d
}
