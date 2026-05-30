// Package telemetry exposes task-core OpenTelemetry metric instruments.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter scope used by task-core instruments.
const MeterName = "github.com/triodelab/model-plane/services/task-core"

var (
	// RequestsTotal counts HTTP requests handled, labelled by method and path.
	RequestsTotal metric.Int64Counter
	// TasksCreatedTotal counts task creation attempts, labelled by outcome.
	TasksCreatedTotal metric.Int64Counter
	// TasksDispatchedTotal counts tasks dispatched by the cron scheduler.
	TasksDispatchedTotal metric.Int64Counter
	// TasksTriggeredTotal counts manual task triggers, labelled by outcome.
	TasksTriggeredTotal metric.Int64Counter
	// CronTicksTotal counts cron scheduler tick iterations.
	CronTicksTotal metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)
	RequestsTotal, _ = meter.Int64Counter(
		"task_core_requests_total",
		metric.WithDescription("Total HTTP requests handled by task-core, labelled by method and path."),
	)
	TasksCreatedTotal, _ = meter.Int64Counter(
		"task_core_tasks_created_total",
		metric.WithDescription("Total task creation attempts, labelled by outcome."),
	)
	TasksDispatchedTotal, _ = meter.Int64Counter(
		"task_core_tasks_dispatched_total",
		metric.WithDescription("Total tasks dispatched by the cron scheduler."),
	)
	TasksTriggeredTotal, _ = meter.Int64Counter(
		"task_core_tasks_triggered_total",
		metric.WithDescription("Total manual task trigger attempts, labelled by outcome."),
	)
	CronTicksTotal, _ = meter.Int64Counter(
		"task_core_cron_ticks_total",
		metric.WithDescription("Total cron scheduler tick iterations."),
	)
}
