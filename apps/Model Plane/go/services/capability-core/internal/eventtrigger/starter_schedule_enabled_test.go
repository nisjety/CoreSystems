package eventtrigger

import (
	"context"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

// W4-T4 (EP-7, plan 3.4 for EVENT_TRIGGER_RUN_ENABLED): the starter turns an authorised trigger fire into a scheduled run
// of the schedule the trigger names. The W4-T3 bind check deliberately does not require cron_schedules.enabled ("the
// sweeper checks enabled on every fire. W4-T4's starter must therefore check cron_schedules.enabled itself"). A schedule the
// owner turned off, or one the cron sweeper paused (paused_failures, paused_expired set enabled=false), must therefore not
// be started by its trigger.

// claimCapturingTx is a pgx.Tx that records the SQL of the claim query and answers no row.
type claimCapturingTx struct {
	pgx.Tx
	sql string
}

func (tx *claimCapturingTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	tx.sql = sql
	return noRow{}
}

type noRow struct{}

func (noRow) Scan(...any) error { return pgx.ErrNoRows }

func TestStarterClaimsOnlyFiresWhoseScheduleIsEnabled(t *testing.T) {
	tx := &claimCapturingTx{}
	candidate, err := pgStartBatch{tx: tx}.next(context.Background(), nil, time.Time{})
	if err != nil || candidate != nil {
		t.Fatalf("next = %+v, %v; want no candidate", candidate, err)
	}
	sql := strings.Join(strings.Fields(tx.sql), " ")
	if !strings.Contains(sql, "FROM event_trigger_fires f") {
		t.Fatalf("captured SQL is not the claim query: %s", sql)
	}
	// The claim must require the bound schedule to be enabled, as the cron sweeper's due query does
	// ("WHERE enabled AND deleted_at IS NULL").
	if !regexp.MustCompile(`\bs\.enabled\b`).MatchString(sql) {
		t.Fatalf("the starter's claim does not require cron_schedules.enabled; a paused or switched-off schedule is still started by its trigger:\n%s", sql)
	}
}
