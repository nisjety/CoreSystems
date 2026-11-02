package rawstore

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// TestFinishDayRefusesAZeroRunIDBeforeAnySQL (A6): a day record with no run id
// cannot name the run it belongs to, so the store refuses it with ErrInvalidRecord
// before any SQL runs. The store is built with no pool, so any SQL would panic and
// fail the test. This needs no database.
func TestFinishDayRefusesAZeroRunIDBeforeAnySQL(t *testing.T) {
	t.Parallel()

	s := New(nil)
	rec := doffinsync.DayRecord{
		Day:   time.Date(2026, time.October, 7, 0, 0, 0, 0, time.UTC),
		State: doffinsync.StateDone,
		RunID: 0,
	}

	err := s.FinishDay(context.Background(), rec)
	if !errors.Is(err, ErrInvalidRecord) {
		t.Fatalf("FinishDay with RunID 0: err = %v, want ErrInvalidRecord", err)
	}
}
