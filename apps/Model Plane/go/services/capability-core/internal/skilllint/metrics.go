package skilllint

import (
	"context"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"
)

// findingsTotal has the same name and code label as session-core's counter, so one query
// over both services shows how many existing rows each rule would refuse before enforce.
var findingsTotal, _ = otel.Meter(telemetry.MeterName).Int64Counter(
	"skill_lint_findings_total",
	metric.WithDescription("Skill lint findings on registry writes, labelled by rule code, severity and mode."),
)

// Count records every finding of one write. Labels are closed words, never skill text.
func Count(ctx context.Context, mode Mode, findings []Finding) {
	if findingsTotal == nil {
		return
	}
	for _, finding := range findings {
		findingsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("code", finding.Code),
			attribute.String("severity", string(finding.Severity)),
			attribute.String("mode", string(mode)),
		))
	}
}
