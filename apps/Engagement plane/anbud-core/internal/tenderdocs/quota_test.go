package tenderdocs

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/docstore"
)

// The limit on what an organisation holds is enforced by the store, in the transaction that stores the upload, and
// the service gives it the limits. The count the service makes before it reads an archive is a courtesy, so that an
// archive that cannot be kept is not read; it is passed by every upload that is under way at once, and was all there
// was until 2026-10-12.

func TestTheServiceGivesTheStoreTheLimitsItEnforces(t *testing.T) {
	for name, tc := range map[string]struct {
		tweak             func(*Options)
		perNotice, perOrg int
	}{
		"the defaults":        {func(*Options) {}, 10, 200},
		"set by the operator": {func(o *Options) { o.MaxUploadsPerNotice, o.MaxUploadsPerOrganisation = 4, 9 }, 4, 9},
	} {
		f := newFixture(t, tc.tweak)
		if _, _, err := f.upload(t, zipOf(t, file("a.txt", "x"))); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		got := f.store.creates[0]
		if got.MaxPerNotice != tc.perNotice || got.MaxPerOrg != tc.perOrg {
			t.Errorf("%s: the store was given limits of %d and %d, want %d and %d", name, got.MaxPerNotice, got.MaxPerOrg, tc.perNotice, tc.perOrg)
		}
	}
}

// racing stores some uploads of the organisation, as other requests do, between the count the service made and the
// call that stores the upload.
type racing struct {
	*fakeStore
	rivals int
	once   sync.Once
}

func (r *racing) Create(ctx context.Context, in docstore.NewUpload) (docstore.Upload, bool, error) {
	r.once.Do(func() {
		for i := 0; i < r.rivals; i++ {
			rival := in
			rival.ArchiveSHA256 = hashOf(fmt.Appendf(nil, "rival %d", i))
			if _, _, err := r.fakeStore.Create(ctx, rival); err != nil {
				panic(err)
			}
		}
	})
	return r.fakeStore.Create(ctx, in)
}

// An upload that was counted and found room, and loses the race for the last place to another, is refused for the
// limit as one that was counted too late would be, and is not stored.
func TestAnUploadThatLosesTheRaceForTheLastPlaceIsRefusedForTheLimit(t *testing.T) {
	for name, tc := range map[string]struct {
		perNotice, perOrg int
		scope             string
	}{
		"the notice":       {2, 200, "notice"},
		"the organisation": {200, 2, "organisation"},
	} {
		f := newFixture(t, func(o *Options) { o.MaxUploadsPerNotice, o.MaxUploadsPerOrganisation = tc.perNotice, tc.perOrg })
		rival := &racing{fakeStore: f.store, rivals: 2}
		svc, err := New(Options{Store: rival, Notices: f.notices, Reader: f.svc.reader, Limits: testLimits(), Retention: 90 * 24 * time.Hour,
			MaxUploadsPerNotice: tc.perNotice, MaxUploadsPerOrganisation: tc.perOrg})
		if err != nil {
			t.Fatal(err)
		}
		_, created, err := svc.Upload(context.Background(), UploadRequest{OrgID: testOrg, NoticeID: testNotice, Archive: zipOf(t, file("a.txt", "x"))})
		var over *QuotaError
		if created || !errors.As(err, &over) || over.Scope != tc.scope {
			t.Fatalf("%s: created %v, err %v; want a refusal for the %s", name, created, err, tc.scope)
		}
		if n := len(f.store.uploads); n != 2 {
			t.Errorf("%s: %d uploads are stored, want the 2 of the rivals and not this one", name, n)
		}
	}
}

// slowStore takes its time over every insert, as a database under load does.
type slowStore struct {
	*fakeStore
	wait time.Duration
}

func (s slowStore) Create(ctx context.Context, in docstore.NewUpload) (docstore.Upload, bool, error) {
	select {
	case <-time.After(s.wait):
	case <-ctx.Done():
		return docstore.Upload{}, false, ctx.Err()
	}
	return s.fakeStore.Create(ctx, in)
}

// Forty small archives a few milliseconds apart, for a limit of ten and an insert that takes a while: the count that the
// service makes sees only what is stored, and every upload that is under way passed it, so that fourteen were
// stored with a fast insert and twenty-nine with a slow one. With the store counting in the transaction that
// stores, ten are, and the rest are refused.
func TestUploadsThatArriveWhileOthersAreBeingStoredDoNotPassTheLimit(t *testing.T) {
	f := newFixture(t)
	reader := &scriptReader{fn: readsAs(1)}
	svc, err := New(Options{Store: slowStore{f.store, 200 * time.Millisecond}, Notices: f.notices, Reader: reader,
		Limits: testLimits(), Retention: 90 * 24 * time.Hour, MaxUploadsPerNotice: 10})
	if err != nil {
		t.Fatal(err)
	}
	var wg sync.WaitGroup
	var mu sync.Mutex
	outcomes := map[string]int{}
	for i := 0; i < 40; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			archive := zipOf(t, file("a.txt", fmt.Sprintf("archive number %d", i)))
			var err error
			var created bool
			for try := 0; try < 200; try++ { // the slots for reading are two, and a busy one is tried again
				_, created, err = svc.Upload(context.Background(), UploadRequest{OrgID: testOrg, NoticeID: testNotice, Archive: archive})
				if !errors.Is(err, ErrBusy) {
					break
				}
				time.Sleep(2 * time.Millisecond)
			}
			var over *QuotaError
			key := "stored"
			switch {
			case errors.As(err, &over):
				key = "refused " + over.Scope
			case err != nil:
				key = "failed: " + err.Error()
			case !created:
				key = "found"
			}
			mu.Lock()
			outcomes[key]++
			mu.Unlock()
		}()
		time.Sleep(5 * time.Millisecond)
	}
	wg.Wait()

	f.store.mu.Lock()
	stored := len(f.store.uploads)
	f.store.mu.Unlock()
	if stored != 10 || outcomes["stored"] != 10 || outcomes["refused notice"] != 30 {
		t.Errorf("%d stored, outcomes %v; want ten stored and thirty refused for the notice", stored, outcomes)
	}
}
