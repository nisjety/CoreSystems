package cron

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// gateRow answers the readiness probe with one count, or with the error it was given.
type gateRow struct {
	count int
	err   error
}

func (row gateRow) Scan(dest ...any) error {
	if row.err != nil {
		return row.err
	}
	*dest[0].(*int) = row.count
	return nil
}

// gateProbe stands in for the database the readiness probe runs on. It counts each probe and
// keeps the statement and arguments of the last one.
type gateProbe struct {
	count  int
	err    error
	probes int
	sql    string
	args   []any
}

func (p *gateProbe) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	p.probes++
	p.sql = sql
	p.args = args
	return gateRow{count: p.count, err: p.err}
}

// The probe reads the five cron_fires columns migration 0056 adds, and counts them in the current schema.
func TestFireSchemaProbeNamesTheFiveMigrationColumns(t *testing.T) {
	probe := &gateProbe{count: 5}
	ready, err := FireSchemaReady(context.Background(), probe)
	if err != nil || !ready {
		t.Fatalf("five columns: ready=%v err=%v, want ready", ready, err)
	}
	want := []string{"fire_key", "run_id", "thread_id", "withheld_effects", "outcome"}
	if len(probe.args) != 1 || !reflect.DeepEqual(probe.args[0], want) {
		t.Fatalf("probe arguments = %v, want the five column names %v", probe.args, want)
	}
	for _, fragment := range []string{"information_schema.columns", "table_name = 'cron_fires'", "current_schema()"} {
		if !strings.Contains(probe.sql, fragment) {
			t.Fatalf("probe must read the catalogue (%q missing): %s", fragment, probe.sql)
		}
	}
	if strings.Contains(strings.ToUpper(probe.sql), "ALTER") || strings.Contains(strings.ToUpper(probe.sql), "CREATE") {
		t.Fatalf("the probe must never run DDL: %s", probe.sql)
	}
}

// Four of five columns is not ready: a fire insert that names fire_key would still fail.
func TestFireSchemaNotReadyUntilEveryColumnIsThere(t *testing.T) {
	for _, count := range []int{0, 1, 4} {
		ready, err := FireSchemaReady(context.Background(), &gateProbe{count: count})
		if err != nil || ready {
			t.Fatalf("%d of 5 columns: ready=%v err=%v, want not ready", count, ready, err)
		}
	}
}

func TestFireSchemaProbeErrorIsReturned(t *testing.T) {
	boom := errors.New("connection refused")
	ready, err := FireSchemaReady(context.Background(), &gateProbe{err: boom})
	if ready || !errors.Is(err, boom) {
		t.Fatalf("probe error: ready=%v err=%v, want not ready with the probe error", ready, err)
	}
}

// A positive answer is kept: the gate does not probe again once the migration has landed.
func TestFireSchemaGateCachesAPositiveAnswer(t *testing.T) {
	ctx := context.Background()
	probe := &gateProbe{count: 5}
	gate := NewFireSchemaGate(probe)
	for i := 0; i < 3; i++ {
		if !gate.Ready(ctx) {
			t.Fatalf("call %d: the migrated schema must be ready", i)
		}
	}
	if probe.probes != 1 {
		t.Fatalf("probes = %d, want exactly one: a positive answer is cached for the process", probe.probes)
	}
}

// A negative answer is probed again, so the first call after the migration sees it.
func TestFireSchemaGateReprobesANegativeAnswer(t *testing.T) {
	ctx := context.Background()
	probe := &gateProbe{count: 4}
	gate := NewFireSchemaGate(probe)
	if gate.Ready(ctx) || gate.Ready(ctx) {
		t.Fatal("an unmigrated schema must not be ready")
	}
	if probe.probes != 2 {
		t.Fatalf("probes = %d, want two: a negative answer is probed on every call", probe.probes)
	}
	probe.count = 5
	if !gate.Ready(ctx) {
		t.Fatal("the first call after the migration must see it")
	}
	gate.Ready(ctx)
	if probe.probes != 3 {
		t.Fatalf("probes = %d after the migration, want three: the positive answer then sticks", probe.probes)
	}
}

// A probe that fails counts as not ready, and is not cached.
func TestFireSchemaGateTreatsAProbeErrorAsNotReady(t *testing.T) {
	ctx := context.Background()
	probe := &gateProbe{err: errors.New("reset by peer")}
	gate := NewFireSchemaGate(probe)
	ready, err := gate.Check(ctx)
	if ready || err == nil {
		t.Fatalf("probe error: ready=%v err=%v, want not ready with the error", ready, err)
	}
	if gate.Ready(ctx) {
		t.Fatal("a probe error must not read as ready")
	}
	if probe.probes != 2 {
		t.Fatalf("probes = %d, want two: a probe error is not cached", probe.probes)
	}
}

// A nil gate is always ready, so a caller built without one behaves exactly as before.
func TestNilFireSchemaGateIsAlwaysReady(t *testing.T) {
	var gate *FireSchemaGate
	if !gate.Ready(context.Background()) {
		t.Fatal("a nil gate must be ready")
	}
	if ready, err := gate.Check(context.Background()); !ready || err != nil {
		t.Fatalf("nil gate Check = %v, %v; want ready with no error", ready, err)
	}
}

// unreachableSweepPool is a pool that never connects. It is built lazily, and the one address it names
// refuses at once, so a sweep that reaches it gets a connection error and a test can tell the two apart.
func unreachableSweepPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), "postgres://capability@127.0.0.1:1/registry?sslmode=disable")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(pool.Close)
	return pool
}

// The sweep waits while the gate is pending: it returns with nothing claimed and never reaches the pool.
// The pool refuses every connection, so a sweep that reached it would return an error, not (0, nil).
func TestSweepWaitsWhileTheFireSchemaIsPending(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{pool: unreachableSweepPool(t), authorizer: authorizer}).WithFireSchemaGate(NewFireSchemaGate(&gateProbe{count: 4}))
	fired, err := sweeper.RunOnce(ctx)
	if fired != 0 || err != nil {
		t.Fatalf("pending sweep: fired=%d err=%v, want 0 and no error", fired, err)
	}
	if authorizer.calls != 0 {
		t.Fatalf("a pending sweep asked Control to authorize %d fire(s)", authorizer.calls)
	}
}

// Once the schema is ready the sweep goes on to the database. The refused connection comes back as the
// sweep's error, nothing fires, and the probe is asked once.
func TestSweepProceedsOnceTheFireSchemaIsReady(t *testing.T) {
	ctx := context.Background()
	if !(&Sweeper{}).WithFireSchemaGate(NewFireSchemaGate(&gateProbe{count: 5})).fireSchemaReady(ctx) {
		t.Fatal("a migrated schema must let the sweep run")
	}
	if !(&Sweeper{}).fireSchemaReady(ctx) {
		t.Fatal("a sweeper without a gate must never wait")
	}

	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	probe := &gateProbe{count: 5}
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{pool: unreachableSweepPool(t), authorizer: authorizer}).WithFireSchemaGate(NewFireSchemaGate(probe))
	fired, err := sweeper.RunOnce(ctx)
	if err == nil || fired != 0 {
		t.Fatalf("ready sweep: fired=%d err=%v, want no fire and the pool's connection error", fired, err)
	}
	if probe.probes != 1 {
		t.Fatalf("probes = %d, want one: a ready schema is cached after the first sweep", probe.probes)
	}
	if authorizer.calls != 0 {
		t.Fatalf("a sweep that never reached a schedule asked Control to authorize %d fire(s)", authorizer.calls)
	}
}

// The sweeper logs the wait once and the release once, and never names an org or a schedule.
func TestSweeperLogsEachFireSchemaChangeOnce(t *testing.T) {
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&logs, nil)))
	defer slog.SetDefault(previous)

	probe := &gateProbe{count: 4}
	sweeper := (&Sweeper{}).WithFireSchemaGate(NewFireSchemaGate(probe))
	ctx := context.Background()
	sweeper.fireSchemaReady(ctx)
	sweeper.fireSchemaReady(ctx)
	probe.count = 5
	sweeper.fireSchemaReady(ctx)
	sweeper.fireSchemaReady(ctx)

	out := logs.String()
	if n := strings.Count(out, "waiting for session-core migration 0056"); n != 1 {
		t.Fatalf("waiting logged %d times, want once per change of state:\n%s", n, out)
	}
	if n := strings.Count(out, "cron fire schema ready"); n != 1 {
		t.Fatalf("ready logged %d times, want once:\n%s", n, out)
	}
	if strings.Contains(out, "org-1") || strings.Contains(out, "sched-") {
		t.Fatalf("the gate logs name an org or a schedule:\n%s", out)
	}
}
