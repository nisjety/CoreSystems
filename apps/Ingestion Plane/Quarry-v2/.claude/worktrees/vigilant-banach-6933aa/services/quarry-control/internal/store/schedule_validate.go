package store

import (
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/cron"
)

// Validate ensures the Schedule's cron expression is well-formed
// and populates default fields (ID, CreatedAt) when missing.
func (s *Schedule) Validate() error {
	if err := cron.Validate(s.Cron); err != nil {
		return err
	}
	if s.ID == "" {
		s.ID = quarrycontracts.NewID(quarrycontracts.KindSchedule)
	}
	if s.CreatedAt == 0 {
		s.CreatedAt = time.Now().Unix()
	}
	return nil
}
