package grant

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"
)

// recordingRepo stands in for the durable store: it records every id and
// every create it is handed, and fails the test the moment a ZDR value
// reaches it.
type recordingRepo struct {
	t     *testing.T
	inner *MemoryRepository
	mu    sync.Mutex
	calls []string
}

func newRecordingRepo(t *testing.T) *recordingRepo {
	return &recordingRepo{t: t, inner: NewMemoryRepository()}
}

func (r *recordingRepo) record(op, id string, zdr bool) {
	r.mu.Lock()
	r.calls = append(r.calls, op+":"+id)
	r.mu.Unlock()
	if IsZdrID(id) || zdr {
		r.t.Errorf("the persistent repository received a ZDR %s (id %q, zdr=%v)", op, id, zdr)
	}
}

func (r *recordingRepo) count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.calls)
}

func (r *recordingRepo) CreateSession(ctx context.Context, p CreateSessionParams) (*Grant, error) {
	r.record("create_session", "", p.Zdr)
	return r.inner.CreateSession(ctx, p)
}

func (r *recordingRepo) CreateScopedSensitiveApproval(ctx context.Context, p ApprovalParams) (*Grant, error) {
	r.record("create_approval", p.ParentGrantID, p.Zdr)
	return r.inner.CreateScopedSensitiveApproval(ctx, p)
}

func (r *recordingRepo) CreateOriginExtension(ctx context.Context, p ExtensionParams) (*Grant, error) {
	r.record("create_extension", p.ParentGrantID, p.Zdr)
	return r.inner.CreateOriginExtension(ctx, p)
}

func (r *recordingRepo) Derive(ctx context.Context, p DeriveParams) (*Grant, error) {
	r.record("derive", p.ParentGrantID, false)
	return r.inner.Derive(ctx, p)
}

func (r *recordingRepo) CreateHumanBrowse(ctx context.Context, p CreateHumanBrowseParams) (*Grant, error) {
	r.record("create_human_browse", "", p.Zdr)
	return r.inner.CreateHumanBrowse(ctx, p)
}

func (r *recordingRepo) Renew(ctx context.Context, p RenewParams) (*Grant, error) {
	r.record("renew", p.GrantID, false)
	return r.inner.Renew(ctx, p)
}

func (r *recordingRepo) GetScoped(ctx context.Context, id, orgID, ownerID string) (*Grant, error) {
	r.record("get", id, false)
	return r.inner.GetScoped(ctx, id, orgID, ownerID)
}

func (r *recordingRepo) RevokeScoped(ctx context.Context, id, orgID, ownerID, reason string) error {
	r.record("revoke", id, false)
	return r.inner.RevokeScoped(ctx, id, orgID, ownerID, reason)
}

func TestRouted_ZdrNeverTouchesPersistentRepo(t *testing.T) {
	persistent := newRecordingRepo(t)
	ephemeral := NewMemoryRepository()
	routed := NewRouted(persistent, ephemeral)

	zdr := session(t, routed, CreateSessionParams{Zdr: true, AllowedDomains: []string{"example.com"}})
	if !IsZdrID(zdr.ID) || !zdr.Zdr {
		t.Fatalf("ZDR session = %#v", zdr)
	}
	if _, err := routed.GetScoped(ctx, zdr.ID, zdr.OrgID, zdr.OwnerID); err != nil {
		t.Fatalf("get ZDR session: %v", err)
	}
	run, err := routed.Derive(ctx, deriveParams(zdr, "run-1"))
	if err != nil || !IsZdrID(run.ID) {
		t.Fatalf("derive from ZDR session: %#v, %v", run, err)
	}
	if _, err := routed.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute)); err != nil {
		t.Fatalf("approval on ZDR run grant: %v", err)
	}
	if err := routed.RevokeScoped(ctx, zdr.ID, zdr.OrgID, zdr.OwnerID, "session_closed"); err != nil {
		t.Fatalf("revoke ZDR session: %v", err)
	}
	if _, err := routed.GetScoped(ctx, run.ID, zdr.OrgID, zdr.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("ZDR run grant after its session was revoked: %v", err)
	}
	if n := persistent.count(); n != 0 {
		t.Fatalf("persistent repository saw %d calls for ZDR grants: %v", n, persistent.calls)
	}

	// A non-ZDR session does go to the persistent store, and only there.
	plain := session(t, routed, CreateSessionParams{SessionKey: "plain"})
	if IsZdrID(plain.ID) || persistent.count() != 1 {
		t.Fatalf("plain session id %q, persistent calls %v", plain.ID, persistent.calls)
	}
	if _, err := ephemeral.GetScoped(ctx, plain.ID, plain.OrgID, plain.OwnerID); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("a non-ZDR grant leaked into the ephemeral store: %v", err)
	}
}

func TestRouted_ZPrefixIdNeverReachesPostgres(t *testing.T) {
	persistent := newRecordingRepo(t)
	routed := NewRouted(persistent, NewMemoryRepository())
	forged := zdrIDPrefix + strings.Repeat("ab", 16)

	if _, err := routed.GetScoped(ctx, forged, "org-a", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("get of an unknown z_ id: %v", err)
	}
	if err := routed.RevokeScoped(ctx, forged, "org-a", "user-a", "x"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("revoke of an unknown z_ id: %v", err)
	}
	if _, err := routed.Derive(ctx, DeriveParams{OrgID: "org-a", OwnerID: "user-a", ParentGrantID: forged, SessionKey: "k", TTL: time.Minute}); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("derive from an unknown z_ parent: %v", err)
	}
	if _, err := routed.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: "org-a", OwnerID: "user-a", ParentGrantID: forged, SessionKey: "s",
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, TTL: time.Minute,
	}); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("approval on an unknown z_ parent: %v", err)
	}
	if n := persistent.count(); n != 0 {
		t.Fatalf("a z_ id reached the persistent repository: %v", persistent.calls)
	}

	// The durable store refuses on its own as well (defence in depth): no
	// pool is needed because it answers before any query.
	pg := &PgStore{now: time.Now}
	if _, err := pg.GetScoped(ctx, forged, "org-a", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("PgStore.GetScoped(z_): %v", err)
	}
	if err := pg.RevokeScoped(ctx, forged, "org-a", "user-a", "x"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("PgStore.RevokeScoped(z_): %v", err)
	}
	if _, err := pg.Derive(ctx, DeriveParams{OrgID: "org-a", ParentGrantID: forged, SessionKey: "k"}); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("PgStore.Derive(z_ parent): %v", err)
	}
	if _, err := pg.CreateSession(ctx, CreateSessionParams{Zdr: true, AllowedDomains: []string{"example.com"}}); !errors.Is(err, ErrZdrNotPersistable) {
		t.Fatalf("PgStore.CreateSession(zdr): %v", err)
	}
	if _, err := pg.CreateScopedSensitiveApproval(ctx, ApprovalParams{ParentGrantID: "abc", Zdr: true}); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("PgStore.CreateScopedSensitiveApproval(zdr): %v", err)
	}
}

func TestRouted_DeriveRoutesByParentPrefix(t *testing.T) {
	persistent := newRecordingRepo(t)
	ephemeral := NewMemoryRepository()
	routed := NewRouted(persistent, ephemeral)

	plain := session(t, routed, CreateSessionParams{SessionKey: "plain"})
	zdr := session(t, routed, CreateSessionParams{SessionKey: "zdr", Zdr: true})
	before := persistent.count()

	plainRun, err := routed.Derive(ctx, deriveParams(plain, "run"))
	if err != nil || IsZdrID(plainRun.ID) || plainRun.Zdr {
		t.Fatalf("derive from a persistent parent: %#v, %v", plainRun, err)
	}
	if persistent.count() != before+1 || persistent.calls[len(persistent.calls)-1] != "derive:"+plain.ID {
		t.Fatalf("persistent calls = %v", persistent.calls)
	}
	zdrRun, err := routed.Derive(ctx, deriveParams(zdr, "run"))
	if err != nil || !IsZdrID(zdrRun.ID) || !zdrRun.Zdr {
		t.Fatalf("derive from a ZDR parent: %#v, %v", zdrRun, err)
	}
	if persistent.count() != before+1 {
		t.Fatalf("a ZDR derive reached the persistent store: %v", persistent.calls)
	}
	if _, err := ephemeral.GetScoped(ctx, zdrRun.ID, zdr.OrgID, zdr.OwnerID); err != nil {
		t.Fatalf("ZDR run grant is not in the ephemeral store: %v", err)
	}
	// Each run grant is found through the router by its own prefix.
	for _, g := range []*Grant{plainRun, zdrRun} {
		if got, err := routed.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); err != nil || got.ID != g.ID {
			t.Fatalf("routed get %s: %#v, %v", g.ID, got, err)
		}
	}
	// A ZDR caller deriving from a persistent parent is refused, not moved.
	p := deriveParams(plain, "zdr-caller")
	p.PrincipalZdr = true
	if _, err := routed.Derive(ctx, p); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("ZDR caller on a persistent parent: %v", err)
	}
}

func TestRouted_PurgeExpiredCoversBothStores(t *testing.T) {
	ephemeral, clock := clockedRepo()
	persistent, _ := clockedRepo()
	persistent.now = clock.Now
	routed := NewRouted(persistent, ephemeral)
	zdr := session(t, routed, CreateSessionParams{Zdr: true})
	run, err := routed.Derive(ctx, deriveParams(zdr, "run"))
	if err != nil {
		t.Fatal(err)
	}
	plain := session(t, routed, CreateSessionParams{SessionKey: "plain"})
	clock.Advance(time.Hour)
	live := session(t, routed, CreateSessionParams{SessionKey: "live"})
	n, err := routed.PurgeExpired(ctx, clock.Now().Add(-time.Minute))
	if err != nil || n != 3 {
		t.Fatalf("purged %d, %v; want 3", n, err)
	}
	for _, g := range []*Grant{zdr, run, plain} {
		if _, err := routed.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); !errors.Is(err, ErrGrantNotFound) {
			t.Fatalf("%s survived the purge: %v", g.ID, err)
		}
	}
	if _, err := routed.GetScoped(ctx, live.ID, live.OrgID, live.OwnerID); err != nil {
		t.Fatalf("a live grant was purged: %v", err)
	}
}
