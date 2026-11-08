package pg

import (
	"strings"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// The Postgres request table (migration 026) answers the same claim, lease and
// finish contract as the in-memory one, including SKIP LOCKED under concurrent
// claims. Needs QUARRY_TEST_DSN (see openIsolatedOrSkip); each subtest gets a
// private schema, so Claim sees only its own rows.
func TestPostgres_ScheduleRequestsConformance(t *testing.T) {
	storetest.ScheduleRequestsConformance(t, func(t *testing.T) store.DB { return openIsolatedOrSkip(t) })
}

// The database, not only the handler, refuses a second open request of one
// schedule: migration 026 carries the partial unique index (runs without a DSN;
// the conformance suite above proves the conflict against a real database).
func TestPostgres_ScheduleRequestsMigrationHasOneOpenIndex(t *testing.T) {
	sql, err := migrationFS.ReadFile("migrations/026_schedule_requests.sql")
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{
		"CREATE TABLE IF NOT EXISTS schedule_requests",
		"schedule_requests_one_open_idx",
		"WHERE status IN ('pending', 'claimed')",
		"schedule_requests_org_idem_idx",
	} {
		if !strings.Contains(string(sql), want) {
			t.Fatalf("migration 026 lacks %q", want)
		}
	}
}

