//go:build integration

package process

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// setupProcessDB spins a throwaway Postgres and applies the migrations this
// store needs, mirroring internal/lease's own setupLeaseDB. 0002 creates the
// leases table that 0003 alters and that Register joins against.
func setupProcessDB(t *testing.T) string {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("sandbox_manager"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("conn string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	for _, migration := range []string{
		"0002_lease_and_snapshot_store.up.sql",
		"0003_process_registry.up.sql",
		"0004_audience_revision_ceiling.up.sql",
	} {
		sqlBytes, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if readErr != nil {
			t.Fatalf("read migration %s: %v", migration, readErr)
		}
		if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply migration %s: %v", migration, applyErr)
		}
	}
	pool.Close()
	return dsn
}

func newPoolStore(t *testing.T, dsn string) *Store {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	return store
}

type seededLease struct {
	id        string
	orgID     string
	spaceID   string
	backendID string
	permitted bool
	state     mpv1.SandboxLifecycleState
	expiresIn time.Duration
}

func defaultLease() seededLease {
	return seededLease{
		id:        "lease-1",
		orgID:     "org-a",
		spaceID:   "space-1",
		backendID: "backend-1",
		permitted: true,
		state:     mpv1.SandboxLifecycleState_ACTIVE,
		expiresIn: time.Hour,
	}
}

// seedLease writes a leases row directly. AcquireLease does not set
// processes_permitted until S4.2 step 2, so this test file writes the column
// itself rather than pretending the lease path already does.
func seedLease(t *testing.T, dsn string, l seededLease) {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	defer pool.Close()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO leases (id, scope_id, scope_type, org_id, owner_id, endpoint,
		                    space_id, backend_id, state, expires_at, created_at, processes_permitted)
		VALUES ($1, 'scope-1', 'agent', $2, 'user-a', 'sandbox://' || $1,
		        $3, $4, $5, now() + $6::interval, now(), $7)
	`, l.id, l.orgID, l.spaceID, l.backendID, int32(l.state),
		fmt.Sprintf("%d seconds", int(l.expiresIn.Seconds())), l.permitted); err != nil {
		t.Fatalf("seed lease: %v", err)
	}
}

func registerRequest(id string) RegisterRequest {
	return RegisterRequest{
		ID:            id,
		OrgID:         "org-a",
		LeaseID:       "lease-1",
		BackendID:     "backend-1",
		HostEpoch:     "epoch-1",
		RunID:         "run-1",
		StepID:        "step-1",
		SubjectID:     "user-a",
		Command:       Command{Program: "python3", Args: []string{"worker.py", "--token", redactedMarker}},
		CommandDigest: "sha256:abc",
		TTLSeconds:    900,
	}
}

// redactedMarker mirrors internal/redact's marker without importing it: the
// command reaching this store is already redacted, and the point of these
// tests is that the registry never receives a plaintext secret at all.
const redactedMarker = "[REDACTED]"

func fenceFor(id string) Fence {
	return Fence{ProcessID: id, OrgID: "org-a", BackendID: "backend-1", HostEpoch: "epoch-1"}
}

func mustRegister(t *testing.T, store *Store, id string) *Process {
	t.Helper()
	p, err := store.Register(context.Background(), registerRequest(id))
	if err != nil {
		t.Fatalf("Register(%s): %v", id, err)
	}
	if err := store.MarkStarted(context.Background(), fenceFor(id)); err != nil {
		t.Fatalf("MarkStarted(%s): %v", id, err)
	}
	return p
}

func chunk(seq int64, text string) Chunk {
	return Chunk{
		Seq:             seq,
		Stream:          StreamStdout,
		Content:         []byte(text),
		EndsWithNewline: true,
		CapturedAt:      time.Now().UTC(),
	}
}

// TestProcessStore_RegisterIsFailClosedOnTheLeaseGate is the authority
// property the whole slice rests on: a lease without the space:processes
// permission Control grants cannot host a background process, and the
// column defaults to false so every lease that already exists reads as not
// permitted rather than being grandfathered in.
func TestProcessStore_RegisterIsFailClosedOnTheLeaseGate(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)

	lease := defaultLease()
	lease.permitted = false
	seedLease(t, dsn, lease)

	_, err := store.Register(context.Background(), registerRequest("proc-001"))
	if err == nil {
		t.Fatal("a lease without space:processes unexpectedly hosted a process")
	}
	if got := err.Error(); got == "" {
		t.Fatal("refusal carried no explanation")
	}
}

func TestProcessStore_RegisterRefusesAScratchOrWrongBackendLease(t *testing.T) {
	for _, tc := range []struct {
		name  string
		lease func(seededLease) seededLease
	}{
		{"scratch lease", func(l seededLease) seededLease {
			l.state = mpv1.SandboxLifecycleState_SCRATCH
			return l
		}},
		{"another backend", func(l seededLease) seededLease {
			l.backendID = "backend-2"
			return l
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dsn := setupProcessDB(t)
			store := newPoolStore(t, dsn)
			seedLease(t, dsn, tc.lease(defaultLease()))

			if _, err := store.Register(context.Background(), registerRequest("proc-001")); err == nil {
				t.Fatal("ineligible lease unexpectedly hosted a process")
			}
		})
	}
}

// TestProcessStore_RegisterRefusesANonSpaceLease: a thread/agent-scoped
// lease has no Space workspace, so there is nowhere to run a background
// process even in principle. RegisterRequest carries no Space of its own —
// it is read from the lease inside the insert — so this is the only place
// the case can be refused.
func TestProcessStore_RegisterRefusesANonSpaceLease(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)

	lease := defaultLease()
	lease.spaceID = ""
	seedLease(t, dsn, lease)

	if _, err := store.Register(context.Background(), registerRequest("proc-001")); err == nil {
		t.Fatal("a non-Space lease unexpectedly hosted a process")
	}
}

// TestProcessStore_RegisterEnforcesTheLiveLimitAndFreesItOnExit proves the
// limit is evaluated against live rows only — a finished process must not
// permanently consume a slot.
func TestProcessStore_RegisterEnforcesTheLiveLimitAndFreesItOnExit(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn).WithLimits(Limits{MaxLivePerLease: 2})
	seedLease(t, dsn, defaultLease())

	mustRegister(t, store, "proc-001")
	mustRegister(t, store, "proc-002")

	if _, err := store.Register(context.Background(), registerRequest("proc-003")); err == nil {
		t.Fatal("third process unexpectedly admitted past the limit of two")
	}

	exitCode := int32(0)
	if err := store.MarkEnded(context.Background(), fenceFor("proc-001"), StateExited, &exitCode, EndExited, true); err != nil {
		t.Fatalf("MarkEnded: %v", err)
	}
	if _, err := store.Register(context.Background(), registerRequest("proc-003")); err != nil {
		t.Fatalf("a finished process must free its slot: %v", err)
	}
}

// TestProcessStore_RegisterClampsExpiryToTheLease: a process must never
// outlive the lease whose workspace and authority it runs under.
func TestProcessStore_RegisterClampsExpiryToTheLease(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)

	lease := defaultLease()
	lease.expiresIn = 60 * time.Second
	seedLease(t, dsn, lease)

	// Ask for far longer than the lease has left.
	req := registerRequest("proc-001")
	req.TTLSeconds = 86_400
	p, err := store.Register(context.Background(), req)
	if err != nil {
		t.Fatalf("Register: %v", err)
	}
	if time.Until(p.ExpiresAt) > 2*time.Minute {
		t.Fatalf("process expiry %s outlives the lease's own 60s", p.ExpiresAt)
	}
	if p.State != StateStarting {
		t.Fatalf("a fresh registration must be STARTING, got %d", p.State)
	}
	if p.TTLSeconds != 86_400 {
		t.Fatalf("the requested TTL should still be recorded, got %d", p.TTLSeconds)
	}
}

// TestProcessStore_CursorResumeReturnsOnlyWhatFollowsTheCursor is the plan's
// "cursor resume" verification. The last case is the one that matters most:
// a fully acknowledged terminal stream is still a known stream, never a
// not-found.
func TestProcessStore_CursorResumeReturnsOnlyWhatFollowsTheCursor(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	if _, err := store.AppendOutput(ctx, fenceFor("proc-001"), []Chunk{
		chunk(1, "a\n"), chunk(2, "b\n"), chunk(3, "c\n"),
	}, 0); err != nil {
		t.Fatalf("AppendOutput: %v", err)
	}

	all, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(all.Chunks) != 3 || all.NextCursor != 3 {
		t.Fatalf("full read = %d chunks, cursor %d", len(all.Chunks), all.NextCursor)
	}
	if all.GapBefore {
		t.Fatal("a complete stream must not report a gap")
	}

	after2, err := store.ReadOutput(ctx, "org-a", "proc-001", 2, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(after2.Chunks) != 1 || after2.Chunks[0].Seq != 3 {
		t.Fatalf("resume after 2 = %+v", after2.Chunks)
	}
	if string(after2.Chunks[0].Content) != "c\n" || !after2.Chunks[0].EndsWithNewline {
		t.Fatalf("chunk content/newline flag not round-tripped: %+v", after2.Chunks[0])
	}

	exitCode := int32(0)
	if err := store.MarkEnded(ctx, fenceFor("proc-001"), StateExited, &exitCode, EndExited, true); err != nil {
		t.Fatalf("MarkEnded: %v", err)
	}
	drained, err := store.ReadOutput(ctx, "org-a", "proc-001", 3, 0, nil)
	if err != nil {
		t.Fatalf("a fully acknowledged terminal stream must still read: %v", err)
	}
	if len(drained.Chunks) != 0 {
		t.Fatalf("expected an empty page, got %d chunks", len(drained.Chunks))
	}
	if drained.State != StateExited || drained.EndReason != EndExited {
		t.Fatalf("empty page lost the terminal outcome: state %d reason %q", drained.State, drained.EndReason)
	}
	if drained.ExitCode == nil || *drained.ExitCode != 0 {
		t.Fatalf("empty page lost the exit code: %v", drained.ExitCode)
	}
}

// TestProcessStore_AReplayedBatchIsANoOp is what lets the host retry after
// an ambiguous transport failure without a batch-id table: the same seqs
// conflict away instead of duplicating.
func TestProcessStore_AReplayedBatchIsANoOp(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	batch := []Chunk{chunk(1, "a\n"), chunk(2, "b\n")}
	first, err := store.AppendOutput(ctx, fenceFor("proc-001"), batch, 0)
	if err != nil {
		t.Fatalf("AppendOutput: %v", err)
	}
	second, err := store.AppendOutput(ctx, fenceFor("proc-001"), batch, 0)
	if err != nil {
		t.Fatalf("replayed AppendOutput: %v", err)
	}
	if second.NextSeq != first.NextSeq {
		t.Fatalf("a replay moved next_seq: %d -> %d", first.NextSeq, second.NextSeq)
	}

	page, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(page.Chunks) != 2 {
		t.Fatalf("replay duplicated output: %d chunks", len(page.Chunks))
	}
}

// TestProcessStore_ASupersededHostCannotWrite is the fence, end to end: a
// host that has been reconciled away lands nothing, and is told so.
func TestProcessStore_ASupersededHostCannotWrite(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	stale := Fence{ProcessID: "proc-001", OrgID: "org-a", BackendID: "backend-1", HostEpoch: "epoch-0"}

	if _, err := store.AppendOutput(ctx, stale, []Chunk{chunk(1, "stale\n")}, 0); err == nil {
		t.Fatal("a superseded host was allowed to append")
	}
	if err := store.MarkStarted(ctx, stale); err == nil {
		t.Fatal("a superseded host was allowed to change state")
	}
	if err := store.RequestSignal(ctx, stale, SignalKill); err == nil {
		t.Fatal("a superseded host was allowed to request a signal")
	}

	page, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(page.Chunks) != 0 {
		t.Fatalf("a fenced write still landed: %d chunks", len(page.Chunks))
	}

	// Another organization cannot reach the row at all.
	crossTenant := Fence{ProcessID: "proc-001", OrgID: "org-b", BackendID: "backend-1", HostEpoch: "epoch-1"}
	if _, err := store.AppendOutput(ctx, crossTenant, []Chunk{chunk(1, "x\n")}, 0); err == nil {
		t.Fatal("another organization was allowed to append")
	}
	if _, err := store.Get(ctx, "org-b", "proc-001"); err != ErrProcessNotFound {
		t.Fatalf("cross-tenant Get = %v, want ErrProcessNotFound", err)
	}
}

// TestProcessStore_ReconcileMarksOnlyTheSupersededEpochLost is the worker
// restart property: a new boot declares its predecessor's children lost
// (they died with it) without touching its own rows or another backend's.
func TestProcessStore_ReconcileMarksOnlyTheSupersededEpochLost(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()

	// A process on a different backend entirely, which must be left alone.
	otherLease := defaultLease()
	otherLease.id = "lease-2"
	otherLease.backendID = "backend-2"
	seedLease(t, dsn, otherLease)
	otherReq := registerRequest("proc-002")
	otherReq.LeaseID = "lease-2"
	otherReq.BackendID = "backend-2"
	otherReq.HostEpoch = "epoch-other"
	if _, err := store.Register(ctx, otherReq); err != nil {
		t.Fatalf("Register on the other backend: %v", err)
	}

	lost, err := store.Reconcile(ctx, "backend-1", "epoch-2")
	if err != nil {
		t.Fatalf("Reconcile: %v", err)
	}
	if lost != 1 {
		t.Fatalf("reconciled %d rows, want exactly the superseded one", lost)
	}

	superseded, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if superseded.State != StateLost || superseded.EndReason != EndHostLost {
		t.Fatalf("superseded process = state %d reason %q", superseded.State, superseded.EndReason)
	}
	if superseded.EndedAt == nil {
		t.Fatal("a terminal row must carry ended_at (the migration's CHECK should have refused otherwise)")
	}
	if superseded.CleanupState != CleanupDone {
		t.Fatal("a child that died with its host has nothing left to clean up")
	}

	untouched, err := store.Get(ctx, "org-a", "proc-002")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if untouched.State.IsTerminal() {
		t.Fatalf("another backend's process was reconciled away: state %d", untouched.State)
	}

	// Reconciling again is a no-op: the rows are already terminal.
	again, err := store.Reconcile(ctx, "backend-1", "epoch-2")
	if err != nil {
		t.Fatalf("Reconcile: %v", err)
	}
	if again != 0 {
		t.Fatalf("a second reconcile touched %d rows, want 0", again)
	}
}

// TestProcessStore_SurvivesAFreshPoolAgainstTheSameDatabase is the
// registry's own restart proof, the direct counterpart of
// internal/lease's TestLeaseStore_SurvivesAFreshPoolAgainstTheSameDatabase.
func TestProcessStore_SurvivesAFreshPoolAgainstTheSameDatabase(t *testing.T) {
	dsn := setupProcessDB(t)
	before := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, before, "proc-001")
	if _, err := before.AppendOutput(context.Background(), fenceFor("proc-001"), []Chunk{chunk(1, "hello\n")}, 0); err != nil {
		t.Fatalf("AppendOutput: %v", err)
	}

	after := newPoolStore(t, dsn)
	page, err := after.ReadOutput(context.Background(), "org-a", "proc-001", 0, 0)
	if err != nil {
		t.Fatalf("ReadOutput from a fresh pool: %v", err)
	}
	if len(page.Chunks) != 1 || string(page.Chunks[0].Content) != "hello\n" {
		t.Fatalf("output did not survive: %+v", page.Chunks)
	}
	if page.State != StateRunning {
		t.Fatalf("state did not survive: %d", page.State)
	}
}

// TestProcessStore_SweepStaleMarksSilentHostsLost covers the backstop for a
// host that vanished without reconciling, and proves a heartbeating process
// is never touched.
func TestProcessStore_SweepStaleMarksSilentHostsLost(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")
	mustRegister(t, store, "proc-002")

	ctx := context.Background()
	// Age one process's heartbeat past the window without touching the other.
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	defer pool.Close()
	if _, err := pool.Exec(ctx, `
		UPDATE sandbox_processes SET last_heartbeat_at = now() - interval '10 minutes' WHERE id = $1
	`, "proc-001"); err != nil {
		t.Fatalf("age heartbeat: %v", err)
	}

	lost, err := store.SweepStale(ctx, time.Minute)
	if err != nil {
		t.Fatalf("SweepStale: %v", err)
	}
	if lost != 1 {
		t.Fatalf("swept %d rows, want exactly the silent one", lost)
	}

	silent, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if silent.State != StateLost || silent.EndReason != EndHostLost {
		t.Fatalf("silent process = state %d reason %q", silent.State, silent.EndReason)
	}

	alive, err := store.Get(ctx, "org-a", "proc-002")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if alive.State != StateRunning {
		t.Fatalf("a heartbeating process was swept: state %d", alive.State)
	}

	// An append refreshes the heartbeat, so a busy process is never swept.
	if _, err := store.AppendOutput(ctx, fenceFor("proc-002"), nil, 0); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	again, err := store.SweepStale(ctx, time.Minute)
	if err != nil {
		t.Fatalf("SweepStale: %v", err)
	}
	if again != 0 {
		t.Fatalf("second sweep touched %d rows, want 0", again)
	}
}

// TestProcessStore_RetentionKeepsHeadAndTailAndReportsTheHole is the
// retention contract. The gap assertion is the one that matters: because the
// head is protected, a trim leaves a hole in the MIDDLE, and a reader
// resuming from inside the surviving head must be told that what follows is
// not contiguous.
func TestProcessStore_RetentionKeepsHeadAndTailAndReportsTheHole(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn).WithLimits(Limits{OutputHeadBytes: 10, OutputRetainBytes: 20})
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	// Ten 10-byte chunks against a 10-byte head and 20-byte tail.
	for seq := int64(1); seq <= 10; seq++ {
		if _, err := store.AppendOutput(ctx, fenceFor("proc-001"),
			[]Chunk{chunk(seq, fmt.Sprintf("line-%04d", seq))}, 0); err != nil {
			t.Fatalf("AppendOutput %d: %v", seq, err)
		}
	}

	p, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if p.DroppedBytes == 0 {
		t.Fatal("nothing was trimmed despite exceeding the window")
	}
	if p.RetainedBytes > 40 {
		t.Fatalf("retained %d bytes, well past head+tail", p.RetainedBytes)
	}

	page, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(page.Chunks) == 0 {
		t.Fatal("retention deleted everything")
	}
	if page.Chunks[0].Seq != 1 {
		t.Fatalf("the protected head was trimmed: first seq %d", page.Chunks[0].Seq)
	}
	last := page.Chunks[len(page.Chunks)-1]
	if last.Seq != 10 {
		t.Fatalf("the newest chunk was trimmed: last seq %d", last.Seq)
	}

	// There is a hole between the head and the tail, and a reader resuming
	// from inside the head must be told.
	resumed, err := store.ReadOutput(ctx, "org-a", "proc-001", 1, 0, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(resumed.Chunks) == 0 {
		t.Fatal("nothing left to resume into")
	}
	if resumed.Chunks[0].Seq == 2 {
		t.Skip("window was large enough that nothing between the head and tail was dropped")
	}
	if !resumed.GapBefore {
		t.Fatalf("a discontinuous page did not report a gap: first seq %d after cursor 1", resumed.Chunks[0].Seq)
	}
}

// TestProcessStore_ReadOutputPagesByByteBudget proves a reader can never be
// wedged: an oversized single chunk is still returned rather than looping
// forever on an empty page.
func TestProcessStore_ReadOutputPagesByByteBudget(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	for seq := int64(1); seq <= 4; seq++ {
		if _, err := store.AppendOutput(ctx, fenceFor("proc-001"),
			[]Chunk{chunk(seq, "0123456789")}, 0); err != nil {
			t.Fatalf("AppendOutput: %v", err)
		}
	}

	page, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 15, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(page.Chunks) != 1 || !page.HasMore {
		t.Fatalf("byte budget not applied: %d chunks, hasMore=%v", len(page.Chunks), page.HasMore)
	}

	tiny, err := store.ReadOutput(ctx, "org-a", "proc-001", 0, 1, nil)
	if err != nil {
		t.Fatalf("ReadOutput: %v", err)
	}
	if len(tiny.Chunks) != 1 {
		t.Fatalf("a chunk larger than the whole budget must still be returned, got %d", len(tiny.Chunks))
	}
	if tiny.NextCursor != 1 {
		t.Fatalf("cursor did not advance past the oversized chunk: %d", tiny.NextCursor)
	}
}

// TestProcessStore_ConcurrentReadersAgreeOnOneStream is the plan's
// "concurrent readers" verification: many readers paging the same process
// must each see a strictly increasing, contiguous sequence and assemble the
// identical stream.
func TestProcessStore_ConcurrentReadersAgreeOnOneStream(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	const chunks = 50
	for seq := int64(1); seq <= chunks; seq++ {
		if _, err := store.AppendOutput(ctx, fenceFor("proc-001"),
			[]Chunk{chunk(seq, fmt.Sprintf("line-%04d\n", seq))}, 0); err != nil {
			t.Fatalf("AppendOutput: %v", err)
		}
	}

	const readers = 20
	var wg sync.WaitGroup
	assembled := make([]string, readers)
	errs := make([]error, readers)
	for i := 0; i < readers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			cursor := int64(0)
			var out []byte
			for {
				// A small budget forces several pages per reader.
				page, err := store.ReadOutput(context.Background(), "org-a", "proc-001", cursor, 32)
				if err != nil {
					errs[i] = err
					return
				}
				for _, c := range page.Chunks {
					if c.Seq != cursor+1 {
						errs[i] = fmt.Errorf("non-contiguous seq %d after cursor %d", c.Seq, cursor)
						return
					}
					cursor = c.Seq
					out = append(out, c.Content...)
				}
				if len(page.Chunks) == 0 {
					break
				}
			}
			assembled[i] = string(out)
		}(i)
	}
	wg.Wait()

	var want string
	for seq := int64(1); seq <= chunks; seq++ {
		want += fmt.Sprintf("line-%04d\n", seq)
	}
	for i := 0; i < readers; i++ {
		if errs[i] != nil {
			t.Fatalf("reader %d: %v", i, errs[i])
		}
		if assembled[i] != want {
			t.Fatalf("reader %d assembled %d bytes, want %d", i, len(assembled[i]), len(want))
		}
	}
}

// TestProcessStore_TheFirstTerminalOutcomeWins: a host reporting an exit it
// observed just after the sweeper declared the process LOST must not
// overwrite the registry's already-truthful answer, and must not fail.
func TestProcessStore_TheFirstTerminalOutcomeWins(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	exitCode := int32(0)
	if err := store.MarkEnded(ctx, fenceFor("proc-001"), StateKilled, nil, EndSignaled, true); err != nil {
		t.Fatalf("MarkEnded: %v", err)
	}
	if err := store.MarkEnded(ctx, fenceFor("proc-001"), StateExited, &exitCode, EndExited, true); err != nil {
		t.Fatalf("a late outcome must be an idempotent no-op, got: %v", err)
	}

	p, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if p.State != StateKilled || p.EndReason != EndSignaled {
		t.Fatalf("the first outcome was overwritten: state %d reason %q", p.State, p.EndReason)
	}
}

func TestProcessStore_SignalEscalationNeverDowngrades(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")

	ctx := context.Background()
	if err := store.RequestSignal(ctx, fenceFor("proc-001"), SignalTerm); err != nil {
		t.Fatalf("RequestSignal(term): %v", err)
	}
	p, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if p.TermRequestedAt == nil {
		t.Fatal("term_requested_at was not stamped; the grace timer has no start")
	}
	firstTerm := *p.TermRequestedAt

	if err := store.RequestSignal(ctx, fenceFor("proc-001"), SignalKill); err != nil {
		t.Fatalf("RequestSignal(kill): %v", err)
	}
	// A reordered or repeated term must not walk the escalation back.
	if err := store.RequestSignal(ctx, fenceFor("proc-001"), SignalTerm); err != nil {
		t.Fatalf("RequestSignal(term again): %v", err)
	}
	p, err = store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if p.SignalRequested != SignalKill {
		t.Fatalf("escalation was downgraded: %d", p.SignalRequested)
	}
	if !p.TermRequestedAt.Equal(firstTerm) {
		t.Fatal("the grace window restarted on a repeated term request")
	}
}

// TestProcessStore_KillForLeaseTerminatesLiveProcessesOnly covers the
// registry half of lease release, including that it leaves cleanup pending:
// releasing a lease proves nothing about the OS process.
func TestProcessStore_KillForLeaseTerminatesLiveProcessesOnly(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())
	mustRegister(t, store, "proc-001")
	mustRegister(t, store, "proc-002")

	ctx := context.Background()
	exitCode := int32(0)
	if err := store.MarkEnded(ctx, fenceFor("proc-001"), StateExited, &exitCode, EndExited, true); err != nil {
		t.Fatalf("MarkEnded: %v", err)
	}

	killed, err := store.KillForLease(ctx, "org-a", "lease-1")
	if err != nil {
		t.Fatalf("KillForLease: %v", err)
	}
	if killed != 1 {
		t.Fatalf("killed %d processes, want only the live one", killed)
	}

	finished, err := store.Get(ctx, "org-a", "proc-001")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if finished.EndReason != EndExited {
		t.Fatalf("an already-finished process was rewritten: %q", finished.EndReason)
	}

	stopped, err := store.Get(ctx, "org-a", "proc-002")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if stopped.State != StateKilled || stopped.EndReason != EndLeaseReleased {
		t.Fatalf("live process = state %d reason %q", stopped.State, stopped.EndReason)
	}
	if stopped.CleanupState != CleanupPending {
		t.Fatal("lease release must not claim the OS process was cleaned up")
	}
}

func TestProcessStore_ListPagesNewestFirstAndHidesTerminalByDefault(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn).WithLimits(Limits{MaxLivePerLease: 10, MaxLivePerSpace: 10})
	seedLease(t, dsn, defaultLease())
	for i := 1; i <= 5; i++ {
		mustRegister(t, store, fmt.Sprintf("proc-%03d", i))
	}

	ctx := context.Background()
	exitCode := int32(0)
	if err := store.MarkEnded(ctx, fenceFor("proc-003"), StateExited, &exitCode, EndExited, true); err != nil {
		t.Fatalf("MarkEnded: %v", err)
	}

	live, hasMore, err := store.List(ctx, "org-a", "space-1", false, 10, "", nil)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(live) != 4 || hasMore {
		t.Fatalf("live listing = %d rows, hasMore=%v", len(live), hasMore)
	}
	if live[0].ID != "proc-005" {
		t.Fatalf("listing is not newest-first: %s", live[0].ID)
	}
	for _, p := range live {
		if p.State.IsTerminal() {
			t.Fatalf("terminal process %s leaked into the live listing", p.ID)
		}
	}

	all, _, err := store.List(ctx, "org-a", "space-1", true, 10, "", nil)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(all) != 5 {
		t.Fatalf("full listing = %d rows, want 5", len(all))
	}

	first, hasMore, err := store.List(ctx, "org-a", "space-1", true, 2, "", nil)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(first) != 2 || !hasMore {
		t.Fatalf("paged listing = %d rows, hasMore=%v", len(first), hasMore)
	}
	next, _, err := store.List(ctx, "org-a", "space-1", true, 2, first[len(first)-1].ID, nil)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(next) != 2 || next[0].ID >= first[len(first)-1].ID {
		t.Fatalf("cursor did not advance: %+v", next)
	}
}

// TestProcessStore_TheRedactedCommandRoundTripsAndNothingElseIsStored is the
// ZDR-shaped assertion for this table: what comes back is exactly the
// redacted argv that went in.
func TestProcessStore_TheRedactedCommandRoundTrips(t *testing.T) {
	dsn := setupProcessDB(t)
	store := newPoolStore(t, dsn)
	seedLease(t, dsn, defaultLease())

	p, err := store.Register(context.Background(), registerRequest("proc-001"))
	if err != nil {
		t.Fatalf("Register: %v", err)
	}
	if p.Command.Program != "python3" {
		t.Fatalf("program = %q", p.Command.Program)
	}
	want := []string{"worker.py", "--token", "[REDACTED]"}
	if len(p.Command.Args) != len(want) {
		t.Fatalf("args = %#v, want %#v", p.Command.Args, want)
	}
	for i := range want {
		if p.Command.Args[i] != want[i] {
			t.Fatalf("args = %#v, want %#v", p.Command.Args, want)
		}
	}
	if p.CommandDigest != "sha256:abc" {
		t.Fatalf("digest = %q", p.CommandDigest)
	}
}
