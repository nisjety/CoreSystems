package pg

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// INNH-9: the Postgres team credit query counts page_fetched events only, the
// same rule as the in-memory store and the per-job credits. Needs a Postgres at
// QUARRY_TEST_DSN and skips without it (QUARRY_REQUIRE_DB=1 fails instead).
func TestPostgres_CreditUsage(t *testing.T) {
	storetest.CreditUsageConformance(t, func(t *testing.T) store.DB { return openIsolatedOrSkip(t) })
}
