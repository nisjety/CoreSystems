package docstore

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// These tests are of the limit on the uploads an organisation holds, which Create enforces itself, in the transaction
// that stores the upload, under a lock of the organisation. A count made before the archive is read, which is what
// the service did alone until 2026-10-12, is passed by every upload that is under way at once: forty small archives a
// few milliseconds apart for a limit of ten stored fourteen with a fast insert and twenty-nine with a slow one. They run
// as the application's role, which is what the service is, so that a privilege the lock needs that the role does not
// have shows here.

// limited is an upload of one archive of the organisation with the limits a service would give it.
func limited(org, notice, archive string, perNotice, perOrg int) NewUpload {
	u := sample(org, notice, archive)
	u.MaxPerNotice, u.MaxPerOrg = perNotice, perOrg
	return u
}

// tally is how a burst of uploads ended.
type tally struct {
	created  int            // stored
	same     int            // the upload that was there already, found
	refused  map[string]int // by the scope of the limit that was reached
	failures []error        // anything else
}

// burst stores every upload at once, released together, and says how each ended.
func burst(ctx context.Context, store *Store, uploads []NewUpload) tally {
	t := tally{refused: map[string]int{}}
	var mu sync.Mutex
	var wg sync.WaitGroup
	start := make(chan struct{})
	for _, u := range uploads {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			_, created, err := store.Create(ctx, u)
			mu.Lock()
			defer mu.Unlock()
			var over *QuotaError
			switch {
			case errors.As(err, &over):
				t.refused[over.Scope]++
			case err != nil:
				t.failures = append(t.failures, err)
			case created:
				t.created++
			default:
				t.same++
			}
		}()
	}
	close(start)
	wg.Wait()
	return t
}

func rows(t *testing.T, env roleEnv, org string) int {
	t.Helper()
	return count(t, env.admin, `SELECT count(*) FROM tenant.tender_uploads WHERE org_id = $1`, org)
}

// Uploads that arrive at once are counted one after another: of thirty for a notice that may hold five, five are
// stored and twenty-five are refused, every time.
func TestTheLimitForANoticeHoldsWhateverArrivesAtOnce(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for round := 0; round < 6; round++ {
		notice := fmt.Sprintf("2026-10%04d", round)
		var uploads []NewUpload
		for i := 0; i < 30; i++ {
			uploads = append(uploads, limited("org-a", notice, fmt.Sprintf("round %d archive %d", round, i), 5, 200))
		}
		got := burst(ctx, env.store, uploads)
		if len(got.failures) > 0 {
			t.Fatalf("round %d: %s", round, dbtest.Describe(got.failures[0]))
		}
		if got.created != 5 || got.refused["notice"] != 25 || got.same != 0 {
			t.Errorf("round %d: %d stored, %d same, refused %v; want 5 stored and 25 refused for the notice", round, got.created, got.same, got.refused)
		}
		if n, _, err := env.store.Counts(ctx, "org-a", notice); err != nil || n != 5 {
			t.Errorf("round %d: the notice holds %d, %v; want 5", round, n, err)
		}
	}
}

// The limit in all counts the uploads of every notice of the organisation.
func TestTheLimitForAnOrganisationHoldsAcrossItsNoticesWhateverArrivesAtOnce(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	var uploads []NewUpload
	for n := 0; n < 4; n++ {
		for i := 0; i < 8; i++ {
			uploads = append(uploads, limited("org-a", fmt.Sprintf("2026-10%04d", n), fmt.Sprintf("notice %d archive %d", n, i), 100, 7))
		}
	}
	got := burst(ctx, env.store, uploads)
	if len(got.failures) > 0 {
		t.Fatalf("%s", dbtest.Describe(got.failures[0]))
	}
	if got.created != 7 || got.refused["organisation"] != 25 {
		t.Errorf("%d stored, refused %v; want 7 stored and 25 refused for the organisation", got.created, got.refused)
	}
	if n := rows(t, env, "org-a"); n != 7 {
		t.Errorf("the organisation holds %d rows, want 7", n)
	}
}

// What one organisation holds is no concern of another's, and two organisations at once do not hold each other up.
func TestOrganisationsAreCountedApartAndDoNotWaitForEachOther(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	var uploads []NewUpload
	for i := 0; i < 20; i++ {
		for _, org := range []string{"org-a", "org-b", "org-c"} {
			uploads = append(uploads, limited(org, "2026-100001", fmt.Sprintf("%s archive %d", org, i), 3, 200))
		}
	}
	got := burst(ctx, env.store, uploads)
	if len(got.failures) > 0 {
		t.Fatalf("%s", dbtest.Describe(got.failures[0]))
	}
	if got.created != 9 || got.refused["notice"] != 51 {
		t.Errorf("%d stored, refused %v; want 3 for each of three organisations, and 51 refused", got.created, got.refused)
	}
	for _, org := range []string{"org-a", "org-b", "org-c"} {
		if n := rows(t, env, org); n != 3 {
			t.Errorf("%s holds %d, want 3", org, n)
		}
	}
}

// The limit is exact to the upload, and the notice is looked at before the organisation.
func TestTheLimitIsExactAndTheNoticeIsLookedAtFirst(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for _, tc := range []struct {
		name, org         string
		perNotice, perOrg int
		notices           []string // the notice of each upload, in turn
		wantStored        int
		wantScope         string // of the refusal of the last one
	}{
		{"one", "org-one", 1, 200, []string{"2026-100001", "2026-100001"}, 1, "notice"},
		{"three for a notice", "org-notice", 3, 200, []string{"2026-100002", "2026-100002", "2026-100002", "2026-100002"}, 3, "notice"},
		{"three in all", "org-all", 200, 3, []string{"2026-100003", "2026-100004", "2026-100005", "2026-100006"}, 3, "organisation"},
		{"both reached at once", "org-both", 2, 2, []string{"2026-100007", "2026-100007", "2026-100007"}, 2, "notice"},
		// A limit that is 0 is no limit, and the other is still one.
		{"only the notice has a limit", "org-onlynotice", 2, 0, []string{"2026-100008", "2026-100009", "2026-100008", "2026-100008"}, 3, "notice"},
		{"only the organisation has a limit", "org-onlyorg", 0, 2, []string{"2026-100010", "2026-100010", "2026-100010"}, 2, "organisation"},
	} {
		var stored int
		var last error
		for i, notice := range tc.notices {
			_, created, err := env.store.Create(ctx, limited(tc.org, notice, fmt.Sprintf("%s %d", tc.name, i), tc.perNotice, tc.perOrg))
			if err == nil && created {
				stored++
			}
			last = err
		}
		var over *QuotaError
		if stored != tc.wantStored || !errors.As(last, &over) || over.Scope != tc.wantScope {
			t.Errorf("%s: %d stored and the last ended with %v; want %d stored and a refusal for the %s", tc.name, stored, last, tc.wantStored, tc.wantScope)
		}
	}
}

// The same archive again is the upload it already is, and is not counted against the limit: an organisation at its
// limit that sends what it has is told it has it, however many of them arrive at once, while a new archive is refused.
func TestTheSameArchiveIsNotRefusedForTheLimitItIsPartOf(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	first, created, err := env.store.Create(ctx, limited("org-a", "2026-100001", "a", 2, 200))
	if err != nil || !created {
		t.Fatalf("Create: %v %v", created, dbtest.Describe(err))
	}
	if _, _, err := env.store.Create(ctx, limited("org-a", "2026-100001", "b", 2, 200)); err != nil {
		t.Fatalf("Create: %s", dbtest.Describe(err))
	}

	again, created, err := env.store.Create(ctx, limited("org-a", "2026-100001", "a", 2, 200))
	if err != nil || created || again.ID != first.ID {
		t.Errorf("the same archive at the limit: id %d, created %v, %v; want the upload that is there (%d), not created, and no error", again.ID, created, err, first.ID)
	}
	var uploads []NewUpload
	for i := 0; i < 12; i++ {
		uploads = append(uploads, limited("org-a", "2026-100001", "a", 2, 200))
	}
	got := burst(ctx, env.store, uploads)
	if len(got.failures) > 0 || got.same != 12 || got.created != 0 || len(got.refused) != 0 {
		t.Errorf("twelve of the same archive at once: %d same, %d stored, refused %v, failures %v; want all twelve found", got.same, got.created, got.refused, got.failures)
	}
	var over *QuotaError
	if _, _, err := env.store.Create(ctx, limited("org-a", "2026-100001", "c", 2, 200)); !errors.As(err, &over) {
		t.Errorf("a new archive at the limit: %v, want a refusal", err)
	}
}

// An upload that has expired is not held: it is gone to every read, and it is not counted either.
func TestAnExpiredUploadIsNotCountedForTheLimit(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for i := 0; i < 3; i++ {
		insertExpired(t, env.admin, "org-a", "2026-100001", fmt.Sprintf("old %d", i))
	}
	for i := 0; i < 3; i++ {
		_, created, err := env.store.Create(ctx, limited("org-a", "2026-100001", fmt.Sprintf("new %d", i), 3, 3))
		if err != nil || !created {
			t.Fatalf("new %d, with three expired ones in the table: created %v, %v", i, created, dbtest.Describe(err))
		}
	}
	var over *QuotaError
	if _, _, err := env.store.Create(ctx, limited("org-a", "2026-100001", "one too many", 3, 3)); !errors.As(err, &over) {
		t.Errorf("a fourth live one: %v, want a refusal", err)
	}
}

// What a refused upload leaves is nothing: no row, and no lock, because the lock is the transaction's and the
// transaction ended.
func TestNothingIsLeftBehindWhenTheLimitIsReached(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	var uploads []NewUpload
	for i := 0; i < 20; i++ {
		uploads = append(uploads, limited("org-a", "2026-100001", fmt.Sprintf("archive %d", i), 4, 200))
	}
	if got := burst(ctx, env.store, uploads); len(got.failures) > 0 || got.created != 4 {
		t.Fatalf("%d stored, failures %v", got.created, got.failures)
	}
	if n := rows(t, env, "org-a"); n != 4 {
		t.Errorf("%d rows, want 4: a refused upload left one", n)
	}
	if n := count(t, env.admin, `SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND classid::bigint = $1`, int64(quotaLockClass)); n != 0 {
		t.Errorf("%d advisory locks of the store are still held", n)
	}
}

// The lock, tried with the count out of the way: a Create that has limits waits for the lock of its organisation,
// goes on when it is released, and is not held up by the lock of another organisation.
func TestCreateWaitsForTheLockOfItsOrganisationAndOnlyForThat(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)

	tx, err := env.admin.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	released := false
	release := func() {
		if !released {
			released = true
			_ = tx.Rollback(ctx)
		}
	}
	defer release()
	if _, err := tx.Exec(ctx, lockOrganisation, int32(quotaLockClass), "org-a"); err != nil {
		t.Fatalf("take the lock of org-a: %s", dbtest.Describe(err))
	}

	other := make(chan error, 1)
	go func() {
		_, _, err := env.store.Create(ctx, limited("org-b", "2026-100001", "b", 5, 200))
		other <- err
	}()
	select {
	case err := <-other:
		if err != nil {
			t.Fatalf("org-b: %s", dbtest.Describe(err))
		}
	case <-time.After(15 * time.Second):
		t.Fatal("an upload of org-b waited for the lock of org-a")
	}

	mine := make(chan error, 1)
	go func() {
		_, _, err := env.store.Create(ctx, limited("org-a", "2026-100001", "a", 5, 200))
		mine <- err
	}()
	select {
	case err := <-mine:
		t.Fatalf("an upload of org-a went on while its lock was held by another transaction (%v)", err)
	case <-time.After(700 * time.Millisecond):
	}
	release()
	select {
	case err := <-mine:
		if err != nil {
			t.Fatalf("org-a, after the lock was released: %s", dbtest.Describe(err))
		}
	case <-time.After(15 * time.Second):
		t.Fatal("an upload of org-a did not go on when the lock was released")
	}
}

// With no limit there is nothing to count and nothing to serialise: an upload that has none takes no lock, and is
// not held up by one, and an organisation with none holds as many as it stores.
func TestAnUploadWithNoLimitTakesNoLockAndIsNotCounted(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)

	tx, err := env.admin.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, lockOrganisation, int32(quotaLockClass), "org-a"); err != nil {
		t.Fatalf("take the lock: %s", dbtest.Describe(err))
	}
	done := make(chan error, 1)
	go func() {
		for i := 0; i < 15; i++ {
			if _, _, err := env.store.Create(ctx, sample("org-a", "2026-100001", fmt.Sprintf("archive %d", i))); err != nil {
				done <- err
				return
			}
		}
		done <- nil
	}()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("Create with no limit: %s", dbtest.Describe(err))
		}
	case <-time.After(30 * time.Second):
		t.Fatal("an upload with no limit waited for a lock")
	}
	if n := rows(t, env, "org-a"); n != 15 {
		t.Errorf("%d rows, want 15", n)
	}
}

func TestAQuotaErrorSaysWhichLimitWasReached(t *testing.T) {
	t.Parallel()

	var err error = &QuotaError{Scope: "notice"}
	var over *QuotaError
	if !errors.As(err, &over) || over.Scope != "notice" {
		t.Fatalf("errors.As: %v", over)
	}
	if got := err.Error(); got != "docstore: the organisation already holds as many uploads as it may for the notice" {
		t.Errorf("Error() = %q", got)
	}
}

// The count after the lock has to see what the one that held the lock before it committed, which a transaction at the
// default level, READ COMMITTED, does: each statement sees what is committed when it begins. A role whose sessions
// default to a stricter level would count in a snapshot taken before the wait for the lock, and every upload that
// waited would count what the one before it had not yet stored. The store asks for the level itself, whatever the
// role's sessions default to.
func TestTheLimitHoldsWhateverIsolationTheRoleDefaultsTo(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	database := pgx.Identifier{env.rs.Database}.Sanitize()
	if _, err := env.admin.Exec(ctx, "ALTER ROLE anbud_app IN DATABASE "+database+" SET default_transaction_isolation = 'repeatable read'"); err != nil {
		t.Fatalf("set the role's default: %s", dbtest.Describe(err))
	}
	app := dbtest.Pool(t, env.rs.AppDSN) // sessions that begin with the setting
	var level string
	if err := app.QueryRow(ctx, `SHOW default_transaction_isolation`).Scan(&level); err != nil || level != "repeatable read" {
		t.Fatalf("the role's sessions default to %q, %v; the test needs them to default to repeatable read", level, err)
	}

	var uploads []NewUpload
	for i := 0; i < 30; i++ {
		uploads = append(uploads, limited("org-a", "2026-100001", fmt.Sprintf("archive %d", i), 5, 200))
	}
	got := burst(ctx, New(app), uploads)
	if len(got.failures) > 0 {
		t.Fatalf("%s", dbtest.Describe(got.failures[0]))
	}
	if got.created != 5 || got.refused["notice"] != 25 {
		t.Errorf("%d stored, refused %v; want 5 stored and 25 refused for the notice", got.created, got.refused)
	}
}

// The count names the organisation in its own statement, and does not leave that to row level security: the owner of the
// table is not held to the policy, and an organisation that is counted with another's uploads is refused for them.
// This is the statement tried with the policy out of the way.
func TestTheCountNamesTheOrganisationWithoutRowLevelSecurity(t *testing.T) {
	store, pool := openStore(t) // the server's own role, which owns the tables and so bypasses the policy
	ctx := testCtx(t)
	for _, org := range []string{"org-a", "org-b"} {
		for i := 0; i < 2; i++ {
			if _, created, err := store.Create(ctx, limited(org, "2026-100001", fmt.Sprintf("%s archive %d", org, i), 2, 3)); err != nil || !created {
				t.Fatalf("%s upload %d: created %v, %s", org, i, created, dbtest.Describe(err))
			}
		}
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 4 {
		t.Fatalf("%d rows, want 4", n)
	}
	var over *QuotaError
	if _, _, err := store.Create(ctx, limited("org-a", "2026-100001", "one more of org-a", 2, 3)); !errors.As(err, &over) || over.Scope != "notice" {
		t.Errorf("a third for org-a: %v, want a refusal for the notice", err)
	}
	// org-c has nothing, and the other two hold four between them: not counted against it.
	if _, created, err := store.Create(ctx, limited("org-c", "2026-100001", "org-c", 2, 3)); err != nil || !created {
		t.Errorf("org-c: created %v, %v; want it stored, for the others' uploads are not its own", created, err)
	}
	if forNotice, forOrg, err := store.Counts(ctx, "org-c", "2026-100001"); err != nil || forNotice != 1 || forOrg != 1 {
		t.Errorf("Counts for org-c: %d, %d, %v; want 1 and 1", forNotice, forOrg, err)
	}
}
