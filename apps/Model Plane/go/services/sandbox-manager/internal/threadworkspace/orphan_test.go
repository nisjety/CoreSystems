package threadworkspace

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"
)

// O-13: the orphan sweep erases the workspace of a thread that no longer exists, and
// only that. These tests run on fakes; the SQL is in orphan_integration_test.go.

// fakeOrphans is an orphanEraser over an in-memory table of workspaces, listed in
// primary-key order the way the SQL lists them. A key in recreated was erased and
// written again after the sweep listed it, so EraseOrphan keeps it.
type fakeOrphans struct {
	mu        sync.Mutex
	table     []ThreadKey
	recreated map[ThreadKey]bool
	listErr   error
	eraseErr  error
	cutoffs   []time.Time
	afters    []ThreadKey
	erased    []ThreadKey
	completed []ThreadKey
}

func newFakeOrphans(keys ...ThreadKey) *fakeOrphans {
	f := &fakeOrphans{table: append([]ThreadKey(nil), keys...), recreated: map[ThreadKey]bool{}}
	sort.Slice(f.table, func(i, j int) bool { return keyLess(f.table[i], f.table[j]) })
	return f
}

func keyLess(a, b ThreadKey) bool {
	if a.OrgID != b.OrgID {
		return a.OrgID < b.OrgID
	}
	if a.OwnerID != b.OwnerID {
		return a.OwnerID < b.OwnerID
	}
	return a.ThreadID < b.ThreadID
}

func (f *fakeOrphans) ListOlderThan(_ context.Context, cutoff time.Time, after ThreadKey, limit int) ([]ThreadKey, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.cutoffs = append(f.cutoffs, cutoff)
	f.afters = append(f.afters, after)
	if f.listErr != nil {
		return nil, f.listErr
	}
	var out []ThreadKey
	for _, key := range f.table {
		if keyLess(after, key) {
			out = append(out, key)
			if len(out) == limit {
				break
			}
		}
	}
	return out, nil
}

func (f *fakeOrphans) EraseOrphan(_ context.Context, key ThreadKey, _ time.Time) (bool, int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.eraseErr != nil {
		return false, 0, f.eraseErr
	}
	if f.recreated[key] {
		return false, 0, nil
	}
	f.erased = append(f.erased, key)
	f.table = slices.DeleteFunc(f.table, func(k ThreadKey) bool { return k == key })
	return true, 2, nil
}

func (f *fakeOrphans) CompleteBlobErasure(_ context.Context, key ThreadKey) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.completed = append(f.completed, key)
	return nil
}

// fakeExistence answers per org from a table of explicit answers, and logs each question.
type fakeExistence struct {
	mu      sync.Mutex
	answers map[string]map[string]bool
	err     error
	errOrgs map[string]bool
	asked   []string
}

func (f *fakeExistence) ThreadsExist(_ context.Context, orgID string, ids []string) (map[string]bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.asked = append(f.asked, orgID+":"+strings.Join(ids, ","))
	if f.err != nil || f.errOrgs[orgID] {
		return nil, errors.New("session-core unavailable")
	}
	out := map[string]bool{}
	for _, id := range ids {
		if v, ok := f.answers[orgID][id]; ok {
			out[id] = v
		}
	}
	return out, nil
}

var (
	liveThread   = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_live"}
	goneThread   = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_gone"}
	unknownThing = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_unknown"}
)

func orphanSweeper(rows idleEraser, orphans orphanEraser, exists ThreadExistence, blobs BlobEraser) *Sweeper {
	return NewSweeper(rows, blobs, 30*24*time.Hour).WithOrphanSweep(orphans, exists, 0)
}

// TestOrphanSweepErasesAGoneThreadAndKeepsALiveOne: the workspace of a thread
// session-core says does not exist goes, rows then blobs, with its tombstone closed;
// the workspace of a live thread stays untouched.
func TestOrphanSweepErasesAGoneThreadAndKeepsALiveOne(t *testing.T) {
	t.Parallel()
	orphans := newFakeOrphans(liveThread, goneThread)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_live": true, "thr_gone": false}}}
	blobs := &fakeBlobs{}
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), now)
	if err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if erased != 1 {
		t.Fatalf("erased = %d, want 1", erased)
	}
	if !slices.Equal(orphans.erased, []ThreadKey{goneThread}) {
		t.Fatalf("erased keys = %v, want only the gone thread", orphans.erased)
	}
	if !slices.Equal(blobs.removed, []string{"org_1/thr_gone"}) {
		t.Fatalf("blobs removed = %v, want only the gone thread's", blobs.removed)
	}
	if !slices.Equal(orphans.completed, []ThreadKey{goneThread}) {
		t.Fatalf("completed = %v, want the gone thread's tombstone closed", orphans.completed)
	}
	if !orphans.cutoffs[0].Equal(now.Add(-DefaultOrphanGrace)) {
		t.Fatalf("cutoff = %v, want now minus the 24 hour grace", orphans.cutoffs[0])
	}
}

// TestOrphanSweepNeverErasesAThreadSessionCoreDidNotAnswerFor: an id missing from
// the answer is unknown, not gone.
func TestOrphanSweepNeverErasesAThreadSessionCoreDidNotAnswerFor(t *testing.T) {
	t.Parallel()
	orphans := newFakeOrphans(unknownThing, liveThread)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_live": true}}}
	blobs := &fakeBlobs{}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil || erased != 0 {
		t.Fatalf("sweep = (%d, %v), want nothing erased", erased, err)
	}
	if len(orphans.erased) != 0 || len(blobs.removed) != 0 {
		t.Fatalf("erased %v and removed %v for threads session-core did not answer about", orphans.erased, blobs.removed)
	}
}

// TestOrphanSweepKeepsEverythingWhenSessionCoreIsDown: a failed existence read keeps
// every workspace it covered, and another org's page still goes on.
func TestOrphanSweepKeepsEverythingWhenSessionCoreIsDown(t *testing.T) {
	t.Parallel()
	otherGone := ThreadKey{OrgID: "org_2", OwnerID: "user_9", ThreadID: "thr_other_gone"}
	orphans := newFakeOrphans(goneThread, otherGone)
	exists := &fakeExistence{
		errOrgs: map[string]bool{"org_1": true},
		answers: map[string]map[string]bool{"org_2": {"thr_other_gone": false}},
	}
	blobs := &fakeBlobs{}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if erased != 1 || !slices.Equal(orphans.erased, []ThreadKey{otherGone}) {
		t.Fatalf("erased = %d %v, want only org_2's thread (org_1's read failed)", erased, orphans.erased)
	}

	exists = &fakeExistence{err: errors.New("down")}
	orphans = newFakeOrphans(goneThread, otherGone)
	erased, err = orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil || erased != 0 || len(orphans.erased) != 0 {
		t.Fatalf("with session-core fully down: (%d, %v) erased %v, want nothing erased and no error", erased, err, orphans.erased)
	}
}

// TestOrphanSweepKeepsAWorkspaceWrittenAgainAfterTheListing: a workspace that was
// recreated since the page was listed is a new one, and EraseOrphan keeps it.
func TestOrphanSweepKeepsAWorkspaceWrittenAgainAfterTheListing(t *testing.T) {
	t.Parallel()
	orphans := newFakeOrphans(goneThread)
	orphans.recreated[goneThread] = true
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_gone": false}}}
	blobs := &fakeBlobs{}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil || erased != 0 {
		t.Fatalf("sweep = (%d, %v), want nothing erased", erased, err)
	}
	if len(blobs.removed) != 0 {
		t.Fatalf("blobs removed = %v for a workspace that was kept", blobs.removed)
	}
}

// TestOrphanSweepLeavesTheTombstoneWhenTheBlobsFail: the rows are gone and the blobs
// are not, so the tombstone stays for F-f06-02's retry and the error is not fatal.
func TestOrphanSweepLeavesTheTombstoneWhenTheBlobsFail(t *testing.T) {
	t.Parallel()
	orphans := newFakeOrphans(goneThread)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_gone": false}}}
	blobs := &fakeBlobs{err: errors.New("object store unavailable")}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want one erased and no error", erased, err)
	}
	if len(orphans.completed) != 0 {
		t.Fatalf("completed = %v after a failed removal; the tombstone must stay", orphans.completed)
	}
}

// TestOrphanSweepErasesEveryOwnerOfAGoneThreadAndRemovesItsBlobsOnce: two owners
// that hold the same gone thread id are both orphans, and the shared prefix is
// removed once, after both.
func TestOrphanSweepErasesEveryOwnerOfAGoneThreadAndRemovesItsBlobsOnce(t *testing.T) {
	t.Parallel()
	second := ThreadKey{OrgID: "org_1", OwnerID: "user_2", ThreadID: "thr_gone"}
	orphans := newFakeOrphans(goneThread, second)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_gone": false}}}
	blobs := &fakeBlobs{}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now())
	if err != nil || erased != 2 {
		t.Fatalf("sweep = (%d, %v), want both owners erased", erased, err)
	}
	if !slices.Equal(blobs.removed, []string{"org_1/thr_gone"}) {
		t.Fatalf("blobs removed = %v, want the shared prefix once", blobs.removed)
	}
	if len(orphans.completed) != 2 {
		t.Fatalf("completed = %v, want both tombstones closed", orphans.completed)
	}
	if len(exists.asked) != 1 || exists.asked[0] != "org_1:thr_gone" {
		t.Fatalf("existence questions = %v, want one question with the id once", exists.asked)
	}
}

// TestOrphanSweepWalksTheWholeTableAcrossPages: a table longer than one page is read
// page by page from where the last page ended, and a live thread early in the table
// never hides a gone one later in it.
func TestOrphanSweepWalksTheWholeTableAcrossPages(t *testing.T) {
	t.Parallel()
	var keys []ThreadKey
	answers := map[string]bool{}
	for i := 0; i < orphanPageSize+30; i++ {
		id := fmt.Sprintf("thr_%04d", i)
		keys = append(keys, ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: id})
		answers[id] = true
	}
	answers[keys[len(keys)-1].ThreadID] = false
	orphans := newFakeOrphans(keys...)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": answers}}

	erased, err := orphanSweeper(newFakeIdle(), orphans, exists, &fakeBlobs{}).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want the one gone thread on the second page erased", erased, err)
	}
	if len(orphans.afters) != 2 || orphans.afters[0] != (ThreadKey{}) || orphans.afters[1] != keys[orphanPageSize-1] {
		t.Fatalf("page cursors = %v, want the start and then the end of page one", orphans.afters)
	}
}

// TestOrphanSweepResumesWhereItStoppedAndWrapsAround: a pass is bounded, so a long
// table takes several passes, each picking up at the last key, and the walk starts
// over when it reaches the end.
func TestOrphanSweepResumesWhereItStoppedAndWrapsAround(t *testing.T) {
	t.Parallel()
	total := orphanPageSize*orphanPagesPerPass + 5
	var keys []ThreadKey
	for i := 0; i < total; i++ {
		keys = append(keys, ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: fmt.Sprintf("thr_%06d", i)})
	}
	answers := map[string]bool{}
	for _, k := range keys {
		answers[k.ThreadID] = true
	}
	orphans := newFakeOrphans(keys...)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": answers}}
	sweeper := orphanSweeper(newFakeIdle(), orphans, exists, &fakeBlobs{})

	if _, err := sweeper.sweep(context.Background(), time.Now()); err != nil {
		t.Fatal(err)
	}
	if got := len(orphans.afters); got != orphanPagesPerPass {
		t.Fatalf("first pass read %d pages, want the bound %d", got, orphanPagesPerPass)
	}
	if sweeper.orphans.cursor == (ThreadKey{}) {
		t.Fatal("the cursor was reset before the end of the table")
	}
	if _, err := sweeper.sweep(context.Background(), time.Now()); err != nil {
		t.Fatal(err)
	}
	if got := orphans.afters[orphanPagesPerPass]; got != keys[orphanPageSize*orphanPagesPerPass-1] {
		t.Fatalf("second pass started after %v, want it to resume after the last key of the first", got)
	}
	if sweeper.orphans.cursor != (ThreadKey{}) {
		t.Fatalf("the cursor = %v after reaching the end, want it reset", sweeper.orphans.cursor)
	}
}

// TestOrphanSweepOffChangesNothing: without WithOrphanSweep the pass does not run, no
// existence question is asked, and the sweep is the one every earlier test pins.
func TestOrphanSweepOffChangesNothing(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	blobs := &fakeBlobs{}
	sweeper := NewSweeper(rows, blobs, 30*24*time.Hour)
	if sweeper.orphans.rows != nil || sweeper.orphans.exists != nil {
		t.Fatal("a plain sweeper has the orphan pass configured")
	}
	erased, err := sweeper.sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want only the idle erase", erased, err)
	}
	if !slices.Equal(blobs.removed, []string{"org_1/thr_idle"}) {
		t.Fatalf("blobs removed = %v", blobs.removed)
	}
}

// TestOrphanSweepListingFailureDoesNotHoldBackTheStaleLeasePass: the orphan pass is
// best effort, like the retry drain.
func TestOrphanSweepListingFailureDoesNotHoldBackTheStaleLeasePass(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle()
	orphans := newFakeOrphans(goneThread)
	orphans.listErr = errors.New("connection reset")
	exists := &fakeExistence{}

	if _, err := orphanSweeper(rows, orphans, exists, &fakeBlobs{}).sweep(context.Background(), time.Now()); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if len(rows.pruneCutoffs) != 1 {
		t.Fatalf("stale lease passes = %d, want 1 even after the orphan listing failed", len(rows.pruneCutoffs))
	}
}

// TestOrphanSweepLogsIdsOnly: the log names the org, the thread and counts, never a
// blob key or an object store error.
func TestOrphanSweepLogsIdsOnly(t *testing.T) {
	// Not parallel: it swaps the process-wide default logger.
	var buf bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug})))
	defer slog.SetDefault(previous)

	orphans := newFakeOrphans(goneThread)
	exists := &fakeExistence{answers: map[string]map[string]bool{"org_1": {"thr_gone": false}}}
	blobs := &fakeBlobs{err: errors.New(`remove "cas/thread/org_1/thr_gone/0a1b2c3d4e5f": access denied`)}

	if _, err := orphanSweeper(newFakeIdle(), orphans, exists, blobs).sweep(context.Background(), time.Now()); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	logged := buf.String()
	if strings.Contains(logged, "cas/thread") || strings.Contains(logged, "0a1b2c3d4e5f") {
		t.Fatalf("log names a blob key or path: %s", logged)
	}
	if !strings.Contains(logged, "org_1") || !strings.Contains(logged, "thr_gone") {
		t.Fatalf("log = %s, want the org and the thread", logged)
	}
}

func TestOrphanGraceFromEnv(t *testing.T) {
	t.Parallel()
	get := func(v string) func(string) string {
		return func(string) string { return v }
	}
	if d, err := OrphanGraceFromEnv(get("")); err != nil || d != DefaultOrphanGrace {
		t.Fatalf("unset = (%v, %v), want the 24 hour default", d, err)
	}
	if d, err := OrphanGraceFromEnv(get("48")); err != nil || d != 48*time.Hour {
		t.Fatalf("48 = (%v, %v)", d, err)
	}
	for _, bad := range []string{"0", "-3", "soon", "1.5"} {
		if _, err := OrphanGraceFromEnv(get(bad)); err == nil {
			t.Fatalf("%q was accepted", bad)
		}
	}
}
