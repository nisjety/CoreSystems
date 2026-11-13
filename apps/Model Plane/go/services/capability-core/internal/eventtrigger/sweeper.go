package eventtrigger

import (
	"context"
	"errors"
	"log/slog"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// DefaultSweepInterval is how often a sweep looks for pending fires.
const DefaultSweepInterval = 30 * time.Second

// defaultBatchSize is the most pending fires one sweep decides. The batch holds its transaction open across one
// authority call per fire, so it stays small.
const defaultBatchSize = 10

// Sweeper decides pending fires. It is the cron rule for a trigger: each claimed event is authorised afresh by
// Control, and a stored record is never authority by itself.
type Sweeper struct {
	ledger    ledger
	authority cron.FireAuthorizer
	limits    Limits
	interval  time.Duration
	batchSize int
	now       func() time.Time
	wake      wakeGate
}

// NewSweeper builds the sweeper over the fire table. Both the database and the authorizer are required: a sweeper
// without an authorizer could decide nothing, and one without a database has nothing to decide.
func NewSweeper(pool *pgxpool.Pool, authority cron.FireAuthorizer, limits Limits) (*Sweeper, error) {
	if pool == nil || authority == nil {
		return nil, errors.New("event trigger sweeper needs a database and a fresh Control fire authorizer")
	}
	return newSweeper(pgStore{pool: pool}, authority, limits), nil
}

func newSweeper(l ledger, authority cron.FireAuthorizer, limits Limits) *Sweeper {
	return &Sweeper{
		ledger:    l,
		authority: authority,
		limits:    limits.withDefaults(),
		interval:  DefaultSweepInterval,
		batchSize: defaultBatchSize,
		now:       time.Now,
	}
}

// EnableWakeGate turns the wake gate on. Before Control is asked, a fire whose trigger carries a wake filter reads its
// webhook payload from source and must match the filter. Without this call (the switch is off) stored filters are ignored,
// and every fire is decided as before. Call it before Start. A nil source fails closed: a filtered fire then stays pending
// and is refused once its attempts are spent.
func (s *Sweeper) EnableWakeGate(source WebhookEventSource) {
	if store, ok := s.ledger.(pgStore); ok {
		store.wakeFilter = true
		s.ledger = store
	}
	s.wake = payloadWakeGate{source: source}
}

// Start sweeps until ctx is cancelled. A failed sweep is logged, not fatal.
func (s *Sweeper) Start(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	s.sweepAndLog(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.sweepAndLog(ctx)
		}
	}
}

func (s *Sweeper) sweepAndLog(ctx context.Context) {
	authorized, err := s.RunOnce(ctx)
	if err != nil {
		slog.Warn("event trigger sweep failed", "authorized", authorized, "error", err)
	}
	if authorized > 0 {
		slog.Info("event trigger sweep authorised fires", "count", authorized)
	}
}

// RunOnce decides one batch of pending fires and returns how many were authorised. A fire whose event has aged past the
// window is refused fire_expired without asking Control. An authority error belongs to one fire and is retried by a later
// sweep, so it does not fail the sweep. A failed database step does fail it, and the whole batch rolls back, so the next
// sweep decides the same fires again.
func (s *Sweeper) RunOnce(ctx context.Context) (int, error) {
	b, err := s.ledger.begin(ctx)
	if err != nil {
		return 0, err
	}
	defer b.rollback(ctx)

	fires, err := b.due(ctx, s.batchSize)
	if err != nil {
		return 0, err
	}
	now := s.now().UTC()
	authorized := 0
	for _, fire := range fires {
		counts, err := b.counts(ctx, fire, now.Add(-fireWindow))
		if err != nil {
			return 0, err
		}
		settlement, authErr := decide(ctx, s.authority, s.wake, fire, counts, s.limits, now)
		if authErr != nil {
			slog.Warn("event trigger fire could not be decided", "trigger_id", fire.TriggerID,
				"status", settlement.Status, "attempts", settlement.Attempts, "error", authErr)
		}
		if err := b.settle(ctx, fire, settlement, now); err != nil {
			return 0, err
		}
		if settlement.Status == StatusAuthorized {
			authorized++
		}
	}
	if err := b.commit(ctx); err != nil {
		return 0, err
	}
	return authorized, nil
}
