// Package janitor implements the Phase 7 working-set retention sweep: it evicts
// old crawl artifacts so the ephemeral tier (FS/S3/CAS bodies) does not accrue
// cost forever, while the durable Data Plane corpus stays permanent.
//
// SAFE BY DEFAULT — the sweep is fully INERT unless two keys are turned:
//   - QUARRY_RETENTION_DAYS must be > 0 (default 0 = disabled entirely), AND
//   - QUARRY_RETENTION_DRY_RUN must be "false" (default true = log only).
//
// So with no config it does nothing; with only RETENTION_DAYS set it merely
// LOGS what it would evict (operators review first); deletion happens only when
// both keys are deliberately set. This two-key opt-in is intentional: artifact
// deletion is irreversible.
//
// PROVENANCE CAVEAT (must close before enabling real deletion): this MVP keys
// purely on artifact age. A correct janitor MUST additionally skip artifacts
// whose run produced a durable Data Plane document (source_trace reference) and
// MUST NOT treat a transient fetch failure as "page vanished". Until that guard
// lands, keep DryRun=true and review the logged candidates.
package janitor

import (
	"context"
	"os"
	"strconv"
	"time"

	"github.com/rs/zerolog"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// Options controls the retention sweep. Zero value = disabled.
type Options struct {
	RetentionDays int
	DryRun        bool
	Interval      time.Duration
	BatchLimit    int
}

// OptionsFromEnv reads the (default-safe) retention configuration. Defaults:
// disabled (RetentionDays=0) and DryRun=true.
func OptionsFromEnv() Options {
	days := 0
	if v, err := strconv.Atoi(os.Getenv("QUARRY_RETENTION_DAYS")); err == nil && v > 0 {
		days = v
	}
	// Dry-run defaults TRUE; only an explicit "false" arms deletion.
	dryRun := os.Getenv("QUARRY_RETENTION_DRY_RUN") != "false"
	return Options{
		RetentionDays: days,
		DryRun:        dryRun,
		Interval:      6 * time.Hour,
		BatchLimit:    500,
	}
}

// Run is the retention loop. It returns immediately (no-op) when disabled, so it
// is safe to always wire into main. Honors ctx cancellation for clean shutdown.
func Run(ctx context.Context, db store.DB, opts Options, logger zerolog.Logger) {
	if opts.RetentionDays <= 0 {
		logger.Info().Msg("retention janitor disabled (QUARRY_RETENTION_DAYS unset or 0)")
		return
	}
	if opts.Interval <= 0 {
		opts.Interval = 6 * time.Hour
	}
	if opts.BatchLimit <= 0 {
		opts.BatchLimit = 500
	}
	logger.Info().
		Int("retention_days", opts.RetentionDays).
		Bool("dry_run", opts.DryRun).
		Dur("interval", opts.Interval).
		Msg("retention janitor enabled")

	ticker := time.NewTicker(opts.Interval)
	defer ticker.Stop()
	for {
		sweep(db, opts, logger)
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

func sweep(db store.DB, opts Options, logger zerolog.Logger) {
	cutoff := time.Now().Add(-time.Duration(opts.RetentionDays) * 24 * time.Hour).Unix()
	arts, _ := db.Artifacts().List(opts.BatchLimit, "")
	candidates, evicted := 0, 0
	for _, a := range arts {
		if a.CreatedAt >= cutoff {
			continue
		}
		candidates++
		if opts.DryRun {
			logger.Info().
				Str("artifact_id", string(a.ID)).
				Str("run_id", string(a.RunID)).
				Int64("created_at", a.CreatedAt).
				Msg("retention: eviction candidate (DRY-RUN — not deleted)")
			continue
		}
		if err := db.Artifacts().Delete(a.ID); err != nil {
			logger.Warn().Err(err).Str("artifact_id", string(a.ID)).Msg("retention: delete failed")
			continue
		}
		evicted++
	}
	logger.Info().
		Int("candidates", candidates).
		Int("evicted", evicted).
		Bool("dry_run", opts.DryRun).
		Msg("retention sweep complete")
}
