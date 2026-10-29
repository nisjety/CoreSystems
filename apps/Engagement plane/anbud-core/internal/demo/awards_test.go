package demo

import (
	"context"
	"testing"

	"github.com/triodelab/anbud/internal/rawstore"
)

// allNotices is a store that can read every body, as the raw store can.
type allNotices struct {
	fakeNotices
	allCalls int
}

func (f *allNotices) AllNotices(_ context.Context, fn func(rawstore.Document) error) error {
	f.allCalls++
	for _, d := range f.docs {
		if err := fn(d); err != nil {
			return err
		}
	}
	return nil
}

// A suggestion reads the whole store when the store can, since an organisation's wins are not all in the
// newest rows. A store that can only read the newest is asked for the newest MaxScan, not for nothing.
func TestAwardsReadEveryStoredNoticeWhenTheStoreCanAndTheNewestWhenNot(t *testing.T) {
	t.Parallel()

	all := &allNotices{}
	svc, err := New(Options{Notices: all})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	if _, err := svc.Awards(context.Background()); err != nil {
		t.Fatalf("Awards: %v", err)
	}
	if all.allCalls != 1 || len(all.limits) != 0 {
		t.Errorf("all-notices calls = %d, recent calls with limits %v: want the whole store read once", all.allCalls, all.limits)
	}

	newest := &fakeNotices{}
	svc, err = New(Options{Notices: newest})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	if _, err := svc.Awards(context.Background()); err != nil {
		t.Fatalf("Awards: %v", err)
	}
	if len(newest.limits) != 1 || newest.limits[0] != MaxScan {
		t.Errorf("recent limits = %v, want one read of %d", newest.limits, MaxScan)
	}
}
