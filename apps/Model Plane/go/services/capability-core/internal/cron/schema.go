package cron

import (
	"context"
	"sync/atomic"

	"github.com/jackc/pgx/v5"
)

// The cron_fires columns below come from session-core migration 0056_cron_fire_run_link.sql.
// The table itself is session-core's (migration 0004), and capability-core only writes fire rows
// into it. Until 0056 is applied every fire insert fails, so schedules stop firing and Run now
// errors. The gate answers whether the columns are in place, so the sweep and Run now wait for the
// migration instead of failing on every fire (F-f02-02). It never runs DDL.

// fireSchemaColumns are the cron_fires columns session-core migration 0056 adds.
var fireSchemaColumns = []string{"fire_key", "run_id", "thread_id", "withheld_effects", "outcome"}

// fireSchemaProbe counts which of those columns the current schema has.
const fireSchemaProbe = `
	SELECT count(*) FROM information_schema.columns
	WHERE table_schema = current_schema() AND table_name = 'cron_fires' AND column_name::text = ANY($1::text[])
`

// FireSchemaQuerier is the one statement the probe runs. *pgxpool.Pool satisfies it.
type FireSchemaQuerier interface {
	QueryRow(context.Context, string, ...any) pgx.Row
}

// FireSchemaReady reports whether cron_fires has every column session-core migration 0056 adds.
func FireSchemaReady(ctx context.Context, q FireSchemaQuerier) (bool, error) {
	var found int
	if err := q.QueryRow(ctx, fireSchemaProbe, fireSchemaColumns).Scan(&found); err != nil {
		return false, err
	}
	return found == len(fireSchemaColumns), nil
}

// FireSchemaGate answers FireSchemaReady for the process, and one gate is shared by the sweeper and
// the cron handler. A positive answer is kept for the life of the process, because a migration does
// not roll back under a running service. A negative answer is probed again on every call, so the
// first call after the migration lands sees it. A nil gate is always ready, which keeps a caller
// built without one exactly as it was.
type FireSchemaGate struct {
	q     FireSchemaQuerier
	ready atomic.Bool
}

// NewFireSchemaGate builds the gate over the database the probe runs on.
func NewFireSchemaGate(q FireSchemaQuerier) *FireSchemaGate {
	return &FireSchemaGate{q: q}
}

// Check reports whether the fire schema is in place. A probe error comes back with false, and the
// caller treats it as not ready.
func (g *FireSchemaGate) Check(ctx context.Context) (bool, error) {
	if g == nil {
		return true, nil
	}
	if g.ready.Load() {
		return true, nil
	}
	ready, err := FireSchemaReady(ctx, g.q)
	if err != nil {
		return false, err
	}
	if ready {
		g.ready.Store(true)
	}
	return ready, nil
}

// Ready is Check without the error, for a caller that only needs the answer.
func (g *FireSchemaGate) Ready(ctx context.Context) bool {
	ready, _ := g.Check(ctx)
	return ready
}
