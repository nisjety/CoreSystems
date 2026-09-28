package process

import (
	"context"
	"log/slog"
	"os"
	"strconv"
	"strings"
	"time"
)

// DefaultSweepInterval is how often the sweeper looks for hosts that have
// stopped heartbeating.
const DefaultSweepInterval = 30 * time.Second

// DefaultStaleAfter is how long a live process may go without a heartbeat
// before the registry declares its host lost. The host heartbeats every 15s
// (design §3.3), so this tolerates three missed beats before acting — long
// enough that a slow round-trip is not mistaken for a dead worker, short
// enough that a reader is not told a dead process is still running for
// minutes.
const DefaultStaleAfter = 60 * time.Second

// staleSweeper is the subset of Store the sweeper needs. A narrow interface
// so its tests need no database at all.
type staleSweeper interface {
	SweepStale(context.Context, time.Duration) (int64, error)
}

// Sweeper marks live processes whose host has stopped heartbeating as LOST.
//
// This is sandbox-manager's first background goroutine. It is deliberately
// the smallest possible one: a single idempotent UPDATE, no claim, no lease,
// no transaction — so any number of replicas may run it concurrently without
// coordinating, and a replica that dies mid-sweep leaves nothing half-done.
// TTL expiry is the host's job (it holds the timer); the registry only needs
// to notice a host that stopped talking.
type Sweeper struct {
	store      staleSweeper
	interval   time.Duration
	staleAfter time.Duration
}

// NewSweeper constructs a sweeper with the default cadence.
func NewSweeper(store staleSweeper) *Sweeper {
	return &Sweeper{store: store, interval: DefaultSweepInterval, staleAfter: DefaultStaleAfter}
}

// WithTiming returns a copy using the given cadence; non-positive values keep
// the default. Used by tests and by the env overrides in cmd/main.go.
func (s *Sweeper) WithTiming(interval, staleAfter time.Duration) *Sweeper {
	out := *s
	if interval > 0 {
		out.interval = interval
	}
	if staleAfter > 0 {
		out.staleAfter = staleAfter
	}
	return &out
}

// Start runs the sweep loop until ctx is cancelled. Errors are logged, not
// fatal — a registry that cannot reach Postgres for a minute must not take
// the service down with it. The first sweep runs immediately so a restart
// re-scans rather than waiting out a full interval, the same shape
// capability-core's cron sweeper uses.
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
	lost, err := s.store.SweepStale(ctx, s.staleAfter)
	if err != nil {
		if ctx.Err() != nil {
			return
		}
		slog.Warn("process staleness sweep failed", "error", err)
		return
	}
	if lost > 0 {
		slog.Warn("marked processes lost after their host stopped heartbeating",
			"count", lost, "stale_after", s.staleAfter.String())
	}
}

// SweeperEnabled reports whether the sweeper should run. Enabled unless
// explicitly switched off, matching capability-core's own
// CRON_SWEEPER_ENABLED=false opt-out.
func SweeperEnabled(value string) bool {
	return strings.TrimSpace(value) != "false"
}

// TimingFromEnv reads the sweeper's cadence overrides. An unset, blank, or
// unparseable value keeps the default rather than failing startup: a typo in
// an optional tuning knob must not stop the service from running, and the
// default is always a safe answer.
func TimingFromEnv(getenv func(string) string) (interval, staleAfter time.Duration) {
	return secondsOrDefault(getenv("PROCESS_SWEEP_INTERVAL_SECS"), DefaultSweepInterval),
		secondsOrDefault(getenv("PROCESS_HEARTBEAT_STALE_SECS"), DefaultStaleAfter)
}

func secondsOrDefault(raw string, fallback time.Duration) time.Duration {
	seconds, err := strconv.Atoi(strings.TrimSpace(raw))
	if err != nil || seconds <= 0 {
		return fallback
	}
	return time.Duration(seconds) * time.Second
}

// EnvSweeperEnabled is a convenience for cmd/main.go.
func EnvSweeperEnabled() bool { return SweeperEnabled(os.Getenv("PROCESS_SWEEPER_ENABLED")) }
