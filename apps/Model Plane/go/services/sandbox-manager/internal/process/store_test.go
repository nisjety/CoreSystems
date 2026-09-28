package process

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// processDatabaseStub mirrors internal/workspace's workspaceDatabaseStub and
// internal/lease's leaseDatabaseStub: a hand-rolled processDatabase recording
// every call's query and bound args, with scripted results.
//
// What a stub can prove here is validation, statement shape, and bound
// arguments. What it cannot prove — that the fence predicates actually
// exclude a superseded host, that ON CONFLICT DO NOTHING makes a replayed
// batch a no-op, that the retention window keeps head and tail — needs a
// real planner, and lives in store_integration_test.go.
type processDatabaseStub struct {
	execTag   pgconn.CommandTag
	execErr   error
	execTags  []pgconn.CommandTag
	execCalls int

	rows     pgx.Rows
	queryErr error

	rowScans []func(dest ...any) error
	rowCalls int

	queries []string
	args    [][]any
}

func (d *processDatabaseStub) record(query string, args []any) {
	d.queries = append(d.queries, query)
	d.args = append(d.args, append([]any(nil), args...))
}

func (d *processDatabaseStub) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	d.record(query, args)
	index := d.execCalls
	d.execCalls++
	if index < len(d.execTags) {
		return d.execTags[index], d.execErr
	}
	return d.execTag, d.execErr
}

func (d *processDatabaseStub) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	d.record(query, args)
	return d.rows, d.queryErr
}

func (d *processDatabaseStub) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	d.record(query, args)
	index := d.rowCalls
	d.rowCalls++
	if index < len(d.rowScans) {
		return rowFunc(d.rowScans[index])
	}
	return rowFunc(func(...any) error { return pgx.ErrNoRows })
}

type rowFunc func(dest ...any) error

func (f rowFunc) Scan(dest ...any) error { return f(dest...) }

// scanValues builds a Scan implementation that assigns a fixed set of values
// into the caller's destinations, the same reflect-based shape
// internal/workspace's manifestRows.Scan uses.
func scanValues(values ...any) func(dest ...any) error {
	return func(dest ...any) error {
		if len(dest) != len(values) {
			return fmt.Errorf("stub scan: got %d destinations, have %d values", len(dest), len(values))
		}
		for i := range dest {
			reflect.ValueOf(dest[i]).Elem().Set(reflect.ValueOf(values[i]))
		}
		return nil
	}
}

func testFence() Fence {
	return Fence{ProcessID: "proc-1", OrgID: "org-a", BackendID: "backend-1", HostEpoch: "epoch-1"}
}

func testRegisterRequest() RegisterRequest {
	return RegisterRequest{
		ID:            "proc-1",
		OrgID:         "org-a",
		LeaseID:       "lease-1",
		BackendID:     "backend-1",
		HostEpoch:     "epoch-1",
		RunID:         "run-1",
		StepID:        "step-1",
		SubjectID:     "user-a",
		Command:       Command{Program: "python3", Args: []string{"main.py"}},
		CommandDigest: "sha256:abc",
		TTLSeconds:    900,
	}
}

func TestNewStoreRejectsNilPool(t *testing.T) {
	t.Parallel()
	if _, err := NewStore(nil); err == nil {
		t.Fatal("nil pool unexpectedly accepted")
	}
}

func TestWithLimitsOverridesOnlyWhatIsSet(t *testing.T) {
	t.Parallel()
	store := (&Store{limits: DefaultLimits()}).WithLimits(Limits{MaxLivePerLease: 2})
	if store.Limits().MaxLivePerLease != 2 {
		t.Fatalf("MaxLivePerLease = %d, want 2", store.Limits().MaxLivePerLease)
	}
	if store.Limits().MaxLivePerSpace != DefaultLimits().MaxLivePerSpace {
		t.Fatalf("unset field should keep its default, got %d", store.Limits().MaxLivePerSpace)
	}
}

func TestStateClassification(t *testing.T) {
	t.Parallel()
	for _, s := range []State{StateStarting, StateRunning} {
		if !s.IsLive() || s.IsTerminal() {
			t.Fatalf("state %d should be live and not terminal", s)
		}
	}
	for _, s := range []State{StateExited, StateKilled, StateLost, StateExpired} {
		if s.IsLive() || !s.IsTerminal() {
			t.Fatalf("state %d should be terminal and not live", s)
		}
	}
	if StateUnspecified.IsLive() || StateUnspecified.IsTerminal() {
		t.Fatal("the unspecified state is neither live nor terminal")
	}
}

func TestRegisterRejectsIncompleteRequests(t *testing.T) {
	t.Parallel()
	cases := map[string]func(*RegisterRequest){
		"missing id":         func(r *RegisterRequest) { r.ID = "" },
		"missing org":        func(r *RegisterRequest) { r.OrgID = "" },
		"missing lease":      func(r *RegisterRequest) { r.LeaseID = "" },
		"missing backend":    func(r *RegisterRequest) { r.BackendID = "" },
		"missing host epoch": func(r *RegisterRequest) { r.HostEpoch = "" },
		"missing run":        func(r *RegisterRequest) { r.RunID = "" },
		"missing subject":    func(r *RegisterRequest) { r.SubjectID = "" },
		"missing program":    func(r *RegisterRequest) { r.Command.Program = "" },
		"missing digest":     func(r *RegisterRequest) { r.CommandDigest = "" },
		"zero ttl":           func(r *RegisterRequest) { r.TTLSeconds = 0 },
		"negative ttl":       func(r *RegisterRequest) { r.TTLSeconds = -1 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			database := &processDatabaseStub{}
			store := &Store{pool: database, limits: DefaultLimits()}
			req := testRegisterRequest()
			mutate(&req)
			if _, err := store.Register(context.Background(), req); err == nil {
				t.Fatal("incomplete request unexpectedly accepted")
			}
			if len(database.queries) != 0 {
				t.Fatalf("no statement should run for a rejected call, got %d", len(database.queries))
			}
		})
	}
}

// TestRegisterGatesOnTheLeaseAndBothLimitsInOneStatement pins the property
// that makes the limits real: eligibility and both live counts are evaluated
// inside the INSERT, joined against the lease, so two concurrent
// registrations cannot both pass.
func TestRegisterGatesOnTheLeaseAndBothLimitsInOneStatement(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{rowScans: []func(...any) error{
		// The insert matches no row; the diagnostic below explains why.
		func(...any) error { return pgx.ErrNoRows },
		scanValues(int32(2), false, "backend-1", "space-1", false, int64(0), int64(0)),
	}}
	store := &Store{pool: database, limits: DefaultLimits()}

	_, err := store.Register(context.Background(), testRegisterRequest())
	if !errors.Is(err, ErrProcessesNotPermitted) {
		t.Fatalf("err = %v, want ErrProcessesNotPermitted", err)
	}

	insert := database.queries[0]
	for _, fragment := range []string{
		"FROM leases l",
		"l.processes_permitted",
		"l.backend_id = $4",
		"l.expires_at > now()",
		"l.space_id <> ''",
		"SELECT $1, $2, l.space_id, $3, $4, $5,",
		"LEAST(now() + ($15::bigint * interval '1 second'), l.expires_at)",
		"WHERE p.lease_id = $3 AND p.state IN (1, 2)) < $14",
		"WHERE p.org_id = $2 AND p.space_id = l.space_id AND p.state IN (1, 2)) < $16",
	} {
		if !strings.Contains(insert, fragment) {
			t.Fatalf("insert is missing %q:\n%s", fragment, insert)
		}
	}
	wantArgs := []any{
		"proc-1", "org-a", "lease-1", "backend-1", "epoch-1",
		"run-1", "step-1", "user-a", `{"program":"python3","args":["main.py"]}`, "sha256:abc",
		int16(StateStarting), int32(900), int32(2),
		DefaultLimits().MaxLivePerLease, int64(900), DefaultLimits().MaxLivePerSpace,
	}
	if !reflect.DeepEqual(database.args[0], wantArgs) {
		t.Fatalf("insert args = %#v,\nwant %#v", database.args[0], wantArgs)
	}
}

func TestRegisterClassifiesEachRefusal(t *testing.T) {
	t.Parallel()
	limits := DefaultLimits()
	cases := []struct {
		name       string
		diagnostic []any
		want       error
		contains   string
	}{
		{
			name:       "not permitted",
			diagnostic: []any{int32(2), false, "backend-1", "space-1", false, int64(0), int64(0)},
			want:       ErrProcessesNotPermitted,
			contains:   "space:processes",
		},
		{
			name:       "wrong backend",
			diagnostic: []any{int32(2), true, "backend-2", "space-1", false, int64(0), int64(0)},
			want:       ErrLeaseNotEligible,
			contains:   "pinned to another backend",
		},
		{
			name:       "expired lease",
			diagnostic: []any{int32(2), true, "backend-1", "space-1", true, int64(0), int64(0)},
			want:       ErrLeaseNotEligible,
			contains:   "expired",
		},
		{
			name:       "lease still scratch",
			diagnostic: []any{int32(1), true, "backend-1", "space-1", false, int64(0), int64(0)},
			want:       ErrLeaseNotEligible,
			contains:   "not ACTIVE",
		},
		{
			name:       "lease process limit",
			diagnostic: []any{int32(2), true, "backend-1", "space-1", false, int64(limits.MaxLivePerLease), int64(0)},
			want:       ErrProcessLimit,
			contains:   "on this lease",
		},
		{
			name:       "space process limit",
			diagnostic: []any{int32(2), true, "backend-1", "space-1", false, int64(0), int64(limits.MaxLivePerSpace)},
			want:       ErrProcessLimit,
			contains:   "in this Space",
		},
		{
			name:       "lost race",
			diagnostic: []any{int32(2), true, "backend-1", "space-1", false, int64(0), int64(0)},
			want:       ErrProcessLimit,
			contains:   "concurrent race",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			database := &processDatabaseStub{rowScans: []func(...any) error{
				func(...any) error { return pgx.ErrNoRows },
				scanValues(tc.diagnostic...),
			}}
			store := &Store{pool: database, limits: limits}
			_, err := store.Register(context.Background(), testRegisterRequest())
			if !errors.Is(err, tc.want) {
				t.Fatalf("err = %v, want %v", err, tc.want)
			}
			if !strings.Contains(err.Error(), tc.contains) {
				t.Fatalf("err %q does not explain the refusal (%q)", err, tc.contains)
			}
		})
	}
}

func TestRegisterReportsAMissingLease(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{rowScans: []func(...any) error{
		func(...any) error { return pgx.ErrNoRows },
		func(...any) error { return pgx.ErrNoRows },
	}}
	store := &Store{pool: database, limits: DefaultLimits()}
	_, err := store.Register(context.Background(), testRegisterRequest())
	if !errors.Is(err, ErrLeaseNotEligible) {
		t.Fatalf("err = %v, want ErrLeaseNotEligible", err)
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Fatalf("err = %q, want it to name the missing lease", err)
	}
}

func TestFencedWritesRequireEveryFenceField(t *testing.T) {
	t.Parallel()
	partial := []Fence{
		{OrgID: "org-a", BackendID: "backend-1", HostEpoch: "epoch-1"},
		{ProcessID: "proc-1", BackendID: "backend-1", HostEpoch: "epoch-1"},
		{ProcessID: "proc-1", OrgID: "org-a", HostEpoch: "epoch-1"},
		{ProcessID: "proc-1", OrgID: "org-a", BackendID: "backend-1"},
	}
	for _, f := range partial {
		database := &processDatabaseStub{}
		store := &Store{pool: database, limits: DefaultLimits()}
		if err := store.MarkStarted(context.Background(), f); err == nil {
			t.Fatalf("incomplete fence %+v unexpectedly accepted", f)
		}
		if _, err := store.AppendOutput(context.Background(), f, nil, 0); err == nil {
			t.Fatalf("incomplete fence %+v unexpectedly accepted by AppendOutput", f)
		}
		if len(database.queries) != 0 {
			t.Fatalf("no statement should run for a rejected fence, got %d", len(database.queries))
		}
	}
}

// TestMarkStartedIsFencedAndIdempotent: the UPDATE carries every fence
// column and accepts a row that is already RUNNING, so a retried start
// report is a no-op rather than an error.
func TestMarkStartedIsFencedAndIdempotent(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 1")}
	store := &Store{pool: database, limits: DefaultLimits()}

	if err := store.MarkStarted(context.Background(), testFence()); err != nil {
		t.Fatalf("MarkStarted: %v", err)
	}
	query := database.queries[0]
	if !strings.Contains(query, "WHERE id = $1 AND org_id = $2 AND backend_id = $3 AND host_epoch = $4") {
		t.Fatalf("update is not fenced: %s", query)
	}
	if !strings.Contains(query, "state IN (1, 2)") {
		t.Fatalf("update accepts a terminal row: %s", query)
	}
	if !strings.Contains(query, "COALESCE(started_at, now())") {
		t.Fatalf("a repeated start must not move started_at: %s", query)
	}
}

func TestMarkStartedReportsAnUnknownProcess(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 0")}
	store := &Store{pool: database, limits: DefaultLimits()}
	// With no row scripted, the classification lookup finds nothing.
	if err := store.MarkStarted(context.Background(), testFence()); !errors.Is(err, ErrProcessNotFound) {
		t.Fatalf("err = %v, want ErrProcessNotFound", err)
	}
}

func TestRequestSignalNeverDowngradesAnEscalation(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 1")}
	store := &Store{pool: database, limits: DefaultLimits()}

	if err := store.RequestSignal(context.Background(), testFence(), SignalKill); err != nil {
		t.Fatalf("RequestSignal: %v", err)
	}
	query := database.queries[0]
	if !strings.Contains(query, "GREATEST(signal_requested, $5)") {
		t.Fatalf("a later term request could downgrade a kill: %s", query)
	}
	if got := database.args[0][4]; got != int16(SignalKill) {
		t.Fatalf("bound signal = %v, want %v", got, int16(SignalKill))
	}
}

func TestRequestSignalRejectsAnUnknownSignal(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}
	for _, signal := range []Signal{SignalNone, Signal(7)} {
		if err := store.RequestSignal(context.Background(), testFence(), signal); err == nil {
			t.Fatalf("signal %d unexpectedly accepted", signal)
		}
	}
	if len(database.queries) != 0 {
		t.Fatalf("no statement should run for a rejected signal, got %d", len(database.queries))
	}
}

func TestMarkEndedRequiresATerminalStateAndAReason(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}

	if err := store.MarkEnded(context.Background(), testFence(), StateRunning, nil, EndExited, true); err == nil {
		t.Fatal("a live state unexpectedly accepted as an outcome")
	}
	if err := store.MarkEnded(context.Background(), testFence(), StateExited, nil, "", true); err == nil {
		t.Fatal("a terminal state without a reason unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no statement should run for a rejected call, got %d", len(database.queries))
	}
}

func TestAppendOutputValidatesEveryChunkBeforeWritingAny(t *testing.T) {
	t.Parallel()
	cases := map[string][]Chunk{
		"zero seq":       {{Seq: 0, Stream: StreamStdout}},
		"negative seq":   {{Seq: -1, Stream: StreamStdout}},
		"unknown stream": {{Seq: 1, Stream: Stream(9)}},
		// The second chunk is the bad one: nothing may be written before the
		// whole batch has been checked.
		"bad second chunk": {{Seq: 1, Stream: StreamStdout}, {Seq: 2, Stream: Stream(0)}},
	}
	for name, chunks := range cases {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			database := &processDatabaseStub{}
			store := &Store{pool: database, limits: DefaultLimits()}
			if _, err := store.AppendOutput(context.Background(), testFence(), chunks, 0); err == nil {
				t.Fatal("invalid chunk batch unexpectedly accepted")
			}
			if len(database.queries) != 0 {
				t.Fatalf("no statement should run for a rejected batch, got %d", len(database.queries))
			}
		})
	}
}

func TestAppendOutputRejectsNegativeStdinBytes(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}
	if _, err := store.AppendOutput(context.Background(), testFence(), nil, -1); err == nil {
		t.Fatal("negative stdin delta unexpectedly accepted")
	}
}

// TestAppendOutputFencesEveryInsertAndDeduplicatesReplays is the core
// concurrency contract of the output log, at the statement level: a
// superseded host's insert matches no row, and a replayed seq conflicts away
// instead of duplicating.
func TestAppendOutputFencesEveryInsertAndDeduplicatesReplays(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{
		execTag: pgconn.NewCommandTag("INSERT 0 1"),
		rowScans: []func(...any) error{
			scanValues(int64(3), int64(1), int64(24), int64(0)),
		},
	}
	store := &Store{pool: database, limits: DefaultLimits()}

	chunks := []Chunk{
		{Seq: 1, Stream: StreamStdout, Content: []byte("hello\n"), EndsWithNewline: true, CapturedAt: time.Unix(1, 0).UTC()},
		{Seq: 2, Stream: StreamStderr, Content: []byte("warn\n"), EndsWithNewline: true, CapturedAt: time.Unix(2, 0).UTC()},
	}
	result, err := store.AppendOutput(context.Background(), testFence(), chunks, 12)
	if err != nil {
		t.Fatalf("AppendOutput: %v", err)
	}
	if result.NextSeq != 3 {
		t.Fatalf("NextSeq = %d, want 3", result.NextSeq)
	}

	if len(database.queries) != 3 { // two inserts plus the bookkeeping update
		t.Fatalf("expected two inserts and one update, got %d statements", len(database.queries))
	}
	for _, insert := range database.queries[:2] {
		if !strings.Contains(insert, "FROM sandbox_processes p") ||
			!strings.Contains(insert, "p.backend_id = $8 AND p.host_epoch = $9") ||
			!strings.Contains(insert, "p.state IN (1, 2)") {
			t.Fatalf("insert is not fenced: %s", insert)
		}
		if !strings.Contains(insert, "ON CONFLICT (process_id, seq) DO NOTHING") {
			t.Fatalf("a replayed batch would duplicate: %s", insert)
		}
	}
	update := database.queries[2]
	if !strings.Contains(update, "GREATEST(next_seq, $5)") {
		t.Fatalf("an out-of-order batch could rewind next_seq: %s", update)
	}
	if !strings.Contains(update, "last_heartbeat_at = now()") {
		t.Fatalf("append must also be a heartbeat: %s", update)
	}
	// maxSeq+1, appended bytes, stdin delta.
	if got := database.args[2][4]; got != int64(3) {
		t.Fatalf("bound next_seq = %v, want 3", got)
	}
	if got := database.args[2][5]; got != int64(11) {
		t.Fatalf("bound appended bytes = %v, want 11", got)
	}
	if got := database.args[2][6]; got != int64(12) {
		t.Fatalf("bound stdin delta = %v, want 12", got)
	}
}

// TestAppendOutputWithNoChunksIsAPureHeartbeat: silence has to be
// distinguishable from a dead host, so an empty batch still refreshes
// last_heartbeat_at and writes nothing to the log.
func TestAppendOutputWithNoChunksIsAPureHeartbeat(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{rowScans: []func(...any) error{
		scanValues(int64(1), int64(1), int64(0), int64(0)),
	}}
	store := &Store{pool: database, limits: DefaultLimits()}

	if _, err := store.AppendOutput(context.Background(), testFence(), nil, 0); err != nil {
		t.Fatalf("AppendOutput: %v", err)
	}
	if len(database.queries) != 1 {
		t.Fatalf("an empty batch must write only the heartbeat, got %d statements", len(database.queries))
	}
	if !strings.Contains(database.queries[0], "last_heartbeat_at = now()") {
		t.Fatalf("heartbeat not refreshed: %s", database.queries[0])
	}
	// An empty batch must not rewind next_seq to 1.
	if got := database.args[0][4]; got != int64(1) {
		t.Fatalf("bound next_seq = %v, want 1 (GREATEST keeps the stored value)", got)
	}
}

func TestAppendOutputReportsAFencedWriteAsNotFoundOrFenced(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("INSERT 0 0")}
	store := &Store{pool: database, limits: DefaultLimits()}
	// No scripted rows: the bookkeeping update matches nothing and the
	// classification lookup finds no process.
	if _, err := store.AppendOutput(context.Background(), testFence(), nil, 0); !errors.Is(err, ErrProcessNotFound) {
		t.Fatalf("err = %v, want ErrProcessNotFound", err)
	}
}

func TestGetRequiresIdentifiers(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}
	if _, err := store.Get(context.Background(), "", "proc-1"); err == nil {
		t.Fatal("missing org_id unexpectedly accepted")
	}
	if _, err := store.Get(context.Background(), "org-a", ""); err == nil {
		t.Fatal("missing process_id unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no statement should run for a rejected call, got %d", len(database.queries))
	}
}

func TestGetIsScopedToTheOrganization(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}
	if _, err := store.Get(context.Background(), "org-a", "proc-1"); !errors.Is(err, ErrProcessNotFound) {
		t.Fatalf("err = %v, want ErrProcessNotFound", err)
	}
	if !strings.Contains(database.queries[0], "WHERE id = $1 AND org_id = $2") {
		t.Fatalf("get is not tenant-scoped: %s", database.queries[0])
	}
}

func TestListClampsItsLimitAndOverFetchesForHasMore(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{rows: &emptyRows{}}
	store := &Store{pool: database, limits: DefaultLimits()}

	if _, _, err := store.List(context.Background(), "org-a", "space-1", false, 100000, "", nil); err != nil {
		t.Fatalf("List: %v", err)
	}
	if got := database.args[0][4]; got != int64(DefaultLimits().MaxListLimit)+1 {
		t.Fatalf("bound limit = %v, want the clamp plus one over-fetch", got)
	}
	if !strings.Contains(database.queries[0], "ORDER BY id DESC") {
		t.Fatalf("list is not newest-first: %s", database.queries[0])
	}
	if !strings.Contains(database.queries[0], "($3 OR state IN (1, 2))") {
		t.Fatalf("list does not filter terminal rows by default: %s", database.queries[0])
	}
}

func TestListRequiresIdentifiers(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}
	if _, _, err := store.List(context.Background(), "", "space-1", false, 10, "", nil); err == nil {
		t.Fatal("missing org_id unexpectedly accepted")
	}
	if _, _, err := store.List(context.Background(), "org-a", "", false, 10, "", nil); err == nil {
		t.Fatal("missing space_id unexpectedly accepted")
	}
}

func TestReconcileAndSweepRequireTheirInputs(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{}
	store := &Store{pool: database, limits: DefaultLimits()}

	if _, err := store.Reconcile(context.Background(), "", "epoch-1"); err == nil {
		t.Fatal("missing backend_id unexpectedly accepted")
	}
	if _, err := store.Reconcile(context.Background(), "backend-1", ""); err == nil {
		t.Fatal("missing host_epoch unexpectedly accepted")
	}
	if _, err := store.SweepStale(context.Background(), 0); err == nil {
		t.Fatal("a zero staleness window unexpectedly accepted")
	}
	if _, err := store.KillForLease(context.Background(), "org-a", ""); err == nil {
		t.Fatal("missing lease_id unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no statement should run for a rejected call, got %d", len(database.queries))
	}
}

// TestReconcileOnlyTouchesOtherEpochs is the property that makes a boot-time
// reconcile safe to run before the new host registers anything: it must
// never mark this epoch's own rows lost.
func TestReconcileOnlyTouchesOtherEpochs(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 2")}
	store := &Store{pool: database, limits: DefaultLimits()}

	lost, err := store.Reconcile(context.Background(), "backend-1", "epoch-2")
	if err != nil {
		t.Fatalf("Reconcile: %v", err)
	}
	if lost != 2 {
		t.Fatalf("lost = %d, want 2", lost)
	}
	query := database.queries[0]
	if !strings.Contains(query, "backend_id = $1 AND host_epoch <> $2") {
		t.Fatalf("reconcile does not exclude this epoch: %s", query)
	}
	if !strings.Contains(query, "state IN (1, 2)") {
		t.Fatalf("reconcile would rewrite terminal rows: %s", query)
	}
	if got := database.args[0][4]; got != CleanupDone {
		t.Fatalf("bound cleanup_state = %v, want done (the children died with their host)", got)
	}
}

func TestKillForLeaseLeavesCleanupPending(t *testing.T) {
	t.Parallel()
	database := &processDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 1")}
	store := &Store{pool: database, limits: DefaultLimits()}

	if _, err := store.KillForLease(context.Background(), "org-a", "lease-1"); err != nil {
		t.Fatalf("KillForLease: %v", err)
	}
	// Releasing a lease proves nothing about the OS process, so this must
	// not claim the cleanup is done — only the host can say that.
	if strings.Contains(database.queries[0], "cleanup_state") {
		t.Fatalf("lease release must not assert cleanup: %s", database.queries[0])
	}
}

// emptyRows is a pgx.Rows over nothing.
type emptyRows struct{}

func (r *emptyRows) Close()                                       {}
func (r *emptyRows) Err() error                                   { return nil }
func (r *emptyRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (r *emptyRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (r *emptyRows) RawValues() [][]byte                          { return nil }
func (r *emptyRows) Conn() *pgx.Conn                              { return nil }
func (r *emptyRows) Next() bool                                   { return false }
func (r *emptyRows) Values() ([]any, error)                       { return nil, pgx.ErrNoRows }
func (r *emptyRows) Scan(...any) error                            { return pgx.ErrNoRows }
