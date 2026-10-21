// The ZDR tombstone sweeper (WS4.J): ages out zdr_run_tombstones rows.
//
// This is NOT the artifact janitor in janitor.go. That one deletes working-set
// artifacts, which are irreversible and not yet guarded by a provenance check, so
// it stays inert until two keys are turned. Tombstones are metadata, nothing
// references them, and deleting them is the privacy-safe direction, so this sweeper
// runs by default with a 30-day retention. That is a deliberate departure from the
// two-key arming of the artifact janitor (recorded in docs/RUNBOOKS.md and the plan
// ledger). With the tombstone flag off on the edge there are no rows and a sweep
// deletes nothing.
//
//   - QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS: days to keep a tombstone after its run
//     ended. Unset or empty: 30. 0: delete every tombstone that ended before now
//     (the evidence run uses it). A value that is not a non-negative integer logs
//     and falls back to 30; it never disables the sweeper.
//   - QUARRY_ZDR_TOMBSTONE_DRY_RUN: true, 1, yes or on logs the count it would delete and
//     deletes nothing. Default false.
//
// Deletion is batched (oldest first, TombstoneBatch rows at a time) so a large
// backlog never holds one long statement. Logs carry counts only.
package janitor

import (
	"context"
	"strconv"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// DefaultTombstoneRetentionDays is the retention when nothing valid is configured.
const DefaultTombstoneRetentionDays = 30

// TombstoneOptions controls the tombstone sweeper.
type TombstoneOptions struct {
	RetentionDays int
	DryRun        bool
	Interval      time.Duration
	Batch         int
	// Now is the clock; nil means time.Now. Tests set it.
	Now func() time.Time
}

// TombstoneOptionsFromEnv reads the configuration through getenv (os.Getenv in
// main, a map lookup in tests). The second result is true when the retention value
// was set but invalid and the default was used instead.
func TombstoneOptionsFromEnv(getenv func(string) string) (TombstoneOptions, bool) {
	opts := TombstoneOptions{
		RetentionDays: DefaultTombstoneRetentionDays,
		DryRun:        truthy(getenv("QUARRY_ZDR_TOMBSTONE_DRY_RUN")),
		Interval:      time.Hour,
		Batch:         1000,
	}
	invalid := false
	if raw := strings.TrimSpace(getenv("QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS")); raw != "" {
		if n, err := strconv.Atoi(raw); err == nil && n >= 0 {
			opts.RetentionDays = n
		} else {
			invalid = true
		}
	}
	return opts, invalid
}

// truthy reads the dry-run flag leniently (true, 1, yes, on): a mistyped value must
// err toward NOT deleting.
func truthy(v string) bool {
	switch strings.ToLower(strings.TrimSpace(v)) {
	case "true", "1", "yes", "on":
		return true
	}
	return false
}

// RunTombstones is the sweeper loop. It always runs (see the file comment) and
// honours ctx cancellation.
func RunTombstones(ctx context.Context, db store.DB, opts TombstoneOptions, logger zerolog.Logger) {
	if opts.Interval <= 0 {
		opts.Interval = time.Hour
	}
	logger.Info().
		Int("retention_days", opts.RetentionDays).
		Bool("dry_run", opts.DryRun).
		Dur("interval", opts.Interval).
		Msg("zdr tombstone sweeper started")
	ticker := time.NewTicker(opts.Interval)
	defer ticker.Stop()
	for {
		SweepTombstones(ctx, db, opts, logger)
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

// SweepTombstones runs one sweep and returns how many rows it deleted (0 in a dry run). A store error stops
// the sweep and is logged; the next tick tries again.
func SweepTombstones(ctx context.Context, db store.DB, opts TombstoneOptions, logger zerolog.Logger) int64 {
	if opts.Batch <= 0 {
		opts.Batch = 1000
	}
	now := time.Now
	if opts.Now != nil {
		now = opts.Now
	}
	cutoff := now().UTC().Add(-time.Duration(opts.RetentionDays) * 24 * time.Hour)
	tombs := db.ZdrTombstones()

	if opts.DryRun {
		// A dry run never calls DeleteBefore: it only counts.
		n, err := tombs.CountBefore(cutoff)
		if err != nil {
			logger.Warn().Err(err).Msg("zdr tombstone dry-run count failed")
			return 0
		}
		logger.Info().
			Int64("would_delete", n).
			Int("retention_days", opts.RetentionDays).
			Msg("zdr tombstone sweep (DRY-RUN: nothing deleted)")
		return 0
	}

	var total int64
	for ctx.Err() == nil {
		n, err := tombs.DeleteBefore(cutoff, opts.Batch)
		if err != nil {
			logger.Warn().Err(err).Int64("deleted_before_error", total).Msg("zdr tombstone sweep failed")
			return total
		}
		total += n
		if n < int64(opts.Batch) {
			break
		}
	}
	if total > 0 {
		logger.Info().Int64("deleted", total).Int("retention_days", opts.RetentionDays).Msg("zdr tombstone sweep complete")
	}
	return total
}
