package lease

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

func TestAcquireThreadRefusesAServicePrincipalWithoutTouchingTheDatabase(t *testing.T) {
	s, db := newTestStore(notFoundRow())
	_, err := s.AcquireThread(context.Background(), "thread-1", "org-1", "", "backend-1", time.Minute)
	if !errors.Is(err, ErrThreadLeaseOwnerRequired) {
		t.Fatalf("err = %v, want ErrThreadLeaseOwnerRequired for an empty owner", err)
	}
	if db.queryRowCalls != 0 || db.execCount() != 0 {
		t.Fatalf("a refused thread acquire reached the database (queryRow %d, exec %d)", db.queryRowCalls, db.execCount())
	}
}

func TestAcquireThreadUpsertsOnTheLiveThreadIndex(t *testing.T) {
	want := testLease(mpv1.SandboxLifecycleState_SCRATCH, "", "backend-1", time.Now().Add(time.Hour))
	want.ScopeType = "thread"
	want.ScopeID = "thread-1"
	want.OrgID = "org-1"
	want.OwnerID = "user-1"
	s, db := newTestStore(rowFor(want))

	got, err := s.AcquireThread(context.Background(), "thread-1", "org-1", "user-1", "backend-1", time.Minute)
	if err != nil {
		t.Fatalf("AcquireThread: %v", err)
	}
	if got.ID != want.ID {
		t.Fatalf("lease id = %s, want the existing row's %s: a retried acquire must not mint a second lease", got.ID, want.ID)
	}
	if db.queryRowCalls != 1 || db.execCount() != 0 {
		t.Fatalf("expected one upsert statement, got queryRow %d exec %d", db.queryRowCalls, db.execCount())
	}
	query := db.lastQueryRowQuery
	for _, must := range []string{
		"ON CONFLICT (org_id, owner_id, scope_id) WHERE scope_type = 'thread' AND state <> 4",
		"GREATEST(leases.expires_at, EXCLUDED.expires_at)",
		"'', $6, false, 0",
	} {
		if !strings.Contains(query, must) {
			t.Fatalf("upsert is missing %q:\n%s", must, query)
		}
	}
}
