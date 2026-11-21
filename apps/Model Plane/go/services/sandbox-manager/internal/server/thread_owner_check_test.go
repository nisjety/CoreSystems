package server

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc/codes"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadowner"
)

// O-13: with SANDBOX_MANAGER_THREAD_OWNER_CHECK on, sandbox-manager asks
// session-core itself whether the verified user owns the thread. These tests run
// on a fake session-core table; the cache and the gRPC adapter are tested in
// internal/threadowner.

// ownerTable is a session-core stand-in: the (org, user, thread) triples it
// answers yes for, an optional outage, and a log of every question asked.
type ownerTable struct {
	mu    sync.Mutex
	owned map[[3]string]bool
	down  error
	asked [][3]string
}

func newOwnerTable(owned ...[3]string) *ownerTable {
	t := &ownerTable{owned: map[[3]string]bool{}}
	for _, k := range owned {
		t.owned[k] = true
	}
	return t
}

func (o *ownerTable) OwnsThread(_ context.Context, orgID, userID, threadID string) (bool, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	key := [3]string{orgID, userID, threadID}
	o.asked = append(o.asked, key)
	if o.down != nil {
		return false, o.down
	}
	return o.owned[key], nil
}

func (o *ownerTable) setDown(err error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.down = err
}

func (o *ownerTable) questions() int {
	o.mu.Lock()
	defer o.mu.Unlock()
	return len(o.asked)
}

func ownerHarness(t *testing.T, owners threadowner.Resolver) (*threadHarness, *erasureRecorder) {
	t.Helper()
	h, log, _, _ := erasureHarness(t, true)
	h.s.WithThreadOwnerCheck(owners)
	return h, log
}

// TestThreadOwnerCheckRefusesAForeignThreadOnEveryPath: user-2 does not own
// thread-1, so acquiring its lease and erasing its workspace both answer NOT_FOUND
// and write nothing.
func TestThreadOwnerCheckRefusesAForeignThreadOnEveryPath(t *testing.T) {
	t.Parallel()
	h, log := ownerHarness(t, newOwnerTable([3]string{"org-1", "user-1", "thread-1"}))
	*h.who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-2", PrincipalType: "user"}

	_, err := h.acquireThread("thread-1", time.Minute)
	wantCode(t, err, codes.NotFound, "foreign user acquiring a thread lease")
	if len(h.leases.byID) != 0 {
		t.Fatalf("a refused acquire wrote %d lease row(s)", len(h.leases.byID))
	}
	_, err = h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	wantCode(t, err, codes.NotFound, "foreign user erasing a thread workspace")
	if len(log.events) != 0 {
		t.Fatalf("a refused erase reached the eraser: %v", log.events)
	}
}

// TestThreadOwnerCheckAnswersAMissingThreadLikeAForeignOne: a thread session-core
// does not know reads the same as another user's, so the answer cannot be used to
// probe which thread ids exist.
func TestThreadOwnerCheckAnswersAMissingThreadLikeAForeignOne(t *testing.T) {
	t.Parallel()
	h, _ := ownerHarness(t, newOwnerTable([3]string{"org-1", "user-1", "thread-1"}))

	_, missingErr := h.acquireThread("thread-nope", time.Minute)
	wantCode(t, missingErr, codes.NotFound, "missing thread")
	*h.who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-2", PrincipalType: "user"}
	_, foreignErr := h.acquireThread("thread-1", time.Minute)
	wantCode(t, foreignErr, codes.NotFound, "foreign thread")
	if missingErr.Error() != foreignErr.Error() {
		t.Fatalf("missing = %q, foreign = %q: the two answers must be identical", missingErr, foreignErr)
	}
}

// TestThreadOwnerCheckPassesTheOwnerThroughTheWholeFlow: the owner acquires,
// writes, reads and erases, and each step asks with the verified org and user and
// the lease's thread id (never a request field the caller chose).
func TestThreadOwnerCheckPassesTheOwnerThroughTheWholeFlow(t *testing.T) {
	t.Parallel()
	table := newOwnerTable([3]string{"org-1", "user-1", "thread-1"})
	h, log := ownerHarness(t, table)

	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("owner acquire: %v", err)
	}
	if _, err := h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1}}, nil); err != nil {
		t.Fatalf("owner apply: %v", err)
	}
	if _, err := h.read(lease.GetLeaseId()); err != nil {
		t.Fatalf("owner read: %v", err)
	}
	if _, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1")); err != nil {
		t.Fatalf("owner erase: %v", err)
	}
	if len(log.events) == 0 {
		t.Fatal("the owner's erase never reached the eraser")
	}
	for _, q := range table.asked {
		if q != [3]string{"org-1", "user-1", "thread-1"} {
			t.Fatalf("session-core was asked about %v", q)
		}
	}
	if len(table.asked) != 4 {
		t.Fatalf("session-core was asked %d times, want one per thread-scoped call (4)", len(table.asked))
	}
}

// TestThreadOwnerCheckGuardsAnOldLeaseOnReadAndWrite: a lease minted while the
// user owned the thread is not proof of ownership later. Once session-core stops
// saying yes, the read and the write through that lease are refused.
func TestThreadOwnerCheckGuardsAnOldLeaseOnReadAndWrite(t *testing.T) {
	t.Parallel()
	table := newOwnerTable([3]string{"org-1", "user-1", "thread-1"})
	h, _ := ownerHarness(t, table)
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	table.mu.Lock()
	delete(table.owned, [3]string{"org-1", "user-1", "thread-1"})
	table.mu.Unlock()

	_, err = h.read(lease.GetLeaseId())
	wantCode(t, err, codes.NotFound, "read through a lease of a thread no longer owned")
	_, err = h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "x.txt", ContentHash: "sha256:x", SizeBytes: 1}}, nil)
	wantCode(t, err, codes.NotFound, "write through a lease of a thread no longer owned")
}

// TestThreadOwnerCheckFailsClosedWhenSessionCoreIsDown: an outage is UNAVAILABLE on
// every thread-scoped path and never falls through to serving the request.
func TestThreadOwnerCheckFailsClosedWhenSessionCoreIsDown(t *testing.T) {
	t.Parallel()
	table := newOwnerTable([3]string{"org-1", "user-1", "thread-1"})
	h, log := ownerHarness(t, table)
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire before the outage: %v", err)
	}
	table.setDown(errors.New("session-core unreachable"))

	_, err = h.acquireThread("thread-2", time.Minute)
	wantCode(t, err, codes.Unavailable, "acquire during an outage")
	_, err = h.read(lease.GetLeaseId())
	wantCode(t, err, codes.Unavailable, "read during an outage")
	_, err = h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "x.txt", ContentHash: "sha256:x", SizeBytes: 1}}, nil)
	wantCode(t, err, codes.Unavailable, "write during an outage")
	_, err = h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	wantCode(t, err, codes.Unavailable, "erase during an outage")
	if len(log.events) != 0 {
		t.Fatalf("an erase ran during an outage: %v", log.events)
	}
	if len(h.leases.byID) != 1 {
		t.Fatalf("lease rows = %d, want only the one minted before the outage", len(h.leases.byID))
	}
}

// TestThreadOwnerCheckPositiveCacheExpires: through the real cache, a yes is served
// for its lifetime without a second question and asked again after it.
func TestThreadOwnerCheckPositiveCacheExpires(t *testing.T) {
	t.Parallel()
	table := newOwnerTable([3]string{"org-1", "user-1", "thread-1"})
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	var mu sync.Mutex
	clock := func() time.Time { mu.Lock(); defer mu.Unlock(); return now }
	cache, err := threadowner.NewCache(table, threadowner.DefaultCacheTTL)
	if err != nil {
		t.Fatal(err)
	}
	cache.WithClock(clock)
	h, _ := ownerHarness(t, cache)

	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	if _, err := h.read(lease.GetLeaseId()); err != nil {
		t.Fatalf("read: %v", err)
	}
	if table.questions() != 1 {
		t.Fatalf("session-core asked %d times inside the window, want 1", table.questions())
	}
	mu.Lock()
	now = now.Add(threadowner.DefaultCacheTTL)
	mu.Unlock()
	if _, err := h.read(lease.GetLeaseId()); err != nil {
		t.Fatalf("read after expiry: %v", err)
	}
	if table.questions() != 2 {
		t.Fatalf("session-core asked %d times after expiry, want 2", table.questions())
	}

	// And the stale yes does not outlive the thread: once session-core says no,
	// the next read after the window is refused.
	table.mu.Lock()
	delete(table.owned, [3]string{"org-1", "user-1", "thread-1"})
	table.mu.Unlock()
	mu.Lock()
	now = now.Add(threadowner.DefaultCacheTTL)
	mu.Unlock()
	_, err = h.read(lease.GetLeaseId())
	wantCode(t, err, codes.NotFound, "read after the thread stopped being the user's")
}

// TestThreadOwnerCheckOffLeavesEveryPathUnchanged: with no resolver wired, an
// unowned thread id is served exactly as before, and the check asks nothing.
func TestThreadOwnerCheckOffLeavesEveryPathUnchanged(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, true)
	if h.s.threadOwners != nil {
		t.Fatal("a fresh server has a thread owner resolver wired")
	}
	lease, err := h.acquireThread("any-thread", time.Minute)
	if err != nil {
		t.Fatalf("acquire with the switch off: %v", err)
	}
	if _, err := h.read(lease.GetLeaseId()); err != nil {
		t.Fatalf("read with the switch off: %v", err)
	}
	if _, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "any-thread")); err != nil {
		t.Fatalf("erase with the switch off: %v", err)
	}
	if want := "rows org-1/user-1/any-thread|blobs org-1/any-thread"; strings.Join(log.events, "|") != want {
		t.Fatalf("events = %v, want %q", log.events, want)
	}
}

// TestThreadOwnerCheckDoesNotGateTheServiceRetentionErase: the service twin that
// session-core's retention sweep calls runs after a thread is deleted, so asking
// "does the user own it" there would block the very erase it exists for.
func TestThreadOwnerCheckDoesNotGateTheServiceRetentionErase(t *testing.T) {
	t.Parallel()
	table := newOwnerTable()
	h, log := ownerHarness(t, table)
	h.s.WithServiceThreadErase(true)
	*h.who = authctx.Principal{
		OrganizationID: "org-1", ActorID: "service:session-core", PrincipalType: "service",
		Scopes: []string{authz.ScopeThreadWorkspaceEraseAny},
	}
	if _, err := h.s.eraseThreadWorkspaceForRetention(context.Background(), "org-1", "thread-gone", "user-1"); err != nil {
		t.Fatalf("service retention erase: %v", err)
	}
	if table.questions() != 0 {
		t.Fatalf("the service erase asked session-core %d times, want 0", table.questions())
	}
	if len(log.events) == 0 {
		t.Fatal("the service erase never reached the eraser")
	}
}
