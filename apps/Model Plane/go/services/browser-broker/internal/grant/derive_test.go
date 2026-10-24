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

// storeFactory returns a Repository and the fake clock it reads, so one
// contract runs against MemoryRepository here and against Routed{PgStore,
// MemoryRepository} in postgres_integration_test.go.
type storeFactory func(t *testing.T) (Repository, *fakeClock)

func memoryStore(*testing.T) (Repository, *fakeClock) {
	store, clock := clockedRepo()
	return store, clock
}

// deriveContract is the store-agnostic Derive and approval semantics of WS1.B.
var deriveContract = []struct {
	name string
	run  func(*testing.T, storeFactory)
}{
	{"Derive_InheritsHostsZdrOwnerScopeAndBindsToParent", contractDerive_InheritsHostsZdrOwnerScopeAndBindsToParent},
	{"Derive_ExplicitHostsMayNarrowToTheParentOrBelowIt", contractDerive_ExplicitHostsMayNarrowToTheParentOrBelowIt},
	{"Derive_NeverWidensTheParentsHosts", contractDerive_NeverWidensTheParentsHosts},
	{"Derive_WideParentMustBeNarrowedExplicitly", contractDerive_WideParentMustBeNarrowedExplicitly},
	{"Derive_ParentMustBeAnActiveOrdinarySessionGrant", contractDerive_ParentMustBeAnActiveOrdinarySessionGrant},
	{"Derive_IsScopedToTheParentsTenantAndOwner", contractDerive_IsScopedToTheParentsTenantAndOwner},
	{"Derive_ZdrFollowsTheParentAndIsNeverLowered", contractDerive_ZdrFollowsTheParentAndIsNeverLowered},
	{"Derive_ExpiryIsCappedByTheParentAndAShortRemainderIsRefused", contractDerive_ExpiryIsCappedByTheParentAndAShortRemainderIsRefused},
	{"Derive_ReplayReturnsTheSameGrantAndCannotWidenOrRefreshIt", contractDerive_ReplayReturnsTheSameGrantAndCannotWidenOrRefreshIt},
	{"Derive_RevokingTheParentRevokesTheRunGrant", contractDerive_RevokingTheParentRevokesTheRunGrant},
	{"Derive_ConcurrentParentRevokeNeverLeavesALiveChild", contractDerive_ConcurrentParentRevokeNeverLeavesALiveChild},
	{"Derive_IsBoundedPerParentAndDoesNotConsumeTheOwnerCap", contractDerive_IsBoundedPerParentAndDoesNotConsumeTheOwnerCap},
	{"Derive_RechecksTheHostPolicy", contractDerive_RechecksTheHostPolicy},
	{"SensitiveApproval_AcceptsASessionOrRunParent", contractSensitiveApproval_AcceptsASessionOrRunParent},
	{"Approval_OnRunParent_CannotWidenHosts", contractApproval_OnRunParent_CannotWidenHosts},
	{"Approval_CannotOutliveParent", contractApproval_CannotOutliveParent},
}

func runDeriveContract(t *testing.T, mk storeFactory) {
	for _, c := range deriveContract {
		t.Run(c.name, func(t *testing.T) { c.run(t, mk) })
	}
}

func TestDeriveContract_Memory(t *testing.T) { runDeriveContract(t, memoryStore) }

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

func contractDerive_InheritsHostsZdrOwnerScopeAndBindsToParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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

func contractDerive_ExplicitHostsMayNarrowToTheParentOrBelowIt(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"example.com", "example.org"}})
	run, err := store.Derive(ctx, deriveParams(parent, "k", " API.Example.com ", "example.org"))
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"api.example.com", "example.org"}; !sameStrings(run.AllowedDomains, want) {
		t.Fatalf("domains = %v, want %v", run.AllowedDomains, want)
	}
}

func contractDerive_NeverWidensTheParentsHosts(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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

func contractDerive_WideParentMustBeNarrowedExplicitly(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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
// a non-empty host list of at most the run bound. human_browse (WS8.C) is that
// kind; this test re-labels an allowlist parent as one to pin the rule on the
// allowlist path (the open-web path is in humanbrowse_test.go).
func TestDerive_NonSessionDerivableKindRequiresExplicitHosts(t *testing.T) {
	const humanBrowse = KindHumanBrowse

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

func contractDerive_ParentMustBeAnActiveOrdinarySessionGrant(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
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

func contractDerive_IsScopedToTheParentsTenantAndOwner(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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

func contractDerive_ZdrFollowsTheParentAndIsNeverLowered(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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

func contractDerive_ExpiryIsCappedByTheParentAndAShortRemainderIsRefused(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
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

func contractDerive_ReplayReturnsTheSameGrantAndCannotWidenOrRefreshIt(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
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

func contractDerive_RevokingTheParentRevokesTheRunGrant(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
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
// parent: either the derive fails or the cascade catches its child. The
// interleaving that matters (parent read, then revoked, then insert) is
// forced through the betweenLocks hook, so it is checked on every run rather
// than only when the scheduler happens to produce it.
func TestDerive_ParentRevokedBetweenReadAndInsertLeavesNoChild(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	store.betweenLocks = func() {
		if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
			t.Errorf("revoke: %v", err)
		}
	}
	if run, err := store.Derive(ctx, deriveParams(parent, "k")); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("derive across a revoke = %#v, %v; want ErrGrantRevoked", run, err)
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	for id, g := range store.grants {
		if g.Kind == KindRun {
			t.Fatalf("run grant %s was inserted under a revoked parent", id)
		}
	}
}

func TestDerive_ParentExpiredBetweenReadAndInsertLeavesNoChild(t *testing.T) {
	store, clock := clockedRepo()
	parent := session(t, store, CreateSessionParams{TTL: 2 * time.Minute})
	store.betweenLocks = func() { clock.Advance(3 * time.Minute) }
	if _, err := store.Derive(ctx, deriveParams(parent, "k")); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("derive across the parent's expiry: %v, want ErrGrantExpired", err)
	}
}

// The unforced race, as a smoke test. Every iteration must end in one of the
// two legal states, and a failed derive must fail for the revoke and nothing
// else, so a Derive that always failed would not pass.
func contractDerive_ConcurrentParentRevokeNeverLeavesALiveChild(t *testing.T, mk storeFactory) {
	for i := 0; i < 200; i++ {
		store, _ := mk(t)
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
			if !errors.Is(deriveErr, ErrGrantRevoked) {
				t.Fatalf("iteration %d: derive failed for another reason: %v", i, deriveErr)
			}
			continue
		}
		if _, err := store.GetScoped(ctx, run.ID, parent.OrgID, parent.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("iteration %d: run grant outlived its revoked parent (err=%v)", i, err)
		}
	}
	// Without the revoke the same derive succeeds, so the loop above was not
	// passing on a Derive that always fails.
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{})
	if _, err := store.Derive(ctx, deriveParams(parent, "k")); err != nil {
		t.Fatalf("derive without a revoke: %v", err)
	}
}

// Run grants are bounded per parent, not per owner: a burst of AI runs can
// neither lock the user out of opening a browser session nor grow without
// bound under one session.
func contractDerive_IsBoundedPerParentAndDoesNotConsumeTheOwnerCap(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{MaxActivePerOwner: 2})
	p := deriveParams(parent, "one")
	p.MaxActivePerParent = 2
	for _, key := range []string{"one", "two"} {
		p.SessionKey = key
		if _, err := store.Derive(ctx, p); err != nil {
			t.Fatalf("derive %s: %v", key, err)
		}
	}
	p.SessionKey = "three"
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("third live run grant under one parent: error = %v", err)
	}
	// A replay of an issued key is not a new grant and still succeeds.
	p.SessionKey = "one"
	if _, err := store.Derive(ctx, p); err != nil {
		t.Fatalf("replay at the cap: %v", err)
	}
	// Two live run grants did not use the owner's two-session cap.
	if _, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: parent.OrgID, OwnerID: parent.OwnerID, SessionKey: "session-b",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, MaxActivePerOwner: 2,
	}); err != nil {
		t.Fatalf("a second session after two runs: %v", err)
	}
	// Revoking a run grant frees its slot under the parent.
	first, err := store.Derive(ctx, func() DeriveParams { q := p; q.SessionKey = "two"; return q }())
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, first.ID, first.OrgID, first.OwnerID, "run_start_failed"); err != nil {
		t.Fatal(err)
	}
	p.SessionKey = "three"
	if _, err := store.Derive(ctx, p); err != nil {
		t.Fatalf("derive after a run grant was revoked: %v", err)
	}
	// The default bound applies when the caller sets none.
	store2, _ := mk(t)
	parent2 := session(t, store2, CreateSessionParams{})
	for i := 0; i < DefaultMaxActiveRunsPerParent; i++ {
		if _, err := store2.Derive(ctx, deriveParams(parent2, fmt.Sprintf("k%d", i))); err != nil {
			t.Fatalf("derive %d: %v", i, err)
		}
	}
	if _, err := store2.Derive(ctx, deriveParams(parent2, "over")); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("default per-parent bound: error = %v", err)
	}
}

type denyAll struct{}

func (denyAll) CheckSession(context.Context, string, []string) error { return ErrDomainNotGrantable }

func (denyAll) ResolveHumanBrowse(context.Context, string, string) (HumanBrowsePolicy, error) {
	return HumanBrowsePolicy{}, nil
}

func contractDerive_RechecksTheHostPolicy(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{})
	p := deriveParams(parent, "k")
	p.Policy = denyAll{}
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrDomainNotGrantable) {
		t.Fatalf("error = %v", err)
	}
}

func contractSensitiveApproval_AcceptsASessionOrRunParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{})
	run, err := store.Derive(ctx, deriveParams(parent, "run-key"))
	if err != nil {
		t.Fatal(err)
	}
	all := []*Grant{parent, run}
	for _, p := range []*Grant{parent, run} {
		approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
			OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
			AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
			AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: p.ID, TTL: time.Minute,
		})
		if err != nil || approval.ParentGrantID != p.ID {
			t.Fatalf("approval on %s parent: %#v, %v", p.Kind, approval, err)
		}
		all = append(all, approval)
	}
	// Revoking the session cascades through the run grant to its approval.
	if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	for _, g := range all {
		if _, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("grant %s (%s) survived the session revoke: %v", g.ID, g.Kind, err)
		}
	}
}

func approvalOn(p *Grant, domains []string, ttl time.Duration) ApprovalParams {
	return ApprovalParams{
		OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
		AllowedDomains: domains, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: p.ID, TTL: ttl,
	}
}

// An approval is never wider than its parent: a run grant narrowed to one
// host cannot parent an approval for a sibling or an unrelated host.
func contractApproval_OnRunParent_CannotWidenHosts(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com", "example.org"}, TTL: 15 * time.Minute,
	})
	run, err := store.Derive(ctx, deriveParams(parent, "run-key", "api.example.com"))
	if err != nil {
		t.Fatal(err)
	}
	for _, widened := range [][]string{{"unrelated.net"}, {"example.com"}, {"example.org"}, {"api.example.com", "www.example.com"}, {"badapi.example.com"}} {
		if approval, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, widened, time.Minute)); !errors.Is(err, ErrDeriveWidensParent) {
			t.Fatalf("approval on run grant %v for %v = %#v, %v; want ErrDeriveWidensParent", run.AllowedDomains, widened, approval, err)
		}
	}
	for _, within := range [][]string{{"api.example.com"}, {"v1.api.example.com"}} {
		if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, within, time.Minute)); err != nil {
			t.Fatalf("approval for %v inside the run grant: %v", within, err)
		}
	}
	// The same rule holds for a session parent.
	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, []string{"unrelated.net"}, time.Minute)); !errors.Is(err, ErrDeriveWidensParent) {
		t.Fatalf("approval on a session grant widened it: %v", err)
	}
}

func contractApproval_CannotOutliveParent(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	parent := session(t, store, CreateSessionParams{TTL: 2 * time.Minute})
	run, err := store.Derive(ctx, deriveParams(parent, "run-key"))
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range []*Grant{parent, run} {
		approval, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(p, []string{"example.com"}, time.Hour))
		if err != nil {
			t.Fatal(err)
		}
		if approval.ExpiresAt.After(p.ExpiresAt) {
			t.Fatalf("approval expires %v after its %s parent %v", approval.ExpiresAt, p.Kind, p.ExpiresAt)
		}
	}
	// A short TTL is kept as asked.
	short, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, 10*time.Second))
	if err != nil || !short.ExpiresAt.Equal(clock.Now().Add(10*time.Second)) {
		t.Fatalf("short approval = %#v, %v", short, err)
	}
}

func TestApproval_ParentRevokedBetweenReadAndInsertLeavesNoApproval(t *testing.T) {
	store, _ := clockedRepo()
	parent := session(t, store, CreateSessionParams{})
	store.betweenLocks = func() {
		if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
			t.Errorf("revoke: %v", err)
		}
	}
	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, []string{"example.com"}, time.Minute)); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("approval across a revoke: %v, want ErrGrantRevoked", err)
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	for id, g := range store.grants {
		if g.Kind == KindSensitiveApproval {
			t.Fatalf("approval %s was inserted under a revoked parent", id)
		}
	}
}
