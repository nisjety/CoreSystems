package sync

import (
	"context"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/store"
)

// EnabledSourcesLister is the narrow surface the scheduler needs from the
// Sources repository — keeps the test fakes tiny.
type EnabledSourcesLister interface {
	ListEnabled(ctx context.Context) ([]store.Source, error)
}

// DriveSyncer abstracts Engine.SyncDrive so tests can plug in a fake.
type DriveSyncer interface {
	SyncDrive(ctx context.Context, sourceID uuid.UUID) (SyncResult, error)
}

// SourceLocker guards a per-source sync with a Postgres advisory lock so two
// finspo-api replicas do not sync the same drive at once. Implemented by
// *store.Locks. Optional — when nil, the scheduler runs without locking
// (correct for a single replica).
type SourceLocker interface {
	WithSourceLock(ctx context.Context, sourceID uuid.UUID, fn func(context.Context) error) (bool, error)
}

// Scheduler periodically fans out SyncDrive across every enabled source.
type Scheduler struct {
	sources  EnabledSourcesLister
	runner   DriveSyncer
	locker   SourceLocker
	interval time.Duration
	logger   zerolog.Logger
}

type SchedulerConfig struct {
	Sources  EnabledSourcesLister
	Runner   DriveSyncer
	Locker   SourceLocker
	Interval time.Duration
	Logger   zerolog.Logger
}

func NewScheduler(cfg SchedulerConfig) *Scheduler {
	if cfg.Interval <= 0 {
		cfg.Interval = 5 * time.Minute
	}
	return &Scheduler{
		sources:  cfg.Sources,
		runner:   cfg.Runner,
		locker:   cfg.Locker,
		interval: cfg.Interval,
		logger:   cfg.Logger,
	}
}

// Run blocks until ctx is cancelled. It performs one immediate sweep at start,
// then ticks at the configured interval. Sync errors are logged but never
// surfaced — the scheduler's job is to keep ticking, not to crash on bad
// drives.
func (s *Scheduler) Run(ctx context.Context) {
	s.logger.Info().Dur("interval", s.interval).Msg("scheduler started")
	defer s.logger.Info().Msg("scheduler stopped")

	s.tick(ctx)
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.tick(ctx)
		}
	}
}

func (s *Scheduler) tick(ctx context.Context) {
	sources, err := s.sources.ListEnabled(ctx)
	if err != nil {
		s.logger.Error().Err(err).Msg("list enabled sources")
		return
	}
	if len(sources) == 0 {
		s.logger.Debug().Msg("no enabled sources; skipping tick")
		return
	}

	for _, src := range sources {
		if ctx.Err() != nil {
			return
		}
		s.syncOne(ctx, src)
	}
}

func (s *Scheduler) syncOne(ctx context.Context, src store.Source) {
	run := func(ctx context.Context) error {
		res, err := s.runner.SyncDrive(ctx, src.ID)
		if err != nil {
			return err
		}
		s.logger.Info().
			Str("source_id", src.ID.String()).
			Int("pages", res.Pages).
			Int("upserts", res.ItemsUpserted).
			Int("deletes", res.ItemsDeleted).
			Msg("scheduled sync ok")
		return nil
	}

	// Single-replica path: no locker configured.
	if s.locker == nil {
		if err := run(ctx); err != nil {
			s.logFailure(src, err)
		}
		return
	}

	// Multi-replica path: only the replica that wins the advisory lock syncs.
	acquired, err := s.locker.WithSourceLock(ctx, src.ID, run)
	if err != nil {
		s.logFailure(src, err)
		return
	}
	if !acquired {
		s.logger.Debug().
			Str("source_id", src.ID.String()).
			Msg("source locked by another replica; skipping")
	}
}

func (s *Scheduler) logFailure(src store.Source, err error) {
	s.logger.Error().
		Err(err).
		Str("source_id", src.ID.String()).
		Str("organization_id", src.OrganizationID).
		Msg("scheduled sync failed")
}
