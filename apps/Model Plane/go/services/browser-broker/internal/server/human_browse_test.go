package server

import (
	"context"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// WS8.C: human_browse grants, RenewGrant and attach derivation over the gRPC
// handlers. The grant-layer rules are pinned in grant.humanBrowseContract; this
// file pins the wiring: who may ask, what the policy says, the status codes and
// the fields on the wire.

type stepClock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *stepClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *stepClock) Advance(d time.Duration) {
	c.mu.Lock()
	c.now = c.now.Add(d)
	c.mu.Unlock()
}

func newStepClock() *stepClock {
	return &stepClock{now: time.Date(2026, 10, 24, 9, 0, 0, 0, time.UTC)}
}

func serviceBrowserPrincipal() authctx.Principal {
	return authctx.Principal{
		OrganizationID: "org1", ActorID: "svc-execution", PrincipalType: "service",
		Scopes: []string{"browser:read", "browser:write"},
	}
}

// humanServer is a server with human_browse switched on (the broker default is
// off), reading clock.
func humanServer(store grant.Repository, principal authctx.Principal, mutate ...func(*Config)) *Server {
	cfg := DefaultConfig()
	cfg.HumanBrowseEnabled = true
	for _, m := range mutate {
		m(&cfg)
	}
	srv := NewServerWithConfig(store, cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

func humanReq(mutate ...func(*AcquireGrantRequest)) *AcquireGrantRequest {
	req := &AcquireGrantRequest{OrgId: "org1", SessionKey: "human-1", GrantKind: "human_browse"}
	for _, m := range mutate {
		m(req)
	}
	return req
}

func acquireHuman(t *testing.T, srv *Server, mutate ...func(*AcquireGrantRequest)) *AcquireGrantResponse {
	t.Helper()
	resp, err := srv.AcquireGrant(context.Background(), humanReq(mutate...))
	if err != nil {
		t.Fatalf("AcquireGrant(human_browse): %v", err)
	}
	return resp
}

func renewReq(id string) *RenewGrantRequest { return &RenewGrantRequest{GrantId: id, OrgId: "org1"} }

func TestAcquireGrant_HumanBrowse_UserOnly(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := humanServer(store, userPrincipal(false))
	resp := acquireHuman(t, srv)
	if resp.GetKind() != "human_browse" || resp.GetPolicyMode() != "open_web" || len(resp.GetAllowedDomains()) != 0 ||
		resp.GetParentGrantId() != "" || resp.GetGrantId() == "" {
		t.Fatalf("human_browse response = %#v", resp)
	}
	if len(resp.GetDeniedDomains()) == 0 {
		t.Fatal("an open-web grant carries a deny-list")
	}

	// A service principal, even with browser:write, is refused.
	svc := humanServer(store, serviceBrowserPrincipal())
	_, err := svc.AcquireGrant(context.Background(), humanReq(func(r *AcquireGrantRequest) { r.SessionKey = "svc" }))
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("service principal: code = %v (%v), want PermissionDenied", status.Code(err), err)
	}
	// Another tenant's request is refused before anything else.
	_, err = srv.AcquireGrant(context.Background(), humanReq(func(r *AcquireGrantRequest) { r.OrgId = "org2" }))
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("other org: code = %v, want PermissionDenied", status.Code(err))
	}
	// An unknown kind is not silently an ordinary grant.
	_, err = srv.AcquireGrant(context.Background(), humanReq(func(r *AcquireGrantRequest) {
		r.GrantKind = "admin"
		r.AllowedDomains = []string{"example.com"}
	}))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("unknown grant_kind: code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestAcquireGrant_HumanBrowse_RejectsClientDomains(t *testing.T) {
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	for name, mutate := range map[string]func(*AcquireGrantRequest){
		"allowed_domains":      func(r *AcquireGrantRequest) { r.AllowedDomains = []string{"example.com"} },
		"sensitive action":     func(r *AcquireGrantRequest) { r.AllowedActions = []string{"download_ref"} },
		"frame scope":          func(r *AcquireGrantRequest) { r.AllowedFrameIds = []string{"frame-1"} },
		"parent grant":         func(r *AcquireGrantRequest) { r.ParentGrantId = "abc" },
		"budget bound":         func(r *AcquireGrantRequest) { r.BudgetMaxSteps = 5; r.BudgetSource = "request" },
		"budget minted at set": func(r *AcquireGrantRequest) { r.BudgetMintedAtUnix = 1 },
	} {
		t.Run(name, func(t *testing.T) {
			_, err := srv.AcquireGrant(context.Background(), humanReq(mutate))
			if status.Code(err) != codes.InvalidArgument {
				t.Fatalf("code = %v (%v), want InvalidArgument", status.Code(err), err)
			}
			if strings.Contains(err.Error(), "example.com") {
				t.Fatalf("the error echoes a host: %v", err)
			}
		})
	}
}

func TestAcquireGrant_HumanBrowse_DisabledByPolicy(t *testing.T) {
	// The broker default is off: a server built with DefaultConfig refuses it.
	off := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	_, err := off.AcquireGrant(context.Background(), humanReq())
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("default config: code = %v (%v), want PermissionDenied", status.Code(err), err)
	}
	// An explicit policy that says no is no, whatever the config switch says.
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false), func(c *Config) {
		c.Policy = grant.StaticPolicy{HumanBrowseEnabled: false}
	})
	_, err = srv.AcquireGrant(context.Background(), humanReq())
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("policy off: code = %v (%v), want PermissionDenied", status.Code(err), err)
	}
	// The ordinary path is not affected by the switch.
	if _, err := off.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "s", AllowedDomains: []string{"example.com"},
	}); err != nil {
		t.Fatalf("ordinary grant with human browse off: %v", err)
	}
}

func TestAcquireGrant_HumanBrowse_SetsAbsoluteCap(t *testing.T) {
	clock := newStepClock()
	store := grant.NewMemoryRepositoryWithClock(clock.Now)
	srv := humanServer(store, userPrincipal(false), func(c *Config) {
		c.TTL = 15 * time.Minute
		c.HumanAbsoluteTTL = 4 * time.Hour
	})
	resp := acquireHuman(t, srv)
	if got := resp.GetExpiresAt().AsTime(); !got.Equal(clock.Now().Add(15 * time.Minute)) {
		t.Fatalf("expires_at = %v", got)
	}
	if got := resp.GetAbsoluteExpiresAt().AsTime(); !got.Equal(clock.Now().Add(4 * time.Hour)) {
		t.Fatalf("absolute_expires_at = %v, want creation + 4h", got)
	}
	// An ordinary grant reports its own expiry as the absolute one.
	plain, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "plain", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if plain.GetPolicyMode() != "allowlist" || len(plain.GetDeniedDomains()) != 0 ||
		!plain.GetAbsoluteExpiresAt().AsTime().Equal(plain.GetExpiresAt().AsTime()) {
		t.Fatalf("ordinary grant response = %#v", plain)
	}
}

func TestAcquireGrant_HumanBrowse_IsEphemeralWhenZdr(t *testing.T) {
	store := grant.NewMemoryRepository()
	for name, tc := range map[string]struct {
		token   bool
		request bool
	}{"request raises": {false, true}, "token raises": {true, false}} {
		t.Run(name, func(t *testing.T) {
			srv := humanServer(store, userPrincipal(tc.token))
			resp := acquireHuman(t, srv, func(r *AcquireGrantRequest) {
				r.Zdr = tc.request
				r.SessionKey = "zdr-" + name
			})
			if !resp.GetZdr() || !strings.HasPrefix(resp.GetGrantId(), "z_") {
				t.Fatalf("ZDR human_browse grant = %#v, want zdr and a z_ id", resp)
			}
		})
	}
	// And a ZDR grant's log line names no id.
	logs := captureLog(t)
	srv := humanServer(store, userPrincipal(true))
	zdr := acquireHuman(t, srv, func(r *AcquireGrantRequest) { r.SessionKey = "logged" })
	if strings.Contains(logs.String(), zdr.GetGrantId()) {
		t.Fatalf("the log carries a ZDR grant id: %s", logs.String())
	}
}

func TestValidateGrant_ReturnsPolicyModeAndDenied(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := humanServer(store, userPrincipal(false))
	issued := acquireHuman(t, srv)
	got, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: issued.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	if got.GetKind() != "human_browse" || got.GetPolicyMode() != "open_web" || got.GetOwnerId() != "user1" || !got.GetActive() ||
		len(got.GetAllowedDomains()) != 0 || !equalStrings(got.GetDeniedDomains(), issued.GetDeniedDomains()) ||
		!got.GetAbsoluteExpiresAt().AsTime().Equal(issued.GetAbsoluteExpiresAt().AsTime()) || got.GetResourceId() != "" {
		t.Fatalf("validate = %#v, issued %#v", got, issued)
	}
	// The default deny-list is on the wire, and every entry is a name, never an IP.
	for _, want := range []string{".internal", ".local", "metadata.goog"} {
		found := false
		for _, d := range got.GetDeniedDomains() {
			found = found || d == want
		}
		if !found {
			t.Errorf("deny-list %v lacks %s", got.GetDeniedDomains(), want)
		}
	}
	// An allowlist grant validates as an allowlist.
	plain := acquire(t, srv, &AcquireGrantRequest{SessionKey: "plain"})
	pv, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: plain.GetGrantId()})
	if err != nil || pv.GetPolicyMode() != "allowlist" || len(pv.GetDeniedDomains()) != 0 || len(pv.GetAllowedDomains()) == 0 {
		t.Fatalf("allowlist validate = %#v, %v", pv, err)
	}
	// A configured deny-list replaces the default.
	custom := humanServer(grant.NewMemoryRepository(), userPrincipal(false), func(c *Config) { c.DenySuffixes = []string{".corp", "intranet.example"} })
	c := acquireHuman(t, custom)
	if !equalStrings(c.GetDeniedDomains(), []string{".corp", "intranet.example"}) {
		t.Fatalf("custom deny-list = %v", c.GetDeniedDomains())
	}
}

func TestRenewGrant_ExtendsToAbsoluteCapOnly(t *testing.T) {
	clock := newStepClock()
	store := grant.NewMemoryRepositoryWithClock(clock.Now)
	srv := humanServer(store, userPrincipal(false), func(c *Config) {
		c.TTL = 15 * time.Minute
		c.HumanAbsoluteTTL = time.Hour
	})
	issued := acquireHuman(t, srv)
	abs := issued.GetAbsoluteExpiresAt().AsTime()

	clock.Advance(10 * time.Minute)
	r, err := srv.RenewGrant(context.Background(), renewReq(issued.GetGrantId()))
	if err != nil {
		t.Fatal(err)
	}
	if !r.GetExpiresAt().AsTime().Equal(clock.Now().Add(15*time.Minute)) || !r.GetAbsoluteExpiresAt().AsTime().Equal(abs) {
		t.Fatalf("first renewal = %#v", r)
	}
	// Renew every 14 minutes (inside each 15-minute lease) until a renewal
	// would run past the one-hour cap: it is clamped to the cap, never beyond.
	for i := 0; i < 10 && !r.GetExpiresAt().AsTime().Equal(abs); i++ {
		clock.Advance(14 * time.Minute)
		r, err = srv.RenewGrant(context.Background(), renewReq(issued.GetGrantId()))
		if err != nil {
			t.Fatalf("renewal at %v: %v", clock.Now(), err)
		}
		if r.GetExpiresAt().AsTime().After(abs) {
			t.Fatalf("renewal expires %v, past the cap %v", r.GetExpiresAt().AsTime(), abs)
		}
	}
	if !r.GetExpiresAt().AsTime().Equal(abs) {
		t.Fatalf("never reached the cap: expires %v, want %v", r.GetExpiresAt().AsTime(), abs)
	}
	clock.Advance(time.Minute)
	_, err = srv.RenewGrant(context.Background(), renewReq(issued.GetGrantId()))
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("renewal at the cap: code = %v (%v), want FailedPrecondition", status.Code(err), err)
	}
	// ValidateGrant reports the capped expiry.
	v, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: issued.GetGrantId()})
	if err != nil || !v.GetExpiresAt().AsTime().Equal(abs) {
		t.Fatalf("validate after renewals = %#v, %v", v, err)
	}
}

func TestRenewGrant_RefusesExpiredRevokedOrNonOwner(t *testing.T) {
	clock := newStepClock()
	store := grant.NewMemoryRepositoryWithClock(clock.Now)
	srv := humanServer(store, userPrincipal(false))
	live := acquireHuman(t, srv)
	revoked := acquireHuman(t, srv, func(r *AcquireGrantRequest) { r.SessionKey = "revoked" })
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: revoked.GetGrantId()}); err != nil {
		t.Fatal(err)
	}
	if _, err := srv.RenewGrant(context.Background(), renewReq(revoked.GetGrantId())); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("revoked: code = %v, want FailedPrecondition", status.Code(err))
	}
	// Another user of the same tenant, and another tenant, see nothing.
	for _, principal := range []authctx.Principal{
		{OrganizationID: "org1", ActorID: "user2", PrincipalType: "user"},
		{OrganizationID: "org2", ActorID: "user1", PrincipalType: "user"},
	} {
		other := humanServer(store, principal)
		req := renewReq(live.GetGrantId())
		req.OrgId = principal.OrganizationID
		if _, err := other.RenewGrant(context.Background(), req); status.Code(err) != codes.NotFound {
			t.Fatalf("%s/%s: code = %v (%v), want NotFound", principal.OrganizationID, principal.ActorID, status.Code(err), err)
		}
	}
	if _, err := srv.RenewGrant(context.Background(), renewReq("no-such-grant")); status.Code(err) != codes.NotFound {
		t.Fatalf("unknown grant: code = %v, want NotFound", status.Code(err))
	}
	// A renewal for another organization than the token's is refused up front.
	crossOrg := renewReq(live.GetGrantId())
	crossOrg.OrgId = "org2"
	if _, err := srv.RenewGrant(context.Background(), crossOrg); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("cross-org renewal: code = %v, want PermissionDenied", status.Code(err))
	}
	for name, req := range map[string]*RenewGrantRequest{
		"no grant id": {OrgId: "org1"},
		"no org":      {GrantId: live.GetGrantId()},
	} {
		if _, err := srv.RenewGrant(context.Background(), req); status.Code(err) != codes.InvalidArgument {
			t.Fatalf("%s: code = %v, want InvalidArgument", name, status.Code(err))
		}
	}
	clock.Advance(16 * time.Minute)
	if _, err := srv.RenewGrant(context.Background(), renewReq(live.GetGrantId())); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("expired: code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestRenewGrant_RefusesRunAttachAndApprovalKinds(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := humanServer(store, userPrincipal(false))
	parent := acquireHuman(t, srv)
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"example.com"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	attach, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	approval, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "attach-key", AllowedDomains: []string{"example.com"},
		AllowedActions: []string{"download_ref"}, AllowedFrameIds: []string{"frame-1"}, ParentGrantId: attach.GetGrantId(),
	})
	if err != nil {
		t.Fatal(err)
	}
	for name, id := range map[string]string{"run": run.GetGrantId(), "attach": attach.GetGrantId(), "approval": approval.GetGrantId()} {
		if _, err := srv.RenewGrant(context.Background(), renewReq(id)); status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("renew %s: code = %v (%v), want FailedPrecondition", name, status.Code(err), err)
		}
	}
}

func TestRenewGrant_RequiresUserPrincipal(t *testing.T) {
	store := grant.NewMemoryRepository()
	owner := humanServer(store, userPrincipal(false))
	issued := acquireHuman(t, owner)
	// A workload with browser:write cannot keep a person's session alive.
	svc := humanServer(store, serviceBrowserPrincipal())
	if _, err := svc.RenewGrant(context.Background(), renewReq(issued.GetGrantId())); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("service principal: code = %v, want PermissionDenied", status.Code(err))
	}
	// A user bearer passes the authorization layer; a service needs browser:write.
	if _, err := owner.RenewGrant(context.Background(), renewReq(issued.GetGrantId())); err != nil {
		t.Fatalf("owner renewal: %v", err)
	}
}

func attachReq(parent, resource string, hosts ...string) *DeriveRunGrantRequest {
	return deriveReq(parent, func(r *DeriveRunGrantRequest) {
		r.Purpose = "attach"
		r.SessionKey = "attach-key"
		r.ResourceId = resource
		r.AllowedDomains = hosts
	})
}

func TestDeriveRunGrant_FromHumanBrowseRequiresExplicitDomains(t *testing.T) {
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquireHuman(t, srv)
	_, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId()))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("no hosts: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"api.corp.internal"}
	}))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("a denied host: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
	nine := make([]string, grant.DefaultMaxRunDomains+1)
	for i := range nine {
		nine[i] = string(rune('a'+i)) + ".example.com"
	}
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) { r.AllowedDomains = nine }))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("nine hosts: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"docs.example.com"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	if run.GetKind() != "run" || !equalStrings(run.GetAllowedDomains(), []string{"docs.example.com"}) || run.GetResourceId() != "" {
		t.Fatalf("run response = %#v", run)
	}
	// An ai_run derive never takes a resource.
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "with-resource"
		r.AllowedDomains = []string{"docs.example.com"}
		r.ResourceId = "run-1"
	}))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("ai_run with resource_id: code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestDeriveAttach_BindsResourceAndIsNotRenewable(t *testing.T) {
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquireHuman(t, srv)
	a, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-42", "example.com", "www.example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if a.GetKind() != "attach" || a.GetResourceId() != "run-42" || a.GetParentGrantId() != parent.GetGrantId() || a.GetZdr() ||
		!equalStrings(a.GetAllowedDomains(), []string{"example.com", "www.example.com"}) {
		t.Fatalf("attach response = %#v", a)
	}
	if a.GetExpiresAt().AsTime().After(parent.GetExpiresAt().AsTime()) || time.Until(a.GetExpiresAt().AsTime()) > grant.AttachMaxTTL {
		t.Fatalf("attach expires %v (parent %v, cap %v)", a.GetExpiresAt().AsTime(), parent.GetExpiresAt().AsTime(), grant.AttachMaxTTL)
	}
	v, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: a.GetGrantId()})
	if err != nil || v.GetKind() != "attach" || v.GetResourceId() != "run-42" || v.GetPolicyMode() != "allowlist" ||
		v.GetParentGrantId() != parent.GetGrantId() || v.GetOwnerId() != "user1" {
		t.Fatalf("validate attach = %#v, %v", v, err)
	}
	if _, err := srv.RenewGrant(context.Background(), renewReq(a.GetGrantId())); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("renew attach: code = %v, want FailedPrecondition", status.Code(err))
	}
	// The resource is required and bounded.
	for name, resource := range map[string]string{"empty": "", "blank": "  ", "long": strings.Repeat("r", 257), "control": "run\n1"} {
		req := attachReq(parent.GetGrantId(), resource, "example.com")
		req.SessionKey = "bad-" + name
		if _, err := srv.DeriveRunGrant(context.Background(), req); status.Code(err) != codes.InvalidArgument {
			t.Fatalf("resource %s: code = %v (%v), want InvalidArgument", name, status.Code(err), err)
		}
	}
	// The attach host bound is its own (BROWSER_BROKER_MAX_ATTACH_DOMAINS).
	tight := humanServer(grant.NewMemoryRepository(), userPrincipal(false), func(c *Config) { c.MaxAttachDomains = 2 })
	p2 := acquireHuman(t, tight)
	_, err = tight.DeriveRunGrant(context.Background(), attachReq(p2.GetGrantId(), "run-1", "a.example.com", "b.example.com", "c.example.com"))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("three hosts under a bound of two: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
}

func TestDeriveAttach_RequiresUserPrincipal(t *testing.T) {
	store := grant.NewMemoryRepository()
	owner := humanServer(store, userPrincipal(false))
	parent := acquireHuman(t, owner)
	svc := humanServer(store, serviceBrowserPrincipal())
	_, err := svc.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "example.com"))
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("service principal: code = %v (%v), want PermissionDenied", status.Code(err), err)
	}
	// ai_run derivation by a scoped service is unchanged.
	if _, err := svc.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"example.com"}
	})); err != nil {
		t.Fatalf("service ai_run derive: %v", err)
	}
}

func TestDeriveAttach_OtherOwnersParentIsNotFound(t *testing.T) {
	store := grant.NewMemoryRepository()
	owner := humanServer(store, userPrincipal(false))
	parent := acquireHuman(t, owner)
	for _, principal := range []authctx.Principal{
		{OrganizationID: "org1", ActorID: "user2", PrincipalType: "user"},
		{OrganizationID: "org2", ActorID: "user1", PrincipalType: "user"},
	} {
		other := humanServer(store, principal)
		req := attachReq(parent.GetGrantId(), "run-1", "example.com")
		req.OrgId = principal.OrganizationID
		if _, err := other.DeriveRunGrant(context.Background(), req); status.Code(err) != codes.NotFound {
			t.Fatalf("%s/%s: code = %v, want NotFound", principal.OrganizationID, principal.ActorID, status.Code(err))
		}
	}
}

func TestDeriveAttach_RejectsInheritFromOpenWeb(t *testing.T) {
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquireHuman(t, srv)
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1")); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("no hosts: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "metadata.goog")); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("a denied host: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
	// From a session grant the hosts must also be named, and be a subset.
	sess := acquire(t, srv, &AcquireGrantRequest{SessionKey: "plain", AllowedDomains: []string{"example.com"}})
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(sess.GetGrantId(), "run-1")); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("session parent, no hosts: code = %v, want InvalidArgument", status.Code(err))
	}
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(sess.GetGrantId(), "run-1", "evil.example")); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("session parent, widening host: code = %v, want PermissionDenied", status.Code(err))
	}
}

func TestDeriveAttach_RejectsZdrParent(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := humanServer(store, userPrincipal(true))
	parent := acquireHuman(t, srv)
	if !parent.GetZdr() {
		t.Fatal("setup: the parent must be ZDR")
	}
	_, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "example.com"))
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("attach under a ZDR parent: code = %v (%v), want FailedPrecondition", status.Code(err), err)
	}
	// The ZDR ai_run path is unchanged.
	if _, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"example.com"}
	})); err != nil {
		t.Fatalf("ZDR ai_run under a ZDR human_browse grant: %v", err)
	}
}

func TestDeriveAttach_ExpiryNeverExceedsParent(t *testing.T) {
	clock := newStepClock()
	store := grant.NewMemoryRepositoryWithClock(clock.Now)
	srv := humanServer(store, userPrincipal(false), func(c *Config) { c.TTL = 5 * time.Minute })
	parent := acquireHuman(t, srv)
	req := attachReq(parent.GetGrantId(), "run-1", "example.com")
	req.TtlSeconds = 3600
	a, err := srv.DeriveRunGrant(context.Background(), req)
	if err != nil {
		t.Fatal(err)
	}
	if got := a.GetExpiresAt().AsTime(); got.After(parent.GetExpiresAt().AsTime()) || !got.Equal(parent.GetExpiresAt().AsTime()) {
		t.Fatalf("attach expires %v, want the parent's %v (the 5-minute parent caps a 15-minute attach)", got, parent.GetExpiresAt().AsTime())
	}
}

func TestRevokeHumanBrowseParentCascadesToAttach(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := humanServer(store, userPrincipal(false))
	parent := acquireHuman(t, srv)
	attach, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: parent.GetGrantId(), Reason: "session_closed"}); err != nil {
		t.Fatal(err)
	}
	for name, id := range map[string]string{"parent": parent.GetGrantId(), "attach": attach.GetGrantId()} {
		if _, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: id}); status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("%s after the revoke: code = %v, want FailedPrecondition", name, status.Code(err))
		}
	}
	if _, err := srv.RenewGrant(context.Background(), renewReq(parent.GetGrantId())); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("renew a revoked parent: code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestSensitiveApprovalUnderAttachParent(t *testing.T) {
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquireHuman(t, srv)
	attach, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	approve := func(hosts ...string) (*AcquireGrantResponse, error) {
		return srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
			OrgId: "org1", SessionKey: "attach-key", AllowedDomains: hosts,
			AllowedActions: []string{"download_ref"}, AllowedFrameIds: []string{"frame-1"}, ParentGrantId: attach.GetGrantId(),
		})
	}
	ok, err := approve("example.com")
	if err != nil {
		t.Fatal(err)
	}
	if ok.GetKind() != "sensitive_approval" || ok.GetParentGrantId() != attach.GetGrantId() ||
		ok.GetExpiresAt().AsTime().After(attach.GetExpiresAt().AsTime()) {
		t.Fatalf("approval = %#v under attach %#v", ok, attach)
	}
	if _, err := approve("evil.example"); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("an approval outside the attach hosts: code = %v, want PermissionDenied", status.Code(err))
	}
	// Revoking the attach revokes the approval.
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: attach.GetGrantId()}); err != nil {
		t.Fatal(err)
	}
	if _, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: ok.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("approval after its attach parent was revoked: code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestHumanBrowseGrantsLogNoHostAndNoOrg(t *testing.T) {
	logs := captureLog(t)
	srv := humanServer(grant.NewMemoryRepository(), userPrincipal(false))
	issued := acquireHuman(t, srv)
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(issued.GetGrantId(), "run-1", "secret-host.example.com")); err != nil {
		t.Fatal(err)
	}
	out := logs.String()
	if strings.Contains(out, "secret-host.example.com") || strings.Contains(out, "org1") {
		t.Fatalf("the broker log carries a host or an org: %s", out)
	}
	if !strings.Contains(out, "human_browse") {
		t.Fatalf("the issue line should name the kind: %s", out)
	}
}

func TestConfigFromEnv_HumanBrowse(t *testing.T) {
	cfg, err := ConfigFromEnv(envOf(nil))
	if err != nil || cfg.HumanBrowseEnabled || cfg.HumanAbsoluteTTL != 4*time.Hour || cfg.MaxAttachDomains != 8 || len(cfg.DenySuffixes) != 0 {
		t.Fatalf("defaults = %#v, %v; human browse must be off, the cap 4h, the attach bound 8", cfg, err)
	}
	cfg, err = ConfigFromEnv(envOf(map[string]string{
		"BROWSER_BROKER_HUMAN_BROWSE_ENABLED":       "1",
		"BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS": "7200",
		"BROWSER_BROKER_MAX_ATTACH_DOMAINS":         "3",
		"BROWSER_BROKER_DENY_SUFFIXES":              ".Corp, intranet.example",
	}))
	if err != nil || !cfg.HumanBrowseEnabled || cfg.HumanAbsoluteTTL != 2*time.Hour || cfg.MaxAttachDomains != 3 ||
		!equalStrings(cfg.DenySuffixes, []string{".corp", "intranet.example"}) {
		t.Fatalf("configured = %#v, %v", cfg, err)
	}
	if cfg, err = ConfigFromEnv(envOf(map[string]string{"BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS": "999999"})); err != nil || cfg.HumanAbsoluteTTL != MaxHumanAbsoluteTTL {
		t.Fatalf("an oversized cap is clamped to %v: %#v, %v", MaxHumanAbsoluteTTL, cfg, err)
	}
	// A typo must stop the broker, not turn open-web browsing on or off.
	for name, env := range map[string]map[string]string{
		"switch":      {"BROWSER_BROKER_HUMAN_BROWSE_ENABLED": "yes"},
		"cap":         {"BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS": "four hours"},
		"cap zero":    {"BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS": "0"},
		"attach":      {"BROWSER_BROKER_MAX_ATTACH_DOMAINS": "-1"},
		"deny-list":   {"BROWSER_BROKER_DENY_SUFFIXES": "http://x.internal"},
		"empty entry": {"BROWSER_BROKER_DENY_SUFFIXES": ".internal,,.local"},
	} {
		if _, err := ConfigFromEnv(envOf(env)); err == nil {
			t.Fatalf("%s: an invalid value was accepted", name)
		}
	}
}
