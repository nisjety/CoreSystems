package pg

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// D2-016: concurrent writers of one run get distinct, contiguous seqs and lose
// no event. The statement reads the run's max seq and inserts under one
// transaction-scoped advisory lock; before it, NextSeq and Append were two
// steps, two writers read the same max and one insert lost on
// events_run_seq_uidx (and the batch handler dropped it as a replay). Needs a
// Postgres at QUARRY_TEST_DSN and skips without it (QUARRY_REQUIRE_DB=1 fails
// instead).
func TestPostgres_EventSeq(t *testing.T) {
	storetest.EventSeqConformance(t, func(t *testing.T) store.DB { return openIsolatedOrSkip(t) })
}
