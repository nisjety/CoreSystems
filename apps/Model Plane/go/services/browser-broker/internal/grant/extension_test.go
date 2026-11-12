package grant

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// extensionContract is the store-agnostic behaviour of an origin-extension
// grant (E3.2-e1, OD13), run against MemoryRepository here and against
// Routed{PgStore, MemoryRepository} in postgres_extension_integration_test.go.
var extensionContract = []struct {
	name string
	run  func(*testing.T, storeFactory)
}{
	{"Extension_IsParentBoundAndCappedByTheParent", contractExt_IsParentBoundAndCappedByTheParent},
	{"Extension_RevokingTheParentOrTheRootRevokesIt", contractExt_RevokingTheParentOrTheRootRevokesIt},
	{"Extension_ParentMustBeALiveOrdinarySessionOrRunOfTheSameKey", contractExt_ParentMustBeALiveOrdinarySessionOrRunOfTheSameKey},
	{"Extension_OnlyNewHostsAreGranted", contractExt_OnlyNewHostsAreGranted},
	{"Extension_HostAndCountCapAreExactAndAReplayIsNotNew", contractExt_HostAndCountCapAreExactAndAReplayIsNotNew},
	{"Extension_IsNeverRenewedDerivedFromOrAnApprovalParent", contractExt_IsNeverRenewedDerivedFromOrAnApprovalParent},
	{"Extension_ZdrIsTurnScopeAndStaysInMemory", contractExt_ZdrIsTurnScopeAndStaysInMemory},
	{"Extension_ScopeIsValidated", contractExt_ScopeIsValidated},
	{"Extension_DoesNotCountTowardTheOwnerSessionCap", contractExt_DoesNotCountTowardTheOwnerSessionCap},
	{"Extension_TheParentsWebPolicyBindsTheExtension", contractExt_TheParentsWebPolicyBindsTheExtension},
}

func TestExtensionContract_Memory(t *testing.T) {
	for _, c := range extensionContract {
		t.Run(c.name, func(t *testing.T) { c.run(t, memoryStore) })
	}
}

// extRun makes a session grant for example.com and a run grant under it whose
// session key is "run-1".
func extRun(t *testing.T, store Repository, sessionParams ...CreateSessionParams) (root, run *Grant) {
	t.Helper()
	p := CreateSessionParams{TTL: 30 * time.Minute}
	if len(sessionParams) > 0 {
		p = sessionParams[0]
		if p.TTL == 0 {
			p.TTL = 30 * time.Minute
		}
	}
	root = session(t, store, p)
	run, err := store.Derive(ctx, deriveParams(root, "run-1"))
	if err != nil {
		t.Fatalf("derive run: %v", err)
	}
	return root, run
}

func extParams(parent *Grant, scope string, hosts ...string) ExtensionParams {
	return ExtensionParams{
		OrgID: parent.OrgID, OwnerID: parent.OwnerID, SessionKey: parent.SessionKey,
		ScopeURL: parent.ScopeURL, ParentGrantID: parent.ID, AllowedDomains: hosts,
		ConsentScope: scope, TTL: 10 * time.Minute, Zdr: parent.Zdr,
	}
}

func contractExt_IsParentBoundAndCappedByTheParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	root, run := extRun(t, store)
	p := extParams(run, ConsentScopeConversation, "docs.other.org")
	p.TTL = 5 * time.Hour // a longer ask is cut to the parent's expiry
	g, err := store.CreateOriginExtension(ctx, p)
	if err != nil {
		t.Fatal(err)
	}
	if g.Kind != KindOriginExtension || g.ParentGrantID != run.ID || g.OwnerID != run.OwnerID || g.OrgID != run.OrgID ||
		g.SessionKey != run.SessionKey || g.ConsentScope != ConsentScopeConversation || g.Zdr || g.KeyID != "" ||
		g.PolicyMode != PolicyModeAllowlist || len(g.DeniedDomains) != 0 || g.Budget.IsSet() {
		t.Fatalf("extension = %#v", g)
	}
	if len(g.AllowedDomains) != 1 || g.AllowedDomains[0] != "docs.other.org" {
		t.Fatalf("hosts = %v", g.AllowedDomains)
	}
	if g.ExpiresAt.After(run.ExpiresAt) || !g.AbsoluteExpiresAt.Equal(g.ExpiresAt) {
		t.Fatalf("extension expires %v / %v, parent %v: never past the parent", g.ExpiresAt, g.AbsoluteExpiresAt, run.ExpiresAt)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil || got.ConsentScope != ConsentScopeConversation || got.ParentGrantID != run.ID ||
		len(got.AllowedDomains) != 1 || got.AllowedDomains[0] != "docs.other.org" {
		t.Fatalf("read back: %#v, %v", got, err)
	}
	// Another owner and another org never see it.
	if _, err := store.GetScoped(ctx, g.ID, g.OrgID, "someone-else"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("other owner: %v", err)
	}
	if _, err := store.GetScoped(ctx, g.ID, "org-b", g.OwnerID); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("other org: %v", err)
	}
	// An extension directly under the session grant is allowed too.
	if _, err := store.CreateOriginExtension(ctx, extParams(root, ConsentScopeTurn, "third.example.net")); err != nil {
		t.Fatalf("extension under a session grant: %v", err)
	}
}

func contractExt_RevokingTheParentOrTheRootRevokesIt(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	root, run := extRun(t, store)
	a, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, run.ID, run.OrgID, run.OwnerID, "user"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, a.ID, a.OrgID, a.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("extension after its run was revoked: %v", err)
	}
	// A revoked parent takes no new extension.
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "b.other.org")); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("extension under a revoked parent: %v", err)
	}
	// Revoking the root session reaches an extension two levels down.
	_, run2 := extRunOn(t, store, root, "run-2")
	b, err := store.CreateOriginExtension(ctx, extParams(run2, ConsentScopeTurn, "c.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, root.ID, root.OrgID, root.OwnerID, "user"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(ctx, b.ID, b.OrgID, b.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("extension after its root was revoked: %v", err)
	}
}

func extRunOn(t *testing.T, store Repository, root *Grant, key string) (*Grant, *Grant) {
	t.Helper()
	run, err := store.Derive(ctx, deriveParams(root, key))
	if err != nil {
		t.Fatalf("derive run %s: %v", key, err)
	}
	return root, run
}

func contractExt_ParentMustBeALiveOrdinarySessionOrRunOfTheSameKey(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	root, run := extRun(t, store)

	wrongKey := extParams(run, ConsentScopeTurn, "a.other.org")
	wrongKey.SessionKey = "another-run"
	if _, err := store.CreateOriginExtension(ctx, wrongKey); !errors.Is(err, ErrExtensionParentInvalid) {
		t.Fatalf("other session key: %v", err)
	}
	wrongOwner := extParams(run, ConsentScopeTurn, "a.other.org")
	wrongOwner.OwnerID = "intruder"
	if _, err := store.CreateOriginExtension(ctx, wrongOwner); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("other owner: %v", err)
	}
	missing := extParams(run, ConsentScopeTurn, "a.other.org")
	missing.ParentGrantID = "nope"
	if _, err := store.CreateOriginExtension(ctx, missing); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("missing parent: %v", err)
	}
	// A sensitive approval is single-use authority and never a parent.
	approval, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: root.OrgID, OwnerID: root.OwnerID, SessionKey: root.SessionKey, ScopeURL: root.ScopeURL,
		AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: root.ID, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.CreateOriginExtension(ctx, extParams(approval, ConsentScopeTurn, "a.other.org")); !errors.Is(err, ErrExtensionParentInvalid) {
		t.Fatalf("approval parent: %v", err)
	}
	// An extension is not a parent of another extension.
	ext, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.CreateOriginExtension(ctx, extParams(ext, ConsentScopeTurn, "b.other.org")); !errors.Is(err, ErrExtensionParentInvalid) {
		t.Fatalf("extension parent: %v", err)
	}
	// An expired parent takes none.
	clock.Advance(time.Hour)
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "c.other.org")); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("expired parent: %v", err)
	}
}

func contractExt_OnlyNewHostsAreGranted(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	_, run := extRun(t, store)
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "example.com", "www.example.com")); !errors.Is(err, ErrExtensionNothingNew) {
		t.Fatalf("hosts the parent already covers: %v", err)
	}
	g, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "example.com", "new.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if len(g.AllowedDomains) != 1 || g.AllowedDomains[0] != "new.other.org" {
		t.Fatalf("hosts = %v, want only the new one", g.AllowedDomains)
	}
	for _, bad := range [][]string{
		{"*.other.org"}, {".other.org"}, {"https://other.org"}, {"other.org:8443"}, {"10.0.0.1"}, {"localhost"}, {""}, nil,
	} {
		if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, bad...)); err == nil {
			t.Fatalf("hosts %q were accepted", bad)
		}
	}
}

func contractExt_HostAndCountCapAreExactAndAReplayIsNotNew(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	_, run := extRun(t, store)
	var first *Grant
	for i := 0; i < MaxExtensionHosts; i++ {
		host := string(rune('a'+i)) + ".other.org"
		g, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, host))
		if err != nil {
			t.Fatalf("host %d: %v", i, err)
		}
		if i == 0 {
			first = g
		}
	}
	// The same decision again returns the grant already issued, even at the cap.
	again, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil || again.ID != first.ID {
		t.Fatalf("replay = %#v, %v; want grant %s", again, err, first.ID)
	}
	// A ninth distinct host passes neither the count nor the host cap.
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "z.other.org")); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("ninth extension: %v", err)
	}
	// Nine hosts in one request are refused outright.
	nine := make([]string, 0, 9)
	for i := 0; i < 9; i++ {
		nine = append(nine, string(rune('a'+i))+".nine.org")
	}
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, nine...)); err == nil {
		t.Fatal("nine hosts in one request were accepted")
	}
}

func contractExt_IsNeverRenewedDerivedFromOrAnApprovalParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	_, run := extRun(t, store)
	ext, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Renew(ctx, RenewParams{OrgID: ext.OrgID, OwnerID: ext.OwnerID, GrantID: ext.ID, TTL: time.Hour}); !errors.Is(err, ErrRenewNotAllowed) {
		t.Fatalf("renew: %v", err)
	}
	for _, purpose := range []string{"", PurposeAIRun, PurposeAttach} {
		p := deriveParams(ext, "child-"+purpose)
		p.Purpose = purpose
		if purpose == PurposeAttach {
			p.ResourceID = "run-9"
		}
		if _, err := store.Derive(ctx, p); !errors.Is(err, ErrParentNotDerivable) {
			t.Fatalf("derive (%q) from an extension: %v", purpose, err)
		}
	}
	if _, err := store.CreateScopedSensitiveApproval(ctx, ApprovalParams{
		OrgID: ext.OrgID, OwnerID: ext.OwnerID, SessionKey: ext.SessionKey, ScopeURL: ext.ScopeURL,
		AllowedDomains: []string{"a.other.org"}, AllowedActions: []string{"download_ref"},
		AllowedFrameIDs: []string{"frame-1"}, ParentGrantID: ext.ID, TTL: time.Minute,
	}); !errors.Is(err, ErrApprovalParentInvalid) {
		t.Fatalf("approval under an extension: %v", err)
	}
}

func contractExt_ZdrIsTurnScopeAndStaysInMemory(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	zroot := session(t, store, CreateSessionParams{Zdr: true, SessionKey: "zs", TTL: 30 * time.Minute})
	zrun, err := store.Derive(ctx, deriveParams(zroot, "zr"))
	if err != nil {
		t.Fatal(err)
	}
	for _, scope := range []string{ConsentScopeConversation, ConsentScopeAlways} {
		if _, err := store.CreateOriginExtension(ctx, extParams(zrun, scope, "a.other.org")); !errors.Is(err, ErrConsentScopeZdr) {
			t.Fatalf("zdr scope %q: %v", scope, err)
		}
	}
	g, err := store.CreateOriginExtension(ctx, extParams(zrun, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if !g.Zdr || !strings.HasPrefix(g.ID, zdrIDPrefix) {
		t.Fatalf("a ZDR parent's extension = %#v, want a z_ id and zdr", g)
	}
	// A ZDR caller under a non-ZDR parent is a posture change, never a ZDR
	// child of a persisted parent.
	_, run := extRun(t, store)
	p := extParams(run, ConsentScopeTurn, "b.other.org")
	p.Zdr = true
	if _, err := store.CreateOriginExtension(ctx, p); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("zdr caller under a non-zdr parent: %v", err)
	}
}

func contractExt_ScopeIsValidated(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	_, run := extRun(t, store)
	for _, scope := range []string{"", "forever", "TURN", "turn "} {
		if _, err := store.CreateOriginExtension(ctx, extParams(run, scope, "a.other.org")); !errors.Is(err, ErrConsentScopeInvalid) {
			t.Fatalf("scope %q: %v", scope, err)
		}
	}
	for _, scope := range []string{ConsentScopeTurn, ConsentScopeConversation, ConsentScopeAlways} {
		host := scope + ".other.org"
		g, err := store.CreateOriginExtension(ctx, extParams(run, scope, host))
		if err != nil || g.ConsentScope != scope {
			t.Fatalf("scope %q: %#v, %v", scope, g, err)
		}
	}
}

func contractExt_DoesNotCountTowardTheOwnerSessionCap(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	capped := func(key string) CreateSessionParams {
		return CreateSessionParams{
			OrgID: "org-a", OwnerID: "user-a", SessionKey: key, ScopeURL: "browser://cloud",
			AllowedDomains: []string{"example.com"}, TTL: 30 * time.Minute, MaxActivePerOwner: 2,
		}
	}
	_, run := extRun(t, store, capped("cap-1"))
	for i := 0; i < 3; i++ {
		host := string(rune('a'+i)) + ".other.org"
		if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, host)); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := store.CreateSession(ctx, capped("cap-2")); err != nil {
		t.Fatalf("a second session with three extensions live: %v", err)
	}
	if _, err := store.CreateSession(ctx, capped("cap-3")); !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("a third session: %v, want the owner cap", err)
	}
}

// contractExt_TheParentsWebPolicyBindsTheExtension: the extension keeps its
// parent's web-policy snapshot (WS7.K), and the hosts it adds pass the
// snapshot's deny and allow rules, as an ordinary grant's hosts do at acquire.
// A host the org denies, or one a deny rule sits below, is refused, and so is a
// host outside the allow rule.
func contractExt_TheParentsWebPolicyBindsTheExtension(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	root, run := extRun(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com"},
		WebPolicy: &WebPolicySnapshot{V: 1, Rev: "1",
			DomainsAllow: []string{"example.com", "other.org"}, DomainsDeny: []string{"evil.other.org"}},
	})
	for _, host := range []string{"evil.other.org", "other.org", "third.net"} {
		if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, host)); !errors.Is(err, ErrDomainDenied) {
			t.Fatalf("extension to %s: %v, want ErrDomainDenied", host, err)
		}
	}
	g, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "new.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if len(g.AllowedDomains) != 1 || g.AllowedDomains[0] != "new.other.org" {
		t.Fatalf("hosts = %v", g.AllowedDomains)
	}
	want := root.WebPolicy
	carries := func(got *WebPolicySnapshot) bool {
		return got != nil && got.V == want.V && got.Rev == want.Rev &&
			equalStrings(got.DomainsAllow, want.DomainsAllow) && equalStrings(got.DomainsDeny, want.DomainsDeny)
	}
	if !carries(g.WebPolicy) {
		t.Fatalf("extension snapshot = %#v, want the parent's %#v", g.WebPolicy, want)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil || !carries(got.WebPolicy) {
		t.Fatalf("read back snapshot = %#v, %v; want the parent's", got, err)
	}
}

// --- pure rules ---

// TestExtensionWebPolicy_NeverSubstitutesAHost: an allow rule keeps or drops the
// hosts an extension asks for and never puts an allowed host in their place
// (Narrow would), since the server checked the policies on the requested hosts.
func TestExtensionWebPolicy_NeverSubstitutesAHost(t *testing.T) {
	s := &WebPolicySnapshot{V: 1, Rev: "1", DomainsAllow: []string{"docs.other.org"}}
	if got, err := extensionWebPolicy([]string{"other.org"}, s); !errors.Is(err, ErrDomainDenied) {
		t.Fatalf("other.org under an allow rule for docs.other.org = %v, %v; want ErrDomainDenied", got, err)
	}
	got, err := extensionWebPolicy([]string{"docs.other.org", "third.net"}, s)
	if err != nil || len(got) != 1 || got[0] != "docs.other.org" {
		t.Fatalf("kept = %v, %v; want only the allowed host", got, err)
	}
	if got, err := extensionWebPolicy([]string{"api.other.org"}, nil); err != nil || len(got) != 1 {
		t.Fatalf("no snapshot must change nothing: %v, %v", got, err)
	}
}

func TestNormalizeExtensionHosts_RefusesPatternsAndBoundsTheList(t *testing.T) {
	for _, raw := range []string{"*.example.org", "*", "a*.example.org", ".example.org", " .example.org", "example.org/*", "[a].example.org", "{a}.example.org", "a?.example.org"} {
		if _, err := NormalizeExtensionHosts([]string{raw}); !errors.Is(err, ErrWildcardRefused) {
			t.Fatalf("%q: %v, want ErrWildcardRefused", raw, err)
		}
	}
	for _, raw := range []string{"https://example.org", "example.org:443", "example.org/x", "127.0.0.1", "localhost", "", "ex ample.org", "EXAMPLE_.org", "bücher.example"} {
		if _, err := NormalizeExtensionHosts([]string{raw}); err == nil {
			t.Fatalf("%q was accepted", raw)
		}
	}
	got, err := NormalizeExtensionHosts([]string{" B.Example.org ", "a.example.org", "b.example.org"})
	if err != nil || len(got) != 2 || got[0] != "a.example.org" || got[1] != "b.example.org" {
		t.Fatalf("normalized = %v, %v", got, err)
	}
	if _, err := NormalizeExtensionHosts(nil); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("empty: %v", err)
	}
	nine := make([]string, 9)
	for i := range nine {
		nine[i] = string(rune('a'+i)) + ".example.org"
	}
	if _, err := NormalizeExtensionHosts(nine); !errors.Is(err, ErrInvalidDomainPolicy) {
		t.Fatalf("nine hosts: %v", err)
	}
}

// The wildcard refusal and isCanonicalDomain stay in step: the base check a
// session grant uses refuses every pattern too, so a pattern cannot enter by
// the older path while the consent path refuses it. Changing one means
// changing both, the Quarry BrowserEgressPolicy and the approved language.
func TestWildcardsAreRefusedByBothHostChecks(t *testing.T) {
	for _, raw := range []string{"*.example.org", "*", ".example.org", "a*.example.org", "example.*"} {
		if isCanonicalDomain(raw) {
			t.Fatalf("isCanonicalDomain accepted %q", raw)
		}
		if _, err := NormalizeAllowedDomains([]string{raw}); err == nil {
			t.Fatalf("NormalizeAllowedDomains accepted %q", raw)
		}
		if _, err := NormalizeExtensionHosts([]string{raw}); err == nil {
			t.Fatalf("NormalizeExtensionHosts accepted %q", raw)
		}
	}
}

func TestConsentPatternLanguage_IsADraftAndOff(t *testing.T) {
	if ConsentPatternLanguageApproved {
		t.Fatal("the consent pattern language is not approved (OD13, pending counsel)")
	}
	if !strings.Contains(ConsentPatternLanguage, "DRAFT") || !strings.Contains(ConsentPatternLanguage, "NOT AVAILABLE") {
		t.Fatal("the pattern language constant must say it is a draft and that wildcards are unavailable")
	}
}

func TestApplyConsentDenies_DenyBeatsApprovalInEveryDirection(t *testing.T) {
	denied, err := NormalizeDeniedDomains([]string{"pay.shop.example", ".cdn.example", "Blocked.Example"})
	if err != nil {
		t.Fatal(err)
	}
	approved := []string{
		"ok.example",           // unrelated: kept
		"pay.shop.example",     // equal to a denied entry: dropped
		"x.pay.shop.example",   // below a denied entry: dropped
		"shop.example",         // above a denied entry (carve-out): dropped
		"blocked.example",      // equal, case-insensitive entry: dropped
		"cdn.example",          // a suffix-only entry still overlaps its apex: dropped
		"img.cdn.example",      // below a suffix entry: dropped
		"notcdn.example",       // a lookalike, not a subdomain: kept
		"shop.example.org",     // different registrable name: kept
		"badpay.shop.example2", // lookalike: kept
	}
	kept, dropped := ApplyConsentDenies(approved, denied)
	wantKept := []string{"ok.example", "notcdn.example", "shop.example.org", "badpay.shop.example2"}
	if strings.Join(kept, ",") != strings.Join(wantKept, ",") || len(dropped) != 6 {
		t.Fatalf("kept = %v dropped = %v", kept, dropped)
	}
	if k, d := ApplyConsentDenies(approved, nil); len(k) != len(approved) || len(d) != 0 {
		t.Fatal("no denies must drop nothing")
	}
}

func TestCheckConsentScope(t *testing.T) {
	for _, scope := range []string{ConsentScopeTurn, ConsentScopeConversation, ConsentScopeAlways} {
		if err := CheckConsentScope(scope, false); err != nil {
			t.Fatalf("%q: %v", scope, err)
		}
	}
	if err := CheckConsentScope(ConsentScopeTurn, true); err != nil {
		t.Fatalf("zdr turn: %v", err)
	}
	if err := CheckConsentScope(ConsentScopeAlways, true); !errors.Is(err, ErrConsentScopeZdr) {
		t.Fatalf("zdr always: %v", err)
	}
	if err := CheckConsentScope("", false); !errors.Is(err, ErrConsentScopeInvalid) {
		t.Fatalf("empty: %v", err)
	}
}
