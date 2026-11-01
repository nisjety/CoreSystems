package lease

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgtype"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// leaseDatabaseStub mirrors capability-core's scope_store_test.go's own
// scopeDatabaseStub: a hand-rolled leaseDatabase implementation recording
// every call's query/args, with a configurable Exec result and a
// configurable QueryRow result. Query is never called by lease.Store, so
// its stub implementation exists only for interface satisfaction.
type leaseDatabaseStub struct {
	execTag  pgconn.CommandTag
	execErr  error
	row      pgx.Row
	queries  []string // Exec calls only
	argsList [][]any  // Exec calls only

	queryRowCalls     int // QueryRow calls only
	lastQueryRowQuery string
}

func (d *leaseDatabaseStub) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	d.queries = append(d.queries, query)
	d.argsList = append(d.argsList, append([]any(nil), args...))
	return d.execTag, d.execErr
}

func (d *leaseDatabaseStub) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	return nil, errors.New("Query is not used by lease.Store")
}

func (d *leaseDatabaseStub) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	d.queryRowCalls++
	d.lastQueryRowQuery = query
	return d.row
}

// execCount is the number of Exec (mutation) calls only — distinct from
// QueryRow (lookup) calls, so a test can assert "the lookup ran but no
// mutation followed" precisely.
func (d *leaseDatabaseStub) execCount() int { return len(d.queries) }

func (d *leaseDatabaseStub) lastQuery() string {
	if len(d.queries) == 0 {
		return ""
	}
	return d.queries[len(d.queries)-1]
}

func (d *leaseDatabaseStub) lastArgs() []any {
	if len(d.argsList) == 0 {
		return nil
	}
	return d.argsList[len(d.argsList)-1]
}

// leaseRow is a fake pgx.Row: either fixed scan values (in the exact column
// order lookup/ReleaseScoped select) or an error (e.g. pgx.ErrNoRows).
type leaseRow struct {
	values []any
	err    error
}

func (r leaseRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	for i, d := range dest {
		switch target := d.(type) {
		case *string:
			*target = r.values[i].(string)
		case *int32:
			*target = r.values[i].(int32)
		case *int64:
			*target = r.values[i].(int64)
		case *time.Time:
			*target = r.values[i].(time.Time)
		case *bool:
			*target = r.values[i].(bool)
		case *pgtype.Timestamptz:
			*target = r.values[i].(pgtype.Timestamptz)
		default:
			return fmt.Errorf("leaseRow.Scan: unsupported dest type %T", d)
		}
	}
	return nil
}

func notFoundRow() leaseRow { return leaseRow{err: pgx.ErrNoRows} }

func rowFor(l *Lease) leaseRow {
	return leaseRow{values: []any{
		l.ID, l.ScopeID, l.ScopeType, l.OrgID, l.OwnerID, l.Endpoint,
		l.SpaceID, l.BackendID, l.ProcessesPermitted, l.AudienceRevision, int32(l.State), l.ExpiresAt, l.CreatedAt,
		int32(l.FailureCause), l.LastActivityAt, heartbeatValue(l.HeartbeatAt),
	}}
}

// heartbeatValue is the nullable column as pgx scans it: unset when the lease has no heartbeat.
func heartbeatValue(at *time.Time) pgtype.Timestamptz {
	if at == nil {
		return pgtype.Timestamptz{}
	}
	return pgtype.Timestamptz{Time: *at, Valid: true}
}

func newTestStore(row pgx.Row) (*Store, *leaseDatabaseStub) {
	database := &leaseDatabaseStub{execTag: pgconn.NewCommandTag("UPDATE 1"), row: row}
	return &Store{pool: database, nowFn: time.Now, randFn: fixedRand}, database
}

func fixedRand(buf []byte) (int, error) {
	for i := range buf {
		buf[i] = 0xAB
	}
	return len(buf), nil
}

func testLease(state mpv1.SandboxLifecycleState, spaceID, backendID string, expiresAt time.Time) *Lease {
	return &Lease{
		ID: "lease-1", ScopeID: "scope-1", ScopeType: "agent", OrgID: "org-a", OwnerID: "user-a",
		Endpoint: "sandbox://lease-1", SpaceID: spaceID, BackendID: backendID, State: state,
		ExpiresAt: expiresAt, CreatedAt: time.Now().Add(-time.Minute),
	}
}

func TestNewStoreRejectsNilPool(t *testing.T) {
	if _, err := NewStore(nil); err == nil {
		t.Fatal("expected error for nil pool")
	}
}

func TestCreateInsertsAScratchLease(t *testing.T) {
	store, database := newTestStore(nil)
	l, err := store.Create(context.Background(), "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a", SpaceGrant{}, time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if l.State != mpv1.SandboxLifecycleState_SCRATCH {
		t.Fatalf("State = %v, want SCRATCH", l.State)
	}
	if !strings.Contains(database.lastQuery(), "INSERT INTO leases") {
		t.Fatalf("query = %q, want an INSERT into leases", database.lastQuery())
	}
	args := database.lastArgs()
	if args[8] != false {
		t.Fatalf("processes_permitted arg = %v, want false", args[8])
	}
	// The audience ceiling sits between processes_permitted and state (0004),
	// and both come from the same verified decision — asserting the position
	// keeps a future column insert from silently shifting one of them into the
	// other's slot, which Postgres would accept for two adjacent values it can
	// coerce.
	if args[9] != int64(0) {
		t.Fatalf("recipient_audience_revision arg = %v, want 0 for a grant that carried none", args[9])
	}
	if args[10] != int32(mpv1.SandboxLifecycleState_SCRATCH) {
		t.Fatalf("state arg = %v, want SCRATCH", args[10])
	}
}

// TestCreatePersistsTheAudienceCeiling: the revision a Space decision was
// signed under is the last thing anyone can honestly record about who the
// work belongs to — a process registers later on a service token with no
// decision at all, and inherits this value through the lease. Losing it here
// would make every process in the Space readable by any member holding any
// valid decision, which is precisely the ceiling migration 0004 exists to
// impose.
func TestCreatePersistsTheAudienceCeiling(t *testing.T) {
	store, database := newTestStore(nil)
	l, err := store.Create(context.Background(), "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a",
		SpaceGrant{ProcessesPermitted: true, RecipientAudienceRevision: 7}, time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if l.AudienceRevision != 7 {
		t.Fatalf("returned lease audience revision = %d, want 7", l.AudienceRevision)
	}
	if got := database.lastArgs()[9]; got != int64(7) {
		t.Fatalf("bound recipient_audience_revision = %v, want 7", got)
	}
}

// TestCreatePersistsTheProcessesPermittedDecision: AcquireLease decides this
// once from the verified capability decision, and the lease row is the only
// place a later process RPC — arriving on a service token with no decision in
// hand — can read it back from.
func TestCreatePersistsTheProcessesPermittedDecision(t *testing.T) {
	store, database := newTestStore(nil)
	l, err := store.Create(context.Background(), "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a", SpaceGrant{ProcessesPermitted: true}, time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if !l.ProcessesPermitted {
		t.Fatal("returned lease lost the decision")
	}
	if !strings.Contains(database.lastQuery(), "processes_permitted") {
		t.Fatalf("insert does not write the column: %q", database.lastQuery())
	}
	if database.lastArgs()[8] != true {
		t.Fatalf("processes_permitted arg = %v, want true", database.lastArgs()[8])
	}
}

func TestGetScopedReportsNotFoundOnNoRows(t *testing.T) {
	store, _ := newTestStore(notFoundRow())
	if _, err := store.GetScoped(context.Background(), "lease-1", "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v, want ErrLeaseNotFound", err)
	}
}

func TestGetScopedChecksExpiryBeforeBackendMismatch(t *testing.T) {
	expired := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(-time.Minute))
	store, _ := newTestStore(rowFor(expired))
	// Caller asserts a DIFFERENT backend than the stored lease AND the
	// lease is expired: expiry must win, so a caller debugging an expiry
	// never sees a misleading backend-mismatch error instead.
	if _, err := store.GetScoped(context.Background(), expired.ID, expired.OrgID, expired.OwnerID, "backend-b"); !errors.Is(err, ErrLeaseExpired) {
		t.Fatalf("error = %v, want ErrLeaseExpired", err)
	}
}

func TestGetScopedRejectsBackendMismatch(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, _ := newTestStore(rowFor(active))
	if _, err := store.GetScoped(context.Background(), active.ID, active.OrgID, active.OwnerID, "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("error = %v, want ErrLeaseBackendMismatch", err)
	}
}

func TestGetScopedTreatsDestroyedAsNotFound(t *testing.T) {
	// The lookup SELECT filters out DESTROYED via "state <> $4" itself, so
	// this proves that filter is present: a stub configured to return a
	// DESTROYED row should never happen against the real query, but a
	// not-found row is exactly what a correct query produces for one.
	store, database := newTestStore(notFoundRow())
	if _, err := store.GetScoped(context.Background(), "lease-1", "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v, want ErrLeaseNotFound", err)
	}
	if !strings.Contains(database.lastQueryRowQuery, "state <> $4") {
		t.Fatalf("query = %q, expected it to exclude DESTROYED rows", database.lastQueryRowQuery)
	}
}

func TestGetAnyReportsNotFoundOnNoRows(t *testing.T) {
	store, _ := newTestStore(notFoundRow())
	if _, err := store.GetAny(context.Background(), "lease-1", "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v, want ErrLeaseNotFound", err)
	}
}

func TestGetAnyRejectsBackendMismatch(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, _ := newTestStore(rowFor(active))
	if _, err := store.GetAny(context.Background(), active.ID, active.OrgID, active.OwnerID, "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("error = %v, want ErrLeaseBackendMismatch", err)
	}
}

// TestGetAnyDoesNotExcludeDestroyedLeases is GetAny's whole reason to exist,
// distinct from GetScoped: PromoteWorkspace must still resolve a lease's
// Space after ReleaseLease has already marked it DESTROYED, since merging a
// run's overlay is deliberately never tied to the lease's own release.
// Proven here by asserting the underlying query does NOT filter on state at
// all -- the opposite of TestGetScopedTreatsDestroyedAsNotFound's own
// "state <> $4" assertion for GetScoped.
func TestGetAnyDoesNotExcludeDestroyedLeases(t *testing.T) {
	destroyed := testLease(mpv1.SandboxLifecycleState_DESTROYED, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(destroyed))
	l, err := store.GetAny(context.Background(), destroyed.ID, destroyed.OrgID, destroyed.OwnerID, destroyed.BackendID)
	if err != nil {
		t.Fatalf("GetAny on a DESTROYED lease: %v", err)
	}
	if l.State != mpv1.SandboxLifecycleState_DESTROYED {
		t.Fatalf("State = %v, want DESTROYED (unmodified)", l.State)
	}
	if strings.Contains(database.lastQueryRowQuery, "state <>") {
		t.Fatalf("query = %q, GetAny must not filter on state at all", database.lastQueryRowQuery)
	}
}

// TestGetAnyDoesNotRejectExpiredLeases mirrors the DESTROYED case: an
// expired lease's overlay data is still a valid, durable thing to resolve
// the Space for -- GetAny is a purely descriptive lookup, not a fresh
// operational grant like GetScoped.
func TestGetAnyDoesNotRejectExpiredLeases(t *testing.T) {
	expired := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(-time.Minute))
	store, _ := newTestStore(rowFor(expired))
	if _, err := store.GetAny(context.Background(), expired.ID, expired.OrgID, expired.OwnerID, expired.BackendID); err != nil {
		t.Fatalf("GetAny on an expired lease: %v", err)
	}
}

func TestActivatePromotesScratchToActive(t *testing.T) {
	scratch := testLease(mpv1.SandboxLifecycleState_SCRATCH, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(scratch))
	l, err := store.Activate(context.Background(), scratch.ID, scratch.OrgID, scratch.OwnerID, scratch.BackendID)
	if err != nil {
		t.Fatal(err)
	}
	if l.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", l.State)
	}
	if !strings.Contains(database.lastQuery(), "UPDATE leases SET state") {
		t.Fatalf("query = %q, expected an UPDATE", database.lastQuery())
	}
}

func TestActivateIsANoOpWhenAlreadyActive(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(active))
	l, err := store.Activate(context.Background(), active.ID, active.OrgID, active.OwnerID, active.BackendID)
	if err != nil {
		t.Fatal(err)
	}
	if l.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", l.State)
	}
	// Only the lookup SELECT should have run — no UPDATE for a no-op.
	if database.execCount() != 0 {
		t.Fatalf("exec count = %d, want 0 (lookup only, no UPDATE)", database.execCount())
	}
}

func TestBeginSnapshotRejectsAScratchSpaceScopedLease(t *testing.T) {
	scratch := testLease(mpv1.SandboxLifecycleState_SCRATCH, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, _ := newTestStore(rowFor(scratch))
	if _, err := store.BeginSnapshot(context.Background(), scratch.ID, scratch.OrgID, scratch.OwnerID, scratch.BackendID); !errors.Is(err, ErrLeaseNotActivated) {
		t.Fatalf("error = %v, want ErrLeaseNotActivated", err)
	}
}

func TestBeginSnapshotAllowsAScratchNonSpaceLease(t *testing.T) {
	nonSpace := testLease(mpv1.SandboxLifecycleState_SCRATCH, "", "", time.Now().Add(time.Minute))
	store, _ := newTestStore(rowFor(nonSpace))
	if _, err := store.BeginSnapshot(context.Background(), nonSpace.ID, nonSpace.OrgID, nonSpace.OwnerID, ""); err != nil {
		t.Fatalf("non-Space lease snapshot should never require activation: %v", err)
	}
}

func TestBeginSnapshotMarksAnActiveSpaceLeaseSnapshotting(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(active))
	l, err := store.BeginSnapshot(context.Background(), active.ID, active.OrgID, active.OwnerID, active.BackendID)
	if err != nil {
		t.Fatal(err)
	}
	if l.State != mpv1.SandboxLifecycleState_SNAPSHOTTING {
		t.Fatalf("State = %v, want SNAPSHOTTING", l.State)
	}
	if !strings.Contains(database.lastQuery(), "UPDATE leases SET state") {
		t.Fatalf("query = %q, expected an UPDATE", database.lastQuery())
	}
}

func TestEndSnapshotIssuesAConditionalUpdate(t *testing.T) {
	store, database := newTestStore(nil)
	store.EndSnapshot(context.Background(), "lease-1")
	if !strings.Contains(database.lastQuery(), "UPDATE leases SET state") || !strings.Contains(database.lastQuery(), "space_id <> ''") {
		t.Fatalf("query = %q, expected a Space-scoped conditional UPDATE", database.lastQuery())
	}
}

func TestReleaseScopedReportsNotFoundOnNoRows(t *testing.T) {
	store, database := newTestStore(notFoundRow())
	if _, err := store.ReleaseScoped(context.Background(), "lease-1", "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v, want ErrLeaseNotFound", err)
	}
	// Not-found must not attempt the DESTROYED update.
	if database.execCount() != 0 {
		t.Fatalf("exec count = %d, want 0", database.execCount())
	}
}

func TestReleaseScopedRejectsBackendMismatchWithoutMutating(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(active))
	if _, err := store.ReleaseScoped(context.Background(), active.ID, active.OrgID, active.OwnerID, "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("error = %v, want ErrLeaseBackendMismatch", err)
	}
	if database.execCount() != 0 {
		t.Fatalf("exec count = %d, want 0 (no DESTROYED update on mismatch)", database.execCount())
	}
}

func TestReleaseScopedMarksDestroyedOnMatch(t *testing.T) {
	active := testLease(mpv1.SandboxLifecycleState_ACTIVE, "space-a", "backend-a", time.Now().Add(time.Minute))
	store, database := newTestStore(rowFor(active))
	ok, err := store.ReleaseScoped(context.Background(), active.ID, active.OrgID, active.OwnerID, active.BackendID)
	if err != nil || !ok {
		t.Fatalf("ok = %v, err = %v", ok, err)
	}
	if !strings.Contains(database.lastQuery(), "UPDATE leases SET state") {
		t.Fatalf("query = %q, expected the DESTROYED UPDATE", database.lastQuery())
	}
	args := database.lastArgs()
	if args[1] != int32(mpv1.SandboxLifecycleState_DESTROYED) {
		t.Fatalf("state arg = %v, want DESTROYED", args[1])
	}
}

func TestAcquireThreadStampsActivityAndReadsTheLifecycleColumns(t *testing.T) {
	want := testLease(mpv1.SandboxLifecycleState_SCRATCH, "", "backend-1", time.Now().Add(time.Hour))
	want.ScopeType = "thread"
	want.ScopeID = "thread-1"
	want.OrgID = "org-1"
	want.OwnerID = "user-1"
	want.LastActivityAt = time.Now()
	s, db := newTestStore(rowFor(want))
	if _, err := s.AcquireThread(context.Background(), "thread-1", "org-1", "user-1", "backend-1", time.Minute); err != nil {
		t.Fatalf("AcquireThread: %v", err)
	}
	for _, must := range []string{
		"last_activity_at = EXCLUDED.last_activity_at",
		"RETURNING id, scope_id, scope_type, org_id, owner_id, endpoint, space_id, backend_id, processes_permitted, recipient_audience_revision, state, expires_at, created_at, failure_cause, last_activity_at, heartbeat_at",
	} {
		if !strings.Contains(db.lastQueryRowQuery, must) {
			t.Fatalf("upsert is missing %q:\n%s", must, db.lastQueryRowQuery)
		}
	}
}

func TestScanLeaseCarriesTheFailureCauseActivityAndHeartbeat(t *testing.T) {
	at := time.Date(2026, 11, 1, 9, 0, 0, 0, time.UTC)
	want := testLease(mpv1.SandboxLifecycleState_ACTIVE, "", "backend-1", time.Now().Add(time.Hour))
	want.FailureCause = mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_WORKSPACE_CONFLICT
	want.LastActivityAt = at
	s, _ := newTestStore(rowFor(want))
	got, err := s.GetAny(context.Background(), want.ID, want.OrgID, want.OwnerID, want.BackendID)
	if err != nil {
		t.Fatalf("GetAny: %v", err)
	}
	if got.FailureCause != want.FailureCause || !got.LastActivityAt.Equal(at) {
		t.Fatalf("lifecycle = (%v, %v), want (%v, %v)", got.FailureCause, got.LastActivityAt, want.FailureCause, at)
	}
	if got.HeartbeatAt != nil {
		t.Fatalf("heartbeat = %v, want unset until WS22-S8 writes one", got.HeartbeatAt)
	}
	heartbeat := at.Add(time.Second)
	want.HeartbeatAt = &heartbeat
	s, _ = newTestStore(rowFor(want))
	got, err = s.GetAny(context.Background(), want.ID, want.OrgID, want.OwnerID, want.BackendID)
	if err != nil {
		t.Fatalf("GetAny with a heartbeat: %v", err)
	}
	if got.HeartbeatAt == nil || !got.HeartbeatAt.Equal(heartbeat) {
		t.Fatalf("heartbeat = %v, want %v", got.HeartbeatAt, heartbeat)
	}
}

func TestRecordActivityWritesTheCauseAndActivityOnlyWithinTheOwner(t *testing.T) {
	store, database := newTestStore(nil)
	at := time.Date(2026, 11, 1, 9, 30, 0, 0, time.UTC)
	if err := store.RecordActivity(context.Background(), "lease-1", "org-a", "user-a", mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_QUOTA_EXCEEDED, at); err != nil {
		t.Fatalf("RecordActivity: %v", err)
	}
	if database.execCount() != 1 || !strings.Contains(database.lastQuery(), "UPDATE leases SET failure_cause = $4, last_activity_at = $5") {
		t.Fatalf("query = %q, want one UPDATE of failure_cause and last_activity_at", database.lastQuery())
	}
	if !strings.Contains(database.lastQuery(), "WHERE id = $1 AND org_id = $2 AND owner_id = $3") {
		t.Fatalf("query = %q, want the write bound to the org and the owner", database.lastQuery())
	}
	args := database.lastArgs()
	if args[0] != "lease-1" || args[1] != "org-a" || args[2] != "user-a" ||
		args[3] != int32(mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_QUOTA_EXCEEDED) || !args[4].(time.Time).Equal(at) {
		t.Fatalf("args = %v", args)
	}
}

