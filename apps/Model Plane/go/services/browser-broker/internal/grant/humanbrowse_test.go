package grant

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// The WS8.C contract: human_browse grants, renewal and attach derivation. Like
// deriveContract and budgetContract it is store-agnostic: it runs here against
// MemoryRepository and in postgres_integration_test.go against
// Routed{PgStore, MemoryRepository} (TestPg_HumanBrowseContract).
var humanBrowseContract = []struct {
	name string
	run  func(*testing.T, storeFactory)
}{
	{"HumanBrowse_IsOpenWebWithADenyListAndAnAbsoluteCap", contractHumanBrowse_IsOpenWebWithADenyListAndAnAbsoluteCap},
	{"HumanBrowse_CountsAgainstThePerOwnerCapWithSessions", contractHumanBrowse_CountsAgainstThePerOwnerCapWithSessions},
	{"HumanBrowse_ZdrGrantIsEphemeral", contractHumanBrowse_ZdrGrantIsEphemeral},
	{"Renew_ExtendsToTheAbsoluteCapOnly", contractRenew_ExtendsToTheAbsoluteCapOnly},
	{"Renew_RefusesExpiredRevokedOrNonOwner", contractRenew_RefusesExpiredRevokedOrNonOwner},
	{"Renew_RefusesRunAttachAndApprovalKinds", contractRenew_RefusesRunAttachAndApprovalKinds},
	{"Derive_FromHumanBrowseRequiresExplicitDomains", contractDerive_FromHumanBrowseRequiresExplicitDomains},
	{"Derive_FromHumanBrowseNarrowsByTheOrgWebPolicy", contractDerive_FromHumanBrowseNarrowsByTheOrgWebPolicy},
	{"DeriveAttach_BindsResourceAndIsNotRenewable", contractDeriveAttach_BindsResourceAndIsNotRenewable},
	{"DeriveAttach_OtherOwnersParentIsNotFound", contractDeriveAttach_OtherOwnersParentIsNotFound},
	{"DeriveAttach_RejectsInheritFromOpenWeb", contractDeriveAttach_RejectsInheritFromOpenWeb},
	{"DeriveAttach_RejectsZdrParent", contractDeriveAttach_RejectsZdrParent},
	{"DeriveAttach_ExpiryNeverExceedsParent", contractDeriveAttach_ExpiryNeverExceedsParent},
	{"Revoke_HumanBrowseParentCascadesToAttach", contractRevoke_HumanBrowseParentCascadesToAttach},
	{"Approval_UnderAnAttachParent", contractApproval_UnderAnAttachParent},
}

func TestHumanBrowseContract_Memory(t *testing.T) {
	for _, c := range humanBrowseContract {
		t.Run(c.name, func(t *testing.T) { c.run(t, memoryStore) })
	}
}

func TestNormalizeDeniedDomains(t *testing.T) {
	got, err := NormalizeDeniedDomains([]string{" .Internal ", ".local", "metadata.goog", ".local"})
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{".internal", ".local", "metadata.goog"}; !equalStrings(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	for _, bad := range [][]string{
		nil, {}, {""}, {"."}, {"a..b"}, {"*.internal"}, {"http://x.internal"}, {"x.internal:80"}, {"x.internal/"}, {"-x.local"}, {"..local"},
	} {
		if _, err := NormalizeDeniedDomains(bad); !errors.Is(err, ErrInvalidDomainPolicy) {
			t.Fatalf("%q: error = %v, want ErrInvalidDomainPolicy", bad, err)
		}
	}
}

func TestHostDenied(t *testing.T) {
	denied := []string{".internal", "metadata.goog", ".lan"}
	for host, want := range map[string]bool{
		"svc.corp.internal": true,
		"internal":          false, // a pure suffix needs a label before it
		"metadata.goog":     true,
		"a.metadata.goog":   true,
		"notmetadata.goog":  false,
		"printer.lan":       true,
		"example.com":       false,
		"lan.example.com":   false,
	} {
		if got := hostDenied(host, denied); got != want {
			t.Errorf("hostDenied(%q) = %v, want %v", host, got, want)
		}
	}
}

func TestDefaultDeniedSuffixesAreAValidDenyList(t *testing.T) {
	got, err := NormalizeDeniedDomains(DefaultDeniedSuffixes)
	if err != nil || len(got) != len(DefaultDeniedSuffixes) {
		t.Fatalf("defaults do not normalize: %v, %v", got, err)
	}
	for _, host := range []string{"x.internal", "x.local", "x.localhost", "x.localdomain", "x.lan", "x.home.arpa", "metadata.google.internal", "metadata.goog"} {
		if !hostDenied(host, got) {
			t.Errorf("%s is not denied by the default list", host)
		}
	}
}

func TestStaticPolicy_ResolveHumanBrowse(t *testing.T) {
	off, err := (StaticPolicy{}).ResolveHumanBrowse(ctx, "org", "user")
	if err != nil || off.Allowed || len(off.DeniedSuffixes) != 0 {
		t.Fatalf("zero policy = %#v, %v; human browse must be off", off, err)
	}
	on, err := (StaticPolicy{HumanBrowseEnabled: true}).ResolveHumanBrowse(ctx, "org", "user")
	if err != nil || !on.Allowed || !equalStrings(on.DeniedSuffixes, DefaultDeniedSuffixes) {
		t.Fatalf("enabled policy = %#v, %v; want the default deny-list", on, err)
	}
	custom, err := (StaticPolicy{HumanBrowseEnabled: true, DeniedSuffixes: []string{".corp"}}).ResolveHumanBrowse(ctx, "org", "user")
	if err != nil || !equalStrings(custom.DeniedSuffixes, []string{".corp"}) {
		t.Fatalf("custom policy = %#v, %v", custom, err)
	}
}

func human(t *testing.T, store Repository, p CreateHumanBrowseParams) *Grant {
	t.Helper()
	if p.OrgID == "" {
		p.OrgID = "org-a"
	}
	if p.OwnerID == "" {
		p.OwnerID = "user-a"
	}
	if p.SessionKey == "" {
		p.SessionKey = "human-a"
	}
	if p.ScopeURL == "" {
		p.ScopeURL = "browser://cloud"
	}
	if p.DeniedDomains == nil {
		p.DeniedDomains = DefaultDeniedSuffixes
	}
	if p.TTL == 0 {
		p.TTL = 15 * time.Minute
	}
	if p.AbsoluteTTL == 0 {
		p.AbsoluteTTL = 4 * time.Hour
	}
	g, err := store.CreateHumanBrowse(ctx, p)
	if err != nil {
		t.Fatal(err)
	}
	return g
}

func attachParams(parent *Grant, key, resource string, domains ...string) DeriveParams {
	p := deriveParams(parent, key, domains...)
	p.Purpose = PurposeAttach
	p.ResourceID = resource
	return p
}

func contractHumanBrowse_IsOpenWebWithADenyListAndAnAbsoluteCap(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	g := human(t, store, CreateHumanBrowseParams{DeniedDomains: []string{".Local", ".internal", "metadata.goog"}})
	now := clock.Now()
	if g.Kind != KindHumanBrowse || g.PolicyMode != PolicyModeOpenWeb || len(g.AllowedDomains) != 0 || g.Zdr || g.ParentGrantID != "" {
		t.Fatalf("human_browse grant = %#v", g)
	}
	if want := []string{".internal", ".local", "metadata.goog"}; !equalStrings(g.DeniedDomains, want) {
		t.Fatalf("denied = %v, want %v canonical", g.DeniedDomains, want)
	}
	if !g.ExpiresAt.Equal(now.Add(15*time.Minute)) || !g.AbsoluteExpiresAt.Equal(now.Add(4*time.Hour)) {
		t.Fatalf("expires %v absolute %v (now %v)", g.ExpiresAt, g.AbsoluteExpiresAt, now)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Kind != KindHumanBrowse || got.PolicyMode != PolicyModeOpenWeb || !equalStrings(got.DeniedDomains, g.DeniedDomains) ||
		len(got.AllowedDomains) != 0 || !got.AbsoluteExpiresAt.Equal(g.AbsoluteExpiresAt) || !got.ExpiresAt.Equal(g.ExpiresAt) {
		t.Fatalf("read back %#v, created %#v", got, g)
	}
	// An allowlist session reads back as an allowlist with no deny-list.
	sess := session(t, store, CreateSessionParams{SessionKey: "plain"})
	if sess.PolicyMode != PolicyModeAllowlist || len(sess.DeniedDomains) != 0 || !sess.AbsoluteExpiresAt.Equal(sess.ExpiresAt) {
		t.Fatalf("session = %#v", sess)
	}
	// An open-web grant with no deny-list is refused, and a malformed one too.
	for _, bad := range [][]string{{}, {"http://x"}, {"*.internal"}} {
		if _, err := store.CreateHumanBrowse(ctx, CreateHumanBrowseParams{
			OrgID: "org-a", OwnerID: "user-a", SessionKey: "bad", ScopeURL: "browser://cloud",
			DeniedDomains: bad, TTL: time.Minute, AbsoluteTTL: time.Hour,
		}); !errors.Is(err, ErrInvalidDomainPolicy) {
			t.Fatalf("deny-list %q: error = %v, want ErrInvalidDomainPolicy", bad, err)
		}
	}
	// An absolute TTL shorter than the TTL is raised to it: the cap is never
	// before the first expiry.
	short := human(t, store, CreateHumanBrowseParams{SessionKey: "short", TTL: 15 * time.Minute, AbsoluteTTL: time.Minute})
	if !short.AbsoluteExpiresAt.Equal(short.ExpiresAt) {
		t.Fatalf("absolute %v expires %v", short.AbsoluteExpiresAt, short.ExpiresAt)
	}
}

func contractHumanBrowse_CountsAgainstThePerOwnerCapWithSessions(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	session(t, store, CreateSessionParams{SessionKey: "s1", MaxActivePerOwner: 2})
	first := human(t, store, CreateHumanBrowseParams{SessionKey: "h1", MaxActivePerOwner: 2})
	if _, err := store.CreateHumanBrowse(ctx, CreateHumanBrowseParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "h2", ScopeURL: "browser://cloud",
		DeniedDomains: DefaultDeniedSuffixes, TTL: time.Minute, AbsoluteTTL: time.Hour, MaxActivePerOwner: 2,
	}); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("third active grant: error = %v, want ErrTooManyGrants", err)
	}
	if _, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "s2", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, MaxActivePerOwner: 2,
	}); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("a session beside two active grants: error = %v, want ErrTooManyGrants", err)
	}
	// An attach grant derived from it never counts toward the owner cap.
	if _, err := store.Derive(ctx, attachParams(first, "attach-key", "run-1", "example.com")); err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, first.ID, first.OrgID, first.OwnerID, "closed"); err != nil {
		t.Fatal(err)
	}
	human(t, store, CreateHumanBrowseParams{SessionKey: "h3", MaxActivePerOwner: 2})
}

func contractHumanBrowse_ZdrGrantIsEphemeral(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	g := human(t, store, CreateHumanBrowseParams{Zdr: true})
	if !g.Zdr || !IsZdrID(g.ID) {
		t.Fatalf("ZDR human_browse grant = %#v, want a z_ id", g)
	}
	plain := human(t, store, CreateHumanBrowseParams{SessionKey: "plain"})
	if plain.Zdr || IsZdrID(plain.ID) {
		t.Fatalf("plain human_browse grant = %#v", plain)
	}
	// A ZDR grant renews in the ephemeral store.
	if _, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute}); err != nil && !errors.Is(err, ErrAbsoluteCapReached) {
		t.Fatalf("renew ZDR: %v", err)
	}
}

func contractRenew_ExtendsToTheAbsoluteCapOnly(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	start := clock.Now()
	g := human(t, store, CreateHumanBrowseParams{})
	abs := start.Add(4 * time.Hour)

	// An early renewal never shortens the grant.
	r, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute})
	if err != nil || !r.ExpiresAt.Equal(g.ExpiresAt) {
		t.Fatalf("immediate renew: %#v, %v; want the expiry unchanged", r, err)
	}
	renewals := 0
	for ; renewals < 40; renewals++ {
		clock.Advance(10 * time.Minute)
		r, err = store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute})
		if err != nil {
			t.Fatalf("renewal %d: %v", renewals, err)
		}
		if r.ExpiresAt.After(abs) || !r.AbsoluteExpiresAt.Equal(abs) {
			t.Fatalf("renewal %d: expires %v absolute %v, want never past %v", renewals, r.ExpiresAt, r.AbsoluteExpiresAt, abs)
		}
		if r.ExpiresAt.Equal(abs) {
			break
		}
		if !r.ExpiresAt.Equal(clock.Now().Add(15 * time.Minute)) {
			t.Fatalf("renewal %d: expires %v, want now+15m", renewals, r.ExpiresAt)
		}
	}
	if !r.ExpiresAt.Equal(abs) {
		t.Fatalf("never reached the absolute cap after %d renewals", renewals)
	}
	// At the cap, a renewal is refused and changes nothing.
	clock.Advance(time.Minute)
	if _, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrAbsoluteCapReached) {
		t.Fatalf("renew at the cap: error = %v, want ErrAbsoluteCapReached", err)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil || !got.ExpiresAt.Equal(abs) {
		t.Fatalf("after the refused renewal: %#v, %v", got, err)
	}
	// Past the cap the grant is simply expired.
	clock.Advance(4 * time.Hour)
	if _, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("renew after the cap: error = %v, want ErrGrantExpired", err)
	}
}

func contractRenew_RefusesExpiredRevokedOrNonOwner(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	g := human(t, store, CreateHumanBrowseParams{})
	renew := func(id, org, owner string) error {
		_, err := store.Renew(ctx, RenewParams{OrgID: org, OwnerID: owner, GrantID: id, TTL: 15 * time.Minute})
		return err
	}
	if err := renew(g.ID, "org-a", "user-b"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("another user: %v, want ErrGrantNotFound", err)
	}
	if err := renew(g.ID, "org-b", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("another org: %v, want ErrGrantNotFound", err)
	}
	if err := renew("does-not-exist", "org-a", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("unknown grant: %v, want ErrGrantNotFound", err)
	}
	if err := renew(g.ID, "org-a", ""); !errors.Is(err, ErrRenewNotAllowed) {
		t.Fatalf("no owner (a service): %v, want ErrRenewNotAllowed", err)
	}
	revoked := human(t, store, CreateHumanBrowseParams{SessionKey: "revoked"})
	if err := store.RevokeScoped(ctx, revoked.ID, "org-a", "user-a", "closed"); err != nil {
		t.Fatal(err)
	}
	if err := renew(revoked.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("revoked: %v, want ErrGrantRevoked", err)
	}
	clock.Advance(16 * time.Minute)
	if err := renew(g.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("expired: %v, want ErrGrantExpired", err)
	}
}

func contractRenew_RefusesRunAttachAndApprovalKinds(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	run, err := store.Derive(ctx, deriveParams(h, "run-key", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	attach, err := store.Derive(ctx, attachParams(h, "attach-key", "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: attach.OrgID, OwnerID: attach.OwnerID, SessionKey: attach.SessionKey,
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: attach.ID, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	for name, g := range map[string]*Grant{"run": run, "attach": attach, "sensitive_approval": approval} {
		if _, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrRenewNotAllowed) {
			t.Fatalf("renew %s: error = %v, want ErrRenewNotAllowed", name, err)
		}
	}
	// A session grant is renewable in kind, but its absolute expiry is its
	// expiry, so there is nothing to give.
	sess := session(t, store, CreateSessionParams{SessionKey: "plain", TTL: 15 * time.Minute})
	if _, err := store.Renew(ctx, RenewParams{OrgID: sess.OrgID, OwnerID: sess.OwnerID, GrantID: sess.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrAbsoluteCapReached) {
		t.Fatalf("renew a session grant: error = %v, want ErrAbsoluteCapReached", err)
	}
}

func contractDerive_FromHumanBrowseRequiresExplicitDomains(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	if _, err := store.Derive(ctx, deriveParams(h, "empty")); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("no hosts: error = %v, want ErrDeriveNeedsExplicitDomains (open web is never inherited)", err)
	}
	if _, err := store.Derive(ctx, deriveParams(h, "nine", hosts(DefaultMaxRunDomains+1)...)); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("nine hosts: error = %v, want ErrInvalidDomainPolicy", err)
	}
	for _, denied := range []string{"api.corp.internal", "metadata.goog", "printer.lan", "svc.default.svc"} {
		if _, err := store.Derive(ctx, deriveParams(h, "denied-"+denied, denied)); !errors.Is(err, ErrDomainNotGrantable) {
			t.Fatalf("%s: error = %v, want ErrDomainNotGrantable", denied, err)
		}
	}
	run, err := store.Derive(ctx, deriveParams(h, "ok", "docs.example.com", "example.org"))
	if err != nil {
		t.Fatal(err)
	}
	if run.Kind != KindRun || run.PolicyMode != PolicyModeAllowlist || len(run.DeniedDomains) != 0 || run.ParentGrantID != h.ID ||
		!equalStrings(run.AllowedDomains, []string{"docs.example.com", "example.org"}) {
		t.Fatalf("run grant = %#v", run)
	}
	if run.ExpiresAt.After(h.ExpiresAt) {
		t.Fatalf("run outlives its parent: %v > %v", run.ExpiresAt, h.ExpiresAt)
	}
}

// contractDerive_FromHumanBrowseNarrowsByTheOrgWebPolicy: the explicit hosts of a
// child of an open-web grant are narrowed by the org web policy the grant
// carries, as an ordinary grant's hosts are (WS7.K). An allow rule narrows a host
// to the allowed hosts it covers and refuses one outside it; a deny rule refuses
// the host it covers. A run child and an attach child are narrowed alike.
func contractDerive_FromHumanBrowseNarrowsByTheOrgWebPolicy(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{
		WebPolicy: &WebPolicySnapshot{V: 1, Rev: "1", DomainsAllow: []string{"example.com"}, DomainsDeny: []string{"evil.example.com"}},
	})
	if _, err := store.Derive(ctx, deriveParams(h, "off-list", "example.org")); !errors.Is(err, ErrDomainDenied) {
		t.Fatalf("run host outside the allow rule: error = %v, want ErrDomainDenied", err)
	}
	if _, err := store.Derive(ctx, attachParams(h, "attach-off-list", "run-1", "example.org")); !errors.Is(err, ErrDomainDenied) {
		t.Fatalf("attach host outside the allow rule: error = %v, want ErrDomainDenied", err)
	}
	if _, err := store.Derive(ctx, deriveParams(h, "denied", "evil.example.com")); !errors.Is(err, ErrDomainDenied) {
		t.Fatalf("run host under the deny rule: error = %v, want ErrDomainDenied", err)
	}
	run, err := store.Derive(ctx, deriveParams(h, "ok", "docs.example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if !equalStrings(run.AllowedDomains, []string{"docs.example.com"}) {
		t.Fatalf("run hosts = %v, want docs.example.com", run.AllowedDomains)
	}
	// A requested host above an allowed host is narrowed to that allowed host,
	// never kept as requested.
	wide := human(t, store, CreateHumanBrowseParams{
		SessionKey: "human-b",
		WebPolicy:  &WebPolicySnapshot{V: 1, Rev: "1", DomainsAllow: []string{"docs.example.com"}},
	})
	run, err = store.Derive(ctx, deriveParams(wide, "narrow", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if !equalStrings(run.AllowedDomains, []string{"docs.example.com"}) {
		t.Fatalf("run hosts = %v, want the allowed docs.example.com", run.AllowedDomains)
	}
}

func contractDeriveAttach_BindsResourceAndIsNotRenewable(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	a, err := store.Derive(ctx, attachParams(h, "attach-1", "run-42", "example.com", "www.example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if a.Kind != KindAttach || a.ResourceID != "run-42" || a.ParentGrantID != h.ID || a.OwnerID != h.OwnerID ||
		a.PolicyMode != PolicyModeAllowlist || !equalStrings(a.AllowedDomains, []string{"example.com", "www.example.com"}) {
		t.Fatalf("attach grant = %#v", a)
	}
	if !a.ExpiresAt.Equal(clock.Now().Add(10*time.Minute)) || !a.AbsoluteExpiresAt.Equal(a.ExpiresAt) {
		t.Fatalf("attach expires %v absolute %v", a.ExpiresAt, a.AbsoluteExpiresAt)
	}
	got, err := store.GetScoped(ctx, a.ID, a.OrgID, a.OwnerID)
	if err != nil || got.ResourceID != "run-42" || got.Kind != KindAttach {
		t.Fatalf("read back %#v, %v", got, err)
	}
	if _, err := store.Renew(ctx, RenewParams{OrgID: a.OrgID, OwnerID: a.OwnerID, GrantID: a.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrRenewNotAllowed) {
		t.Fatalf("renew attach: error = %v, want ErrRenewNotAllowed", err)
	}
	// Idempotent on (parent, session_key); a replay never changes the grant.
	again, err := store.Derive(ctx, attachParams(h, "attach-1", "run-42", "example.com", "www.example.com"))
	if err != nil || again.ID != a.ID {
		t.Fatalf("replay: %#v, %v", again, err)
	}
	if _, err := store.Derive(ctx, attachParams(h, "attach-1", "run-43", "example.com", "www.example.com")); !errors.Is(err, ErrDeriveConflict) {
		t.Fatalf("replay with another run: error = %v, want ErrDeriveConflict", err)
	}
	if _, err := store.Derive(ctx, attachParams(h, "attach-1", "run-42", "example.com")); !errors.Is(err, ErrDeriveConflict) {
		t.Fatalf("replay with other hosts: error = %v, want ErrDeriveConflict", err)
	}
	// A run grant and an attach grant under one key are different grants.
	run, err := store.Derive(ctx, deriveParams(h, "attach-1", "example.com"))
	if err != nil || run.ID == a.ID || run.Kind != KindRun {
		t.Fatalf("run under the attach key: %#v, %v", run, err)
	}
	// The resource is required for an attach grant and refused for a run.
	for _, bad := range []string{"", strings.Repeat("r", 257), "run\n1"} {
		if _, err := store.Derive(ctx, attachParams(h, "bad-"+bad[:min(len(bad), 3)], bad, "example.com")); !errors.Is(err, ErrAttachResourceInvalid) {
			t.Fatalf("resource %q: error = %v, want ErrAttachResourceInvalid", bad, err)
		}
	}
	runWithResource := deriveParams(h, "ai-run-with-resource", "example.com")
	runWithResource.ResourceID = "run-42"
	if _, err := store.Derive(ctx, runWithResource); !errors.Is(err, ErrAttachResourceInvalid) {
		t.Fatalf("ai_run with a resource: error = %v, want ErrAttachResourceInvalid", err)
	}
	// An unknown purpose is not derivable.
	odd := deriveParams(h, "odd", "example.com")
	odd.Purpose = "bogus"
	if _, err := store.Derive(ctx, odd); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("unknown purpose: error = %v, want ErrParentNotDerivable", err)
	}
	// The attach host bound is its own.
	tight := attachParams(h, "tight", "run-1", hosts(3)...)
	tight.MaxRunDomains = 2
	if _, err := store.Derive(ctx, tight); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("three hosts under a bound of two: error = %v, want ErrInvalidDomainPolicy", err)
	}
}

func contractDeriveAttach_OtherOwnersParentIsNotFound(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	other := attachParams(h, "steal", "run-1", "example.com")
	other.OwnerID = "user-b"
	if _, err := store.Derive(ctx, other); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("another user's parent: error = %v, want ErrGrantNotFound", err)
	}
	otherOrg := attachParams(h, "steal", "run-1", "example.com")
	otherOrg.OrgID = "org-b"
	if _, err := store.Derive(ctx, otherOrg); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("another org's parent: error = %v, want ErrGrantNotFound", err)
	}
	// A service (no owner filter) cannot attach: only the person may.
	service := attachParams(h, "svc", "run-1", "example.com")
	service.OwnerID = ""
	if _, err := store.Derive(ctx, service); !errors.Is(err, ErrAttachNeedsOwner) {
		t.Fatalf("no owner: error = %v, want ErrAttachNeedsOwner", err)
	}
}

func contractDeriveAttach_RejectsInheritFromOpenWeb(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	if _, err := store.Derive(ctx, attachParams(h, "inherit", "run-1")); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("attach from open web with no hosts: error = %v, want ErrDeriveNeedsExplicitDomains", err)
	}
	// A narrow allowlist parent is no different: an attach names its hosts.
	sess := session(t, store, CreateSessionParams{SessionKey: "plain", TTL: 15 * time.Minute})
	if _, err := store.Derive(ctx, attachParams(sess, "inherit-session", "run-1")); !errors.Is(err, ErrDeriveNeedsExplicitDomains) {
		t.Fatalf("attach from a session with no hosts: error = %v, want ErrDeriveNeedsExplicitDomains", err)
	}
	// From a session grant the hosts must be a subset of the parent's.
	if _, err := store.Derive(ctx, attachParams(sess, "widen", "run-1", "evil.example")); !errors.Is(err, ErrDeriveWidensParent) {
		t.Fatalf("host outside the session grant: error = %v, want ErrDeriveWidensParent", err)
	}
	a, err := store.Derive(ctx, attachParams(sess, "narrow", "run-1", "www.example.com"))
	if err != nil || !equalStrings(a.AllowedDomains, []string{"www.example.com"}) {
		t.Fatalf("attach from a session: %#v, %v", a, err)
	}
	// A run grant cannot be a parent.
	run, err := store.Derive(ctx, deriveParams(sess, "run", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, attachParams(run, "from-run", "run-1", "example.com")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("attach from a run grant: error = %v, want ErrParentNotDerivable", err)
	}
}

func contractDeriveAttach_RejectsZdrParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	zh := human(t, store, CreateHumanBrowseParams{SessionKey: "zdr-human", Zdr: true})
	if _, err := store.Derive(ctx, attachParams(zh, "attach", "run-1", "example.com")); !errors.Is(err, ErrAttachZdrRefused) {
		t.Fatalf("attach under a ZDR human_browse grant: error = %v, want ErrAttachZdrRefused", err)
	}
	zs := session(t, store, CreateSessionParams{SessionKey: "zdr-session", Zdr: true})
	if _, err := store.Derive(ctx, attachParams(zs, "attach", "run-1", "example.com")); !errors.Is(err, ErrAttachZdrRefused) {
		t.Fatalf("attach under a ZDR session grant: error = %v, want ErrAttachZdrRefused", err)
	}
	// A ZDR run grant is still allowed under a ZDR open-web parent (ai_run path).
	if _, err := store.Derive(ctx, deriveParams(zh, "run", "example.com")); err != nil {
		t.Fatalf("ZDR ai_run under a ZDR human_browse grant: %v", err)
	}
	// A ZDR principal cannot take a non-ZDR parent's attach.
	h := human(t, store, CreateHumanBrowseParams{SessionKey: "plain"})
	p := attachParams(h, "zdr-principal", "run-1", "example.com")
	p.PrincipalZdr = true
	if _, err := store.Derive(ctx, p); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("ZDR principal on a plain parent: error = %v, want ErrZdrPostureChanged", err)
	}
}

func contractDeriveAttach_ExpiryNeverExceedsParent(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	// A parent with 5 minutes left caps the attach below its own 10 minutes.
	sess := session(t, store, CreateSessionParams{SessionKey: "short", TTL: 5 * time.Minute})
	a, err := store.Derive(ctx, attachParams(sess, "a", "run-1", "example.com"))
	if err != nil || !a.ExpiresAt.Equal(sess.ExpiresAt) {
		t.Fatalf("attach under a short parent: %#v, %v; want the parent's expiry", a, err)
	}
	// A long request is capped at the attach maximum, not the parent's.
	h := human(t, store, CreateHumanBrowseParams{})
	long := attachParams(h, "long", "run-2", "example.com")
	long.TTL = time.Hour
	l, err := store.Derive(ctx, long)
	if err != nil || !l.ExpiresAt.Equal(clock.Now().Add(AttachMaxTTL)) {
		t.Fatalf("attach with a one-hour request: %#v, %v; want now+%v", l, err, AttachMaxTTL)
	}
	// Near the end of a parent, a remainder under the minimum is refused (the
	// parent has 15 s left here; the caller renews it first).
	clock.Advance(14*time.Minute + 45*time.Second)
	if _, err := store.Derive(ctx, attachParams(h, "last", "run-3", "example.com")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("attach under a parent with 15 s left: error = %v, want ErrParentNotDerivable", err)
	}
	if _, err := store.Renew(ctx, RenewParams{OrgID: h.OrgID, OwnerID: h.OwnerID, GrantID: h.ID, TTL: 15 * time.Minute}); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Derive(ctx, attachParams(h, "last", "run-3", "example.com")); err != nil {
		t.Fatalf("attach after the parent was renewed: %v", err)
	}
}

func contractRevoke_HumanBrowseParentCascadesToAttach(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	run, err := store.Derive(ctx, deriveParams(h, "run", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	attach, err := store.Derive(ctx, attachParams(h, "attach", "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: attach.OrgID, OwnerID: attach.OwnerID, SessionKey: attach.SessionKey,
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: attach.ID, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	unrelated := human(t, store, CreateHumanBrowseParams{SessionKey: "other"})
	if err := store.RevokeScoped(ctx, h.ID, h.OrgID, h.OwnerID, "closed"); err != nil {
		t.Fatal(err)
	}
	for name, g := range map[string]*Grant{"human_browse": h, "run": run, "attach": attach, "approval": approval} {
		if _, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("%s after the parent revoke: error = %v, want ErrGrantRevoked", name, err)
		}
	}
	if _, err := store.GetScoped(ctx, unrelated.ID, unrelated.OrgID, unrelated.OwnerID); err != nil {
		t.Fatalf("an unrelated grant was touched: %v", err)
	}
	// Revoking only the attach leaves the human session live.
	h2 := human(t, store, CreateHumanBrowseParams{SessionKey: "h2"})
	a2, err := store.Derive(ctx, attachParams(h2, "attach", "run-9", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, a2.ID, a2.OrgID, a2.OwnerID, "released"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, h2.ID, h2.OrgID, h2.OwnerID); err != nil {
		t.Fatalf("revoking the attach revoked its parent: %v", err)
	}
}

func contractApproval_UnderAnAttachParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	h := human(t, store, CreateHumanBrowseParams{})
	attach, err := store.Derive(ctx, attachParams(h, "attach", "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	ok := ApprovalParams{
		OrgID: attach.OrgID, OwnerID: attach.OwnerID, SessionKey: attach.SessionKey,
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: attach.ID, TTL: time.Hour,
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, ok)
	if err != nil {
		t.Fatal(err)
	}
	if approval.ParentGrantID != attach.ID || approval.ExpiresAt.After(attach.ExpiresAt) {
		t.Fatalf("approval %#v outlives or misses its attach parent %#v", approval, attach)
	}
	widen := ok
	widen.AllowedDomains = []string{"evil.example"}
	if _, err := store.CreateScopedSensitiveApproval(ctx, widen); !errors.Is(err, ErrDeriveWidensParent) {
		t.Fatalf("approval host outside the attach hosts: error = %v, want ErrDeriveWidensParent", err)
	}
	// An approval cannot bind to an open-web parent: it has no hosts to bound it.
	onHuman := ok
	onHuman.ParentGrantID = h.ID
	onHuman.SessionKey = h.SessionKey
	if _, err := store.CreateScopedSensitiveApproval(ctx, onHuman); !errors.Is(err, ErrApprovalParentInvalid) {
		t.Fatalf("approval under human_browse: error = %v, want ErrApprovalParentInvalid", err)
	}
}
