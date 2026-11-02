package rawstore

import (
	"context"
	"errors"
	"testing"
)

// New(nil) has no pool. Any SQL would panic, so each refusal also proves that the store refused
// before it reached the database.

func TestRecentNoticesRefusesALimitOutsideTheBounds(t *testing.T) {
	t.Parallel()

	for _, limit := range []int{0, -1, MaxRecent + 1} {
		err := New(nil).RecentNotices(context.Background(), limit, func(Document) error { return nil })
		if !errors.Is(err, ErrInvalidQuery) {
			t.Errorf("limit %d: err = %v, want ErrInvalidQuery", limit, err)
		}
	}
}

func TestNoticeRefusesAnIdThatIsNotANoticeID(t *testing.T) {
	t.Parallel()

	for _, id := range []string{"", "2026", "2026-1", "2026-1234567890", "x2026-104837", "2026-104837; DROP TABLE raw_documents", "../etc/passwd"} {
		if _, err := New(nil).Notice(context.Background(), id); !errors.Is(err, ErrInvalidQuery) {
			t.Errorf("id %q: err = %v, want ErrInvalidQuery", id, err)
		}
	}
}
