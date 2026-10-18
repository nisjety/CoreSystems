package grant

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"
)

// fakeClock is a settable clock for a MemoryRepository.
type fakeClock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *fakeClock) Advance(d time.Duration) {
	c.mu.Lock()
	c.now = c.now.Add(d)
	c.mu.Unlock()
}

func clockedRepo() (*MemoryRepository, *fakeClock) {
	clock := &fakeClock{now: time.Date(2026, 10, 18, 12, 0, 0, 0, time.UTC)}
	store := NewMemoryRepository()
	store.now = clock.Now
	return store, clock
}

func deriveParams(parent *Grant, key string, domains ...string) DeriveParams {
	return DeriveParams{
		OrgID:         parent.OrgID,
		OwnerID:       parent.OwnerID,
		ParentGrantID: parent.ID,
		SessionKey:    key,
		RunDomains:    domains,
		TTL:           10 * time.Minute,
	}
}

func hosts(n int) []string {
	out := make([]string, n)
	for i := range out {
		out[i] = fmt.Sprintf("h%02d.example.com", i)
	}
	return out
}

func TestDerive_InheritsHostsZdrOwnerScopeAndBindsToParent(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com", "api.example.org"}, TTL: 15 * time.Minute,
	})
	run, err := store.Derive(ctx, deriveParams(parent, "browser-run-1"))
	if err != nil {
		t.Fatal(err)
	}
	if run.Kind != KindRun || run.ParentGrantID != parent.ID || run.OwnerID != parent.OwnerID ||
		run.OrgID != parent.OrgID || run.ScopeURL != parent.ScopeURL || run.SessionKey != "browser-run-1" {
		t.Fatalf("run grant = %#v", run)
	}
	if !sameStrings(run.AllowedDomains, parent.AllowedDomains) || run.Zdr || strings.HasPrefix(run.ID, zdrIDPrefix) {
		t.Fatalf("inherited policy = %v zdr=%v id=%q", run.AllowedDomains, run.Zdr, run.ID)
	}
	if len(run.AllowedActions) != 0 {
		t.Fatal("a run grant must carry no sensitive action")
	}
	stored, err := store.GetScoped(ctx, run.ID, parent.OrgID, parent.OwnerID)
	if err != nil || stored.Kind != KindRun {
		t.Fatalf("stored run grant = %#v, %v", stored, err)
	}
}

func TestDerive_ExplicitHostsMayNarrowToTheParentOrBelowIt(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"example.com", "example.org"}})
	run, err := store.Derive(ctx, deriveParams(parent, "k", " API.Example.com ", "example.org"))
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"api.example.com", "example.org"}; !sameStrings(run.AllowedDomains, want) {
		t.Fatalf("domains = %v, want %v", run.AllowedDomains, want)
	}
}

func TestDerive_NeverWidensTheParentsHosts(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"api.example.com"}})
	for _, attempt := range [][]string{
		{"evil.com"},
		{"example.com"},                  // the parent's parent domain is wider
		{"badapi.example.com"},           // no dot boundary
		{"api.example.com.evil.net"},     // the parent host as a prefix
		{"api.example.com", "other.com"}, // one bad host spoils the list
		{"xapi.example.com", "api.example.com"},
	} {
		if _, err := store.Derive(ctx, deriveParams(parent, "k-"+attempt[0], attempt...)); !errors.Is(err, ErrDeriveWidensParent) {
			t.Fatalf("%v: error = %v, want ErrDeriveWidensParent", attempt, err)
		}
	}
	for _, malformed := range [][]string{{"*.api.example.com"}, {"https://api.example.com"}, {"10.0.0.1"}} {
		if _, err := store.Derive(ctx, deriveParams(parent, "m", malformed...)); !errors.Is(err, ErrInvalidDomainPolicy) {
			t.Fatalf("%v: error = %v, want ErrInvalidDomainPolicy", malformed, err)
		}
	}
}

func TestDerive_WideParentMustBeNarrowedExplicitly(t *testing.T) {
	store, _ := clockedRepo()
	wide := hosts(DefaultMaxRunDomains + 1)
	parent := session(t, store, CreateSessionParams{AllowedDomains: wide})
	if _, err := store.Derive(ctx, deriveParams(parent, "inherit")); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("inheriting %d hosts: error = %v", len(wide), err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "all", wide...)); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("naming all %d hosts: error = %v", len(wide), err)
	}
	run, err := store.Derive(ctx, deriveParams(parent, "subset", wide[:DefaultMaxRunDomains]...))
	if err != nil || len(run.AllowedDomains) != DefaultMaxRunDomains {
		t.Fatalf("explicit subset: %v, %v", run, err)
	}
	// A configured bound applies to both the inherited and the explicit list.
	small := session(t, store, CreateSessionParams{SessionKey: "small", AllowedDomains: hosts(3)})
	p := deriveParams(small, "bounded")
	p.MaxRunDomains = 2
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("configured bound on inherit: %v", err)
	}
}

// R-16: a derive from any derivable parent kind other than session must name
// a non-empty host list of at most the run bound. No such kind ships yet
// (human_browse is WS8.C), so the test registers one for its duration.
func TestDerive_NonSessionDerivableKindRequiresExplicitHosts(t *testing.T) {
	const humanBrowse Kind = "human_browse"
	derivableParents[humanBrowse] = derivePolicy{explicitHosts: true}
	t.Cleanup(func() { delete(derivableParents, humanBrowse) })

	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"example.com"}})
	store.mu.Lock()
	stored := store.grants[parent.ID]
	stored.Kind = humanBrowse
	store.mu.Unlock()

	if _, err := store.Derive(ctx, deriveParams(parent, "empty")); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("implicit hosts from %s: error = %v", humanBrowse, err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "nine", hosts(DefaultMaxRunDomains+1)...)); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("nine explicit hosts: error = %v", err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "one", "www.example.com")); err != nil {
		t.Fatalf("one explicit host: %v", err)
	}
}

func TestDerive_ParentMustBeAnActiveOrdinarySessionGrant(t *testing.T) {
	store, clock := clockedRepo()
	parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})

	run, err := store.Derive(ctx, deriveParams(parent, "first"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, deriveParams(run, "chained")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("run parent: error = %v, want ErrParentNotDerivable", err)
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: parent.OrgID, OwnerID: parent.OwnerID, SessionKey: parent.SessionKey,
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: parent.ID, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, deriveParams(approval, "from-approval")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("approval parent: error = %v, want ErrParentNotDerivable", err)
	}
	if _, err := store.Derive(ctx, DeriveParams{OrgID: "org-a", OwnerID: "user-a", ParentGrantID: "forged", SessionKey: "k", TTL: time.Minute}); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("unknown parent: error = %v", err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("empty key: error = %v", err)
	}

	revoked := session(t, store, CreateSessionParams{SessionKey: "revoked"})
	if err := store.RevokeScoped(ctx, revoked.ID, revoked.OrgID, revoked.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, deriveParams(revoked, "k")); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("revoked parent: error = %v", err)
	}
	clock.Advance(11 * time.Minute)
	if _, err := store.Derive(ctx, deriveParams(parent, "late")); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("expired parent: error = %v", err)
	}
}

func TestDerive_IsScopedToTheParentsTenantAndOwner(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	for _, tc := range []struct{ name, org, owner string }{
		{"other tenant", "org-b", "user-a"},
		{"other tenant as a service", "org-b", ""},
		{"other user", "org-a", "user-b"},
	} {
		p := deriveParams(parent, "k")
		p.OrgID, p.OwnerID = tc.org, tc.owner
		if _, err := store.Derive(ctx, p); !errors.Is(err, ErrGrantNotFound) {
			t.Fatalf("%s: error = %v, want ErrGrantNotFound", tc.name, err)
		}
	}
	// A scoped service inside the tenant may derive; the run grant still
	// belongs to the parent's owner, never to the caller.
	p := deriveParams(parent, "service-run")
	p.OwnerID = ""
	run, err := store.Derive(ctx, p)
	if err != nil || run.OwnerID != parent.OwnerID {
		t.Fatalf("service derive: %#v, %v", run, err)
	}
}

func TestDerive_ZdrFollowsTheParentAndIsNeverLowered(t *testing.T) {
	store, _ := clockedRepo()
	zdrParent := session(t, store, CreateSessionParams{Zdr: true, SessionKey: "zdr"})
	run, err := store.Derive(ctx, deriveParams(zdrParent, "k"))
	if err != nil {
		t.Fatal(err)
	}
	if !run.Zdr || !strings.HasPrefix(run.ID, zdrIDPrefix) {
		t.Fatalf("ZDR parent gave zdr=%v id=%q", run.Zdr, run.ID)
	}

	plain := session(t, store, CreateSessionParams{SessionKey: "plain"})
	p := deriveParams(plain, "k")
	p.PrincipalZdr = true
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("ZDR caller on a non-ZDR parent: error = %v", err)
	}
}

func TestDerive_ExpiryIsCappedByTheParentAndAShortRemainderIsRefused(t *testing.T) {
	store, clock := clockedRepo()
	parent := session(t, store, CreateSessionParams{TTL: 5 * time.Minute})

	p := deriveParams(parent, "long")
	p.TTL = time.Hour
	run, err := store.Derive(ctx, p)
	if err != nil {
		t.Fatal(err)
	}
	if !run.ExpiresAt.Equal(parent.ExpiresAt) {
		t.Fatalf("run expiry %v outlives parent %v", run.ExpiresAt, parent.ExpiresAt)
	}

	p = deriveParams(parent, "short")
	p.TTL = time.Minute
	run, err = store.Derive(ctx, p)
	if err != nil || !run.ExpiresAt.Equal(clock.Now().Add(time.Minute)) {
		t.Fatalf("short ttl: %v, %v", run, err)
	}

	clock.Advance(5*time.Minute - 20*time.Second)
	if _, err := store.Derive(ctx, deriveParams(parent, "dying")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("20 s left on the parent: error = %v", err)
	}
}

func TestDerive_ReplayReturnsTheSameGrantAndCannotWidenOrRefreshIt(t *testing.T) {
	store, clock := clockedRepo()
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"example.com", "example.org"}, TTL: 30 * time.Minute})
	first, err := store.Derive(ctx, deriveParams(parent, "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	clock.Advance(2 * time.Minute)
	again, err := store.Derive(ctx, deriveParams(parent, "run-1", "EXAMPLE.com"))
	if err != nil {
		t.Fatal(err)
	}
	if again.ID != first.ID || !again.ExpiresAt.Equal(first.ExpiresAt) {
		t.Fatalf("replay changed the grant: %s/%v vs %s/%v", again.ID, again.ExpiresAt, first.ID, first.ExpiresAt)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "run-1", "example.com", "example.org")); !errors.Is(err, ErrDeriveConflict) {
		t.Fatalf("replay with more hosts: error = %v, want ErrDeriveConflict", err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "run-1")); !errors.Is(err, ErrDeriveConflict) {
		t.Fatalf("replay inheriting the parent: error = %v, want ErrDeriveConflict", err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "run-1", "evil.com")); !errors.Is(err, ErrDeriveWidensParent) {
		t.Fatalf("replay outside the parent: error = %v", err)
	}
	other, err := store.Derive(ctx, deriveParams(parent, "run-2", "example.com"))
	if err != nil || other.ID == first.ID {
		t.Fatalf("a new key must mint a new grant: %v, %v", other, err)
	}

	// A revoked run grant is not resurrected by replaying its key.
	if err := store.RevokeScoped(ctx, first.ID, parent.OrgID, parent.OwnerID, "run_finished"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, deriveParams(parent, "run-1", "example.com")); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("replay of a revoked run grant: error = %v", err)
	}
	// Nor is an expired one.
	short := deriveParams(parent, "run-3")
	short.TTL = time.Minute
	if _, err := store.Derive(ctx, short); err != nil {
		t.Fatal(err)
	}
	clock.Advance(2 * time.Minute)
	if _, err := store.Derive(ctx, deriveParams(parent, "run-3")); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("replay of an expired run grant: error = %v", err)
	}
}

func TestDerive_RevokingTheParentRevokesTheRunGrant(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	run, err := store.Derive(ctx, deriveParams(parent, "k"))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, run.ID, parent.OrgID, parent.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("run grant after parent revoke: error = %v", err)
	}
}

// A revoke racing a derive must never leave a live run grant under a revoked
// parent: either the derive fails or the cascade catches its child.
func TestDerive_ConcurrentParentRevokeNeverLeavesALiveChild(t *testing.T) {
	for i := 0; i < 200; i++ {
		store := NewMemoryRepository()
		parent := session(t, store, CreateSessionParams{})
		var wg sync.WaitGroup
		var run *Grant
		var deriveErr error
		wg.Add(2)
		go func() {
			defer wg.Done()
			run, deriveErr = store.Derive(ctx, deriveParams(parent, "k"))
		}()
		go func() {
			defer wg.Done()
			_ = store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed")
		}()
		wg.Wait()
		if deriveErr != nil {
			continue
		}
		if _, err := store.GetScoped(ctx, run.ID, parent.OrgID, parent.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("iteration %d: run grant outlived its revoked parent (err=%v)", i, err)
		}
	}
}

func TestDerive_CountsTowardThePerOwnerCap(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	p := deriveParams(parent, "one")
	p.MaxActivePerOwner = 2
	if _, err := store.Derive(ctx, p); err != nil {
		t.Fatal(err)
	}
	p.SessionKey = "two"
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("third active grant: error = %v", err)
	}
	// A replay of an issued key is not a new grant and still succeeds.
	p.SessionKey = "one"
	if _, err := store.Derive(ctx, p); err != nil {
		t.Fatalf("replay at the cap: %v", err)
	}
}

type denyAll struct{}

func (denyAll) CheckSession(context.Context, string, []string) error { return ErrDomainNotGrantable }

func TestDerive_RechecksTheHostPolicy(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	p := deriveParams(parent, "k")
	p.Policy = denyAll{}
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrDomainNotGrantable) {
		t.Fatalf("error = %v", err)
	}
}

func TestSensitiveApproval_AcceptsASessionOrRunParent(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	run, err := store.Derive(ctx, deriveParams(parent, "run-key"))
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range []*Grant{parent, run} {
		approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
			OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
			AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
			AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: p.ID, TTL: time.Minute,
		})
		if err != nil || approval.ParentGrantID != p.ID {
			t.Fatalf("approval on %s parent: %#v, %v", p.Kind, approval, err)
		}
	}
	// Revoking the session cascades through the run grant to its approval.
	if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	for id, g := range store.grants {
		if !g.Revoked {
			t.Fatalf("grant %s (%s) survived the session revoke", id, g.Kind)
		}
	}
}
