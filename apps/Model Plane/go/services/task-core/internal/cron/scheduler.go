// Package cron implements a lightweight cron scheduler that ticks on a fixed
// interval, evaluates which tasks are due, and dispatches them. Cron
// expressions follow the standard five-field format: minute hour day-of-month
// month day-of-week.
package cron

import (
	"context"
	"fmt"
	"log/slog"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/task-core/internal/store"
	"github.com/triodelab/model-plane/services/task-core/internal/telemetry"
)

const tickInterval = 30 * time.Second

// TaskStore is the subset of store.Store that the scheduler needs.
type TaskStore interface {
	ListDue(now time.Time) []*store.Task
	UpdateStatus(id string, status store.Status) error
	SetNextRun(id string, next time.Time) error
}

// Scheduler periodically checks for due tasks and dispatches them.
type Scheduler struct {
	store  TaskStore
	logger *slog.Logger
}

// NewScheduler creates a Scheduler backed by the provided store.
func NewScheduler(s TaskStore, logger *slog.Logger) *Scheduler {
	return &Scheduler{store: s, logger: logger}
}

// Run starts the tick loop. It blocks until the context is cancelled.
func (sc *Scheduler) Run(ctx context.Context) {
	sc.logger.Info("cron scheduler started", "tick_interval", tickInterval.String())
	ticker := time.NewTicker(tickInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			sc.logger.Info("cron scheduler stopping")
			return
		case now := <-ticker.C:
			sc.tick(now.UTC())
		}
	}
}

// tick processes all due tasks for the given instant.
func (sc *Scheduler) tick(now time.Time) {
	due := sc.store.ListDue(now)
	if len(due) == 0 {
		return
	}
	sc.logger.Info("cron tick: due tasks found", "count", len(due))
	for _, t := range due {
		telemetry.TasksDispatchedTotal.Add(context.Background(), 1)
		sc.logger.Info("dispatching task", "task_id", t.ID, "name", t.Name)

		if err := sc.store.UpdateStatus(t.ID, store.StatusRunning); err != nil {
			sc.logger.Error("failed to mark task running", "task_id", t.ID, "error", err)
			continue
		}

		// Simulate execution: mark completed immediately.
		// A production implementation would enqueue to a work queue.
		if err := sc.store.UpdateStatus(t.ID, store.StatusCompleted); err != nil {
			sc.logger.Error("failed to mark task completed", "task_id", t.ID, "error", err)
			continue
		}

		// Reschedule recurring tasks.
		if t.CronExpr != "" {
			next, err := NextRun(t.CronExpr, now)
			if err != nil {
				sc.logger.Error("failed to compute next run", "task_id", t.ID, "cron", t.CronExpr, "error", err)
				continue
			}
			if err := sc.store.SetNextRun(t.ID, next); err != nil {
				sc.logger.Error("failed to set next run", "task_id", t.ID, "error", err)
				continue
			}
			// Reset to pending so it fires again.
			_ = sc.store.UpdateStatus(t.ID, store.StatusPending)
			sc.logger.Info("rescheduled task", "task_id", t.ID, "next_run", next)
		}
	}
}

// NextRun computes the next execution time for a five-field cron expression
// relative to the given reference time. Supported fields: minute (0-59),
// hour (0-23), day-of-month (1-31), month (1-12), day-of-week (0-6, 0=Sunday).
// Each field may be "*" (any), a single integer, or a "*/step" pattern.
func NextRun(expr string, after time.Time) (time.Time, error) {
	fields := strings.Fields(expr)
	if len(fields) != 5 {
		return time.Time{}, fmt.Errorf("cron expression must have 5 fields, got %d", len(fields))
	}

	minuteSpec, err := parseField(fields[0], 0, 59)
	if err != nil {
		return time.Time{}, fmt.Errorf("minute field: %w", err)
	}
	hourSpec, err := parseField(fields[1], 0, 23)
	if err != nil {
		return time.Time{}, fmt.Errorf("hour field: %w", err)
	}
	domSpec, err := parseField(fields[2], 1, 31)
	if err != nil {
		return time.Time{}, fmt.Errorf("day-of-month field: %w", err)
	}
	monthSpec, err := parseField(fields[3], 1, 12)
	if err != nil {
		return time.Time{}, fmt.Errorf("month field: %w", err)
	}
	dowSpec, err := parseField(fields[4], 0, 6)
	if err != nil {
		return time.Time{}, fmt.Errorf("day-of-week field: %w", err)
	}

	// Walk forward from one minute after "after" up to 366 days.
	candidate := after.Truncate(time.Minute).Add(time.Minute)
	limit := after.Add(366 * 24 * time.Hour)
	for candidate.Before(limit) {
		if matchSet(monthSpec, int(candidate.Month())) &&
			matchSet(domSpec, candidate.Day()) &&
			matchSet(dowSpec, int(candidate.Weekday())) &&
			matchSet(hourSpec, candidate.Hour()) &&
			matchSet(minuteSpec, candidate.Minute()) {
			return candidate, nil
		}
		candidate = candidate.Add(time.Minute)
	}
	return time.Time{}, fmt.Errorf("no matching time found within 366 days for %q", expr)
}

// fieldSet is nil for wildcard ("*"), otherwise contains the set of matching
// values.
type fieldSet map[int]struct{}

func parseField(raw string, min, max int) (fieldSet, error) {
	if raw == "*" {
		return nil, nil // wildcard
	}
	if after, ok := strings.CutPrefix(raw, "*/"); ok {
		step, err := strconv.Atoi(after)
		if err != nil || step <= 0 {
			return nil, fmt.Errorf("invalid step %q", raw)
		}
		s := make(fieldSet)
		for v := min; v <= max; v += step {
			s[v] = struct{}{}
		}
		return s, nil
	}
	v, err := strconv.Atoi(raw)
	if err != nil {
		return nil, fmt.Errorf("invalid value %q", raw)
	}
	if v < min || v > max {
		return nil, fmt.Errorf("value %d out of range [%d, %d]", v, min, max)
	}
	return fieldSet{v: {}}, nil
}

func matchSet(s fieldSet, val int) bool {
	if s == nil {
		return true // wildcard
	}
	_, ok := s[val]
	return ok
}
