package rawstore

import (
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// S2 (D-B7): an HTTP status outside 100 to 599 is stored as NULL. It never fails
// the attempt that carried it, and a valid status is kept as it is.
func TestAttemptStatusOutsideRangeIsStoredAsNull(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	const id = "2026-100051"

	for _, status := range []int{0, -1, 99, 600} {
		if err := s.RecordAttempt(ctx, run, doffinsync.Attempt{
			DoffinID: id, Outcome: doffinsync.OutcomeHTTPError, HTTPStatus: status,
			Message: "upstream answered", At: fixedNow,
		}); err != nil {
			t.Fatalf("RecordAttempt with status %d: %v", status, err)
		}
	}
	if err := s.RecordAttempt(ctx, run, doffinsync.Attempt{
		DoffinID: id, Outcome: doffinsync.OutcomeHTTPError, HTTPStatus: 503,
		Message: "upstream answered", At: fixedNow,
	}); err != nil {
		t.Fatalf("RecordAttempt with status 503: %v", err)
	}
	if got := count(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND http_status IS NULL`, id); got != 4 {
		t.Fatalf("attempts with a NULL status = %d, want 4", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND http_status = 503`, id); got != 1 {
		t.Fatalf("attempts with status 503 = %d, want 1", got)
	}
}
