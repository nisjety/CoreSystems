package store

import (
	"fmt"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/cron"
)

// TargetKindChangeMonitor is the recurring change-monitoring schedule kind.
// Its cadence is preset-driven (see presetCron) rather than free-form cron.
const TargetKindChangeMonitor = "change_monitor"

// presetCron maps the fixed change-monitor cadences to literal 5-field cron
// expressions. cron.Validate rejects "@daily"/"@hourly" aliases and any
// non-5-field expression, so the presets MUST resolve to explicit fields.
// Times are in the scheduler's timezone (UTC in the orchestrator).
var presetCron = map[string]string{
	"hourly": "0 * * * *",
	"daily":  "0 9 * * *",
	"weekly": "0 9 * * 1",
}

// PresetCron returns the 5-field cron for a known change-monitor preset.
func PresetCron(preset string) (string, bool) {
	c, ok := presetCron[preset]
	return c, ok
}

// Validate ensures the Schedule is well-formed and populates default fields
// (ID, CreatedAt) when missing.
//
//   - change_monitor schedules are preset-driven: the caller sends a fixed
//     cadence and Validate maps it to a literal 5-field cron server-side, so
//     the cadence stays deterministic and cron.Validate passes.
//   - org_id is mandatory on every schedule so the Temporal workflow Args and
//     all downstream baselines/diffs are tenant-scoped.
func (s *Schedule) Validate() error {
	if s.TargetKind == TargetKindChangeMonitor {
		c, ok := PresetCron(s.Preset)
		if !ok {
			return fmt.Errorf("change_monitor schedule requires a known preset (hourly|daily|weekly), got %q", s.Preset)
		}
		s.Cron = c
	}
	if err := cron.Validate(s.Cron); err != nil {
		return err
	}
	if s.OrgID == "" {
		return fmt.Errorf("org_id required")
	}
	if s.TargetRef == "" {
		return fmt.Errorf("target_ref required")
	}
	if s.ID == "" {
		s.ID = quarrycontracts.NewID(quarrycontracts.KindSchedule)
	}
	if s.CreatedAt == 0 {
		s.CreatedAt = time.Now().Unix()
	}
	return nil
}
