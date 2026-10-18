package grant

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

var ctx = context.Background()

func session(t *testing.T, store Repository, p CreateSessionParams) *Grant {
	t.Helper()
	if p.OrgID == "" {
		p.OrgID = "org-a"
	}
	if p.OwnerID == "" {
		p.OwnerID = "user-a"
	}
	if p.SessionKey == "" {
		p.SessionKey = "session-a"
	}
	if p.ScopeURL == "" {
		p.ScopeURL = "browser://cloud"
	}
	if p.AllowedDomains == nil {
		p.AllowedDomains = []string{"example.com"}
	}
	if p.TTL == 0 {
		p.TTL = time.Minute
	}
	g, err := store.CreateSession(ctx, p)
	if err != nil {
		t.Fatal(err)
	}
	return g
}

func TestStoreScopesCopiesAndRevokesGrants(t *testing.T) {
	store := NewMemoryRepository()
	created := session(t, store, CreateSessionParams{})
	created.OrgID = "attacker"

	for _, tc := range []struct{ name, org, owner string }{
		{name: "wrong org", org: "org-b", owner: "user-a"},
		{name: "wrong owner", org: "org-a", owner: "user-b"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := store.GetScoped(ctx, created.ID, tc.org, tc.owner); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("error = %v", err)
			}
			if err := store.RevokeScoped(ctx, created.ID, tc.org, tc.owner, "x"); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("revoke error = %v", err)
			}
		})
	}

	visible, err := store.GetScoped(ctx, created.ID, "org-a", "")
	if err != nil {
		t.Fatal(err)
	}
	if visible.OrgID != "org-a" || visible.OwnerID != "user-a" {
		t.Fatalf("stored grant mutated: %#v", visible)
	}
	visible.OwnerID = "attacker"
	visibleAgain, err := store.GetScoped(ctx, created.ID, "org-a", "user-a")
	if err != nil || visibleAgain.OwnerID != "user-a" {
		t.Fatalf("returned grant was not copied: %#v, %v", visibleAgain, err)
	}

	if err := store.RevokeScoped(ctx, created.ID, "org-a", "user-a", "session_closed"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, created.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("error = %v, want revoked", err)
	}
}

func TestStoreReportsExpiredAndUnknownGrants(t *testing.T) {
	store := NewMemoryRepository()
	expired := session(t, store, CreateSessionParams{TTL: -time.Second})
	if _, err := store.GetScoped(ctx, expired.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("error = %v, want expired", err)
	}
	if _, err := store.GetScoped(ctx, "missing", "org-a", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("error = %v", err)
	}
}

func TestNormalizeAllowedDomainsRejectsUnboundedOrNonHostPolicy(t *testing.T) {
	for _, domains := range [][]string{
		nil,
		{},
		{""},
		{"https://example.com"},
		{"example.com/path"},
		{"*.example.com"},
		{"127.0.0.1"},
		{"localhost"},
	} {
		if _, err := NormalizeAllowedDomains(domains); !errors.Is(err, ErrInvalidDomainPolicy) {
			t.Fatalf("NormalizeAllowedDomains(%#v) error = %v, want ErrInvalidDomainPolicy", domains, err)
		}
	}
}

func TestStoreCopiesCanonicalAllowedDomains(t *testing.T) {
	store := NewMemoryRepository()
	created := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"EXAMPLE.com", "api.example.com", "example.com"},
	})
	if got, want := created.AllowedDomains, []string{"api.example.com", "example.com"}; !sameStrings(got, want) {
		t.Fatalf("created policy = %#v, want %#v", got, want)
	}
	created.AllowedDomains[0] = "attacker.example"
	stored, err := store.GetScoped(ctx, created.ID, "org-a", "user-a")
	if err != nil {
		t.Fatal(err)
	}
	if got, want := stored.AllowedDomains, []string{"api.example.com", "example.com"}; !sameStrings(got, want) {
		t.Fatalf("stored policy = %#v, want %#v", got, want)
	}
}

func TestCreateSession_IssuesSessionKindWithZdrPrefixOnlyForZdr(t *testing.T) {
	store := NewMemoryRepository()
	plain := session(t, store, CreateSessionParams{})
	if plain.Kind != KindSession || plain.Zdr || strings.HasPrefix(plain.ID, "z_") || len(plain.ID) != 32 {
		t.Fatalf("plain grant = %#v", plain)
	}
	zdr := session(t, store, CreateSessionParams{Zdr: true})
	if !zdr.Zdr || !strings.HasPrefix(zdr.ID, "z_") || len(zdr.ID) != 34 {
		t.Fatalf("zdr grant = %#v", zdr)
	}
}

func TestCreateSession_PerOwnerActiveCapCountsOnlyLiveGrantsOfThatOwner(t *testing.T) {
	store := NewMemoryRepository()
	var first *Grant
	for i := 0; i < 3; i++ {
		g := session(t, store, CreateSessionParams{MaxActivePerOwner: 3})
		if first == nil {
			first = g
		}
	}
	if _, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "s", ScopeURL: "u",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, MaxActivePerOwner: 3,
	}); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("fourth grant error = %v, want ErrTooManyGrants", err)
	}
	// Another owner, and another organization with the same owner id, are not counted.
	session(t, store, CreateSessionParams{OwnerID: "user-b", MaxActivePerOwner: 3})
	session(t, store, CreateSessionParams{OrgID: "org-b", MaxActivePerOwner: 3})
	// Revoking one frees a slot.
	if err := store.RevokeScoped(ctx, first.ID, "org-a", "user-a", "session_closed"); err != nil {
		t.Fatal(err)
	}
	session(t, store, CreateSessionParams{MaxActivePerOwner: 3})
}

func TestCreateSession_ExpiredGrantsDoNotCountTowardTheCap(t *testing.T) {
	store := NewMemoryRepository()
	now := time.Now()
	store.now = func() time.Time { return now }
	session(t, store, CreateSessionParams{TTL: time.Minute, MaxActivePerOwner: 1})
	now = now.Add(2 * time.Minute)
	session(t, store, CreateSessionParams{TTL: time.Minute, MaxActivePerOwner: 1})
}

func TestCreateSession_CapIsAtomicUnderConcurrency(t *testing.T) {
	store := NewMemoryRepository()
	results := make(chan error, 40)
	for i := 0; i < 40; i++ {
		go func() {
			_, err := store.CreateSession(ctx, CreateSessionParams{
				OrgID: "org-a", OwnerID: "user-a", SessionKey: "s", ScopeURL: "u",
				AllowedDomains: []string{"example.com"}, TTL: time.Minute, MaxActivePerOwner: 5,
			})
			results <- err
		}()
	}
	created := 0
	for i := 0; i < 40; i++ {
		switch err := <-results; {
		case err == nil:
			created++
		case !errors.Is(err, ErrTooManyGrants):
			t.Fatal(err)
		}
	}
	if created != 5 {
		t.Fatalf("created %d grants, want exactly the cap of 5", created)
	}
}

func TestRevokeScoped_CascadesToSensitiveApprovalsAndRecordsReason(t *testing.T) {
	store := NewMemoryRepository()
	parent := session(t, store, CreateSessionParams{Zdr: true})
	approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "session-a", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: parent.ID, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	if approval.Kind != KindSensitiveApproval || !approval.Zdr || !strings.HasPrefix(approval.ID, "z_") {
		t.Fatalf("approval did not inherit ZDR: %#v", approval)
	}
	other := session(t, store, CreateSessionParams{SessionKey: "other"})

	if err := store.RevokeScoped(ctx, parent.ID, "org-a", "user-a", "  session_closed  "); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, approval.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("approval after parent revoke: %v, want revoked", err)
	}
	if _, err := store.GetScoped(ctx, other.ID, "org-a", "user-a"); err != nil {
		t.Fatalf("unrelated grant was revoked: %v", err)
	}
	store.mu.Lock()
	reason := store.grants[approval.ID].RevokedReason
	store.mu.Unlock()
	if reason != "session_closed" {
		t.Fatalf("reason = %q", reason)
	}
}

func TestRevokeScoped_TruncatesALongReasonOnARuneBoundary(t *testing.T) {
	got := truncateReason(strings.Repeat("é", 100))
	if len(got) > maxRevokeReasonBytes || strings.ContainsRune(got, '�') {
		t.Fatalf("reason = %d bytes %q", len(got), got)
	}
}

func TestSensitiveApproval_RejectsAnApprovalParentAndMismatchedSession(t *testing.T) {
	store := NewMemoryRepository()
	parent := session(t, store, CreateSessionParams{})
	approval := ApprovalParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "session-a", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"f"}, ParentGrantID: parent.ID, TTL: time.Minute,
	}
	child, err := store.CreateScopedSensitiveApproval(ctx, approval)
	if err != nil {
		t.Fatal(err)
	}
	nested := approval
	nested.ParentGrantID = child.ID
	if _, err := store.CreateScopedSensitiveApproval(ctx, nested); err == nil {
		t.Fatal("an approval must not parent another approval")
	}
	mismatched := approval
	mismatched.SessionKey = "other-session"
	if _, err := store.CreateScopedSensitiveApproval(ctx, mismatched); err == nil {
		t.Fatal("approval for a different session must be refused")
	}
	noAction := approval
	noAction.AllowedActions, noAction.AllowedFrameIDs = nil, nil
	if _, err := store.CreateScopedSensitiveApproval(ctx, noAction); err == nil {
		t.Fatal("an approval without an action must be refused")
	}
}

func TestStaticPolicy(t *testing.T) {
	policy := StaticPolicy{MaxDomains: 2}
	for _, ok := range [][]string{
		{"example.com"},
		{"example.com", "api.example.com"},
		{"notinternal.com"},
		{"local.example.com"},
	} {
		if err := policy.CheckSession(ctx, "org", ok); err != nil {
			t.Fatalf("%v: unexpected %v", ok, err)
		}
	}
	for _, denied := range [][]string{
		{"a.example.com", "b.example.com", "c.example.com"},
		{"metadata.google.internal"},
		{"db.corp.internal"},
		{"printer.local"},
		{"app.localhost"},
		{"api.default.svc"},
		{"foo.localdomain"},
		{"metadata.goog"},
		{"x.metadata.goog"},
	} {
		if err := policy.CheckSession(ctx, "org", denied); !errors.Is(err, ErrDomainNotGrantable) {
			t.Fatalf("%v: error = %v, want ErrDomainNotGrantable", denied, err)
		}
	}
	if err := (StaticPolicy{}).CheckSession(ctx, "org", make([]string, DefaultMaxSessionDomains+1)); !errors.Is(err, ErrDomainNotGrantable) {
		t.Fatalf("default cap not applied: %v", err)
	}
	if ErrDomainNotGrantable.Error() == "" || strings.Contains(ErrDomainNotGrantable.Error(), ".") {
		t.Fatalf("error text must not look like a host: %q", ErrDomainNotGrantable)
	}
}

func sameStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for index := range got {
		if got[index] != want[index] {
			return false
		}
	}
	return true
}
