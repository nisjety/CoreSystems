package browsersites

import (
	"context"
	"errors"
	"testing"
	"time"
)

// erasureStore records the erasure calls the service makes, so the tests see what the
// service passes on and what it refuses before any write.
type erasureStore struct {
	floorStore
	calls   []string
	orgID   string
	userID  string
	prefs   int64
	history int64
	err     error
}

func (e *erasureStore) DeleteUserData(_ context.Context, orgID, userID string) (int64, int64, error) {
	e.calls = append(e.calls, "DeleteUserData")
	e.orgID, e.userID = orgID, userID
	return e.prefs, e.history, e.err
}

func (e *erasureStore) DeleteRemovedMemberData(_ context.Context, orgID, userID string) (int64, int64, error) {
	e.calls = append(e.calls, "DeleteRemovedMemberData")
	e.orgID, e.userID = orgID, userID
	return e.prefs, e.history, e.err
}

// TestErasureRefusesBlankIdsAndTheTombstoneBeforeAnyWrite pins the validation the erasure
// hooks share: a blank organisation or user id, and the history tombstone as a user id, are
// refused and the store is never asked.
func TestErasureRefusesBlankIdsAndTheTombstoneBeforeAnyWrite(t *testing.T) {
	store := &erasureStore{prefs: 9, history: 9}
	service := NewService(store, time.Now)
	cases := []struct {
		name, orgID, userID string
		want                error
	}{
		{"blank organisation", "  ", "user-1", nil},
		{"blank user", "org-1", "   ", nil},
		{"tombstone as user", "org-1", erasedTombstone, ErrReservedUserID},
		{"padded tombstone", "org-1", "  erased  ", ErrReservedUserID},
	}
	for _, tc := range cases {
		for _, method := range []string{"DeleteUserData", "DeleteRemovedMemberData"} {
			var err error
			if method == "DeleteUserData" {
				_, _, err = service.DeleteUserData(context.Background(), tc.orgID, tc.userID)
			} else {
				_, _, err = service.DeleteRemovedMemberData(context.Background(), tc.orgID, tc.userID)
			}
			if err == nil {
				t.Fatalf("%s via %s: want an error", tc.name, method)
			}
			if tc.want != nil && !errors.Is(err, tc.want) {
				t.Fatalf("%s via %s: want %v, got %v", tc.name, method, tc.want, err)
			}
		}
	}
	if len(store.calls) != 0 {
		t.Fatalf("a refused erasure must not reach the store, got calls %v", store.calls)
	}
}

// TestErasureTrimsTheIdsAndReturnsTheStoreCounts pins that the hooks pass the trimmed ids
// and return the counts the store reports, and nothing else.
func TestErasureTrimsTheIdsAndReturnsTheStoreCounts(t *testing.T) {
	store := &erasureStore{prefs: 3, history: 7}
	service := NewService(store, time.Now)

	prefs, history, err := service.DeleteUserData(context.Background(), " org-1 ", " user-1 ")
	if err != nil || prefs != 3 || history != 7 {
		t.Fatalf("DeleteUserData: prefs=%d history=%d err=%v; want 3, 7, nil", prefs, history, err)
	}
	if store.orgID != "org-1" || store.userID != "user-1" {
		t.Fatalf("the store must get the trimmed ids, got org=%q user=%q", store.orgID, store.userID)
	}

	prefs, history, err = service.DeleteRemovedMemberData(context.Background(), "org-2", "user-2")
	if err != nil || prefs != 3 || history != 7 {
		t.Fatalf("DeleteRemovedMemberData: prefs=%d history=%d err=%v; want 3, 7, nil", prefs, history, err)
	}
	if got := store.calls; len(got) != 2 || got[0] != "DeleteUserData" || got[1] != "DeleteRemovedMemberData" {
		t.Fatalf("calls = %v; want DeleteUserData then DeleteRemovedMemberData", got)
	}
}

// TestErasureReturnsAStoreFailureUnchanged pins that a failed erase is an error to the
// caller, never a count of zero, so the caller can retry it.
func TestErasureReturnsAStoreFailureUnchanged(t *testing.T) {
	boom := errors.New("database unavailable")
	service := NewService(&erasureStore{err: boom}, time.Now)
	if _, _, err := service.DeleteUserData(context.Background(), "org-1", "user-1"); !errors.Is(err, boom) {
		t.Fatalf("DeleteUserData must return the store failure, got %v", err)
	}
	if _, _, err := service.DeleteRemovedMemberData(context.Background(), "org-1", "user-1"); !errors.Is(err, boom) {
		t.Fatalf("DeleteRemovedMemberData must return the store failure, got %v", err)
	}
}
