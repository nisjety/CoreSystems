package server

import (
	"context"
	"strings"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func deriveReq(parent string, mutate ...func(*DeriveRunGrantRequest)) *DeriveRunGrantRequest {
	req := &DeriveRunGrantRequest{
		ParentGrantId: parent,
		SessionKey:    "browser-01J",
		OrgId:         "org1",
		Purpose:       "ai_run",
	}
	for _, m := range mutate {
		m(req)
	}
	return req
}

func TestDeriveRunGrant_IssuesAValidatableRunGrantUnderTheParent(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "example.org"}})

	resp, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"docs.example.com"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	if resp.GetKind() != "run" || resp.GetParentGrantId() != parent.GetGrantId() || resp.GetZdr() ||
		len(resp.GetAllowedDomains()) != 1 || resp.GetAllowedDomains()[0] != "docs.example.com" ||
		!strings.HasSuffix(resp.GetEndpoint(), "/grants/"+resp.GetGrantId()) {
		t.Fatalf("derive response = %#v", resp)
	}
	if resp.GetExpiresAt().AsTime().After(parent.GetExpiresAt().AsTime()) {
		t.Fatal("run grant outlives its parent")
	}
	got, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	if got.GetKind() != "run" || got.GetOwnerId() != "user1" || got.GetParentGrantId() != parent.GetGrantId() {
		t.Fatalf("validate derived = %#v", got)
	}
}

func TestDeriveRunGrant_RejectsMalformedRequestsBeforeTouchingGrants(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{})
	for name, tc := range map[string]struct {
		mutate func(*DeriveRunGrantRequest)
		want   codes.Code
	}{
		"no parent":        {func(r *DeriveRunGrantRequest) { r.ParentGrantId = "" }, codes.InvalidArgument},
		"no key":           {func(r *DeriveRunGrantRequest) { r.SessionKey = " " }, codes.InvalidArgument},
		"control key":      {func(r *DeriveRunGrantRequest) { r.SessionKey = "a\nb" }, codes.InvalidArgument},
		"long key":         {func(r *DeriveRunGrantRequest) { r.SessionKey = strings.Repeat("k", 257) }, codes.InvalidArgument},
		"no org":           {func(r *DeriveRunGrantRequest) { r.OrgId = "" }, codes.InvalidArgument},
		"other org":        {func(r *DeriveRunGrantRequest) { r.OrgId = "org2" }, codes.PermissionDenied},
		"no purpose":       {func(r *DeriveRunGrantRequest) { r.Purpose = "" }, codes.InvalidArgument},
		"other purpose":    {func(r *DeriveRunGrantRequest) { r.Purpose = "bogus" }, codes.InvalidArgument},
		"negative ttl":     {func(r *DeriveRunGrantRequest) { r.TtlSeconds = -1 }, codes.InvalidArgument},
		"ttl under 30 s":   {func(r *DeriveRunGrantRequest) { r.TtlSeconds = 29 }, codes.InvalidArgument},
		"wildcard host":    {func(r *DeriveRunGrantRequest) { r.AllowedDomains = []string{"*.example.com"} }, codes.InvalidArgument},
		"widening host":    {func(r *DeriveRunGrantRequest) { r.AllowedDomains = []string{"evil.com"} }, codes.PermissionDenied},
		"internal subhost": {func(r *DeriveRunGrantRequest) { r.AllowedDomains = []string{"x.example.com.internal"} }, codes.PermissionDenied},
	} {
		t.Run(name, func(t *testing.T) {
			_, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), tc.mutate))
			if status.Code(err) != tc.want {
				t.Fatalf("code = %v (%v), want %v", status.Code(err), err, tc.want)
			}
		})
	}
}

func TestDeriveRunGrant_StatusCodesForEachRule(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := testServerFor(store, userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{})
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId()))
	if err != nil {
		t.Fatal(err)
	}

	// A run grant is not a parent.
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(run.GetGrantId()))
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("chained derive: %v", err)
	}
	// Replay with a different policy.
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = []string{"www.example.com"}
	}))
	if status.Code(err) != codes.AlreadyExists {
		t.Fatalf("conflicting replay: %v", err)
	}
	// Replay with the same policy is the same grant.
	again, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId()))
	if err != nil || again.GetGrantId() != run.GetGrantId() {
		t.Fatalf("idempotent replay: %v, %v", again, err)
	}
	// A ZDR principal cannot derive from a non-ZDR parent.
	zdrSrv := testServerFor(store, userPrincipal(true))
	_, err = zdrSrv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "zdr-run"
	}))
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("ZDR posture change: %v", err)
	}
	// Another user of the same tenant, and another tenant, see nothing.
	for _, principal := range []authctx.Principal{
		{OrganizationID: "org1", ActorID: "user2", PrincipalType: "user"},
		{OrganizationID: "org2", ActorID: "user1", PrincipalType: "user"},
	} {
		other := testServerFor(store, principal)
		_, err = other.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
			r.OrgId = principal.OrganizationID
			r.SessionKey = "steal"
		}))
		if status.Code(err) != codes.NotFound {
			t.Fatalf("%s/%s: %v, want NotFound", principal.OrganizationID, principal.ActorID, err)
		}
	}
	// A revoked parent.
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: parent.GetGrantId()}); err != nil {
		t.Fatal(err)
	}
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "after-revoke"
	}))
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("revoked parent: %v", err)
	}
	if _, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: run.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("run grant after parent revoke: %v", err)
	}
}

func TestDeriveRunGrant_WideParentNeedsAnExplicitSubset(t *testing.T) {
	cfg := DefaultConfig()
	cfg.MaxSessionDomains = 12
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	wide := make([]string, 9)
	for i := range wide {
		wide[i] = "h" + string(rune('a'+i)) + ".example.com"
	}
	parent := acquire(t, srv, &AcquireGrantRequest{AllowedDomains: wide})
	if _, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId())); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("inheriting 9 hosts: %v, want InvalidArgument", err)
	}
	resp, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = wide[:2]
	}))
	if err != nil || len(resp.GetAllowedDomains()) != 2 {
		t.Fatalf("explicit subset: %v, %v", resp, err)
	}
}

func TestDeriveRunGrant_TTLIsDefaultedClampedAndCappedByTheParent(t *testing.T) {
	cfg := DefaultConfig()
	cfg.TTL = 10 * time.Minute
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	parent := acquire(t, srv, &AcquireGrantRequest{})
	long, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.TtlSeconds = 7200
	}))
	if err != nil {
		t.Fatal(err)
	}
	if !long.GetExpiresAt().AsTime().Equal(parent.GetExpiresAt().AsTime()) {
		t.Fatalf("a 2 h request must stop at the parent's expiry: %v vs %v", long.GetExpiresAt().AsTime(), parent.GetExpiresAt().AsTime())
	}
	short, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey, r.TtlSeconds = "short", 60
	}))
	if err != nil {
		t.Fatal(err)
	}
	if left := time.Until(short.GetExpiresAt().AsTime()); left > 61*time.Second || left < 50*time.Second {
		t.Fatalf("60 s request gave %v", left)
	}
}

func TestDeriveRunGrant_ServiceDeriveKeepsTheParentsOwner(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := testServerFor(store, userPrincipal(false))
	parent := acquire(t, user, &AcquireGrantRequest{})
	service := testServerFor(store, authctx.Principal{
		OrganizationID: "org1", ActorID: "service:model-gateway", PrincipalType: "service",
		Scopes: []string{"browser:write"},
	})
	resp, err := service.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId()))
	if err != nil {
		t.Fatal(err)
	}
	got, err := user.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil || got.GetOwnerId() != "user1" {
		t.Fatalf("service-derived grant owner: %#v, %v", got, err)
	}
}

func TestDeriveRunGrant_TransportRoundTrip(t *testing.T) {
	client := newTestClient(t)
	parent, err := client.AcquireGrant(context.Background(), &mpv1.AcquireGrantRequest{
		SessionKey: "s1", OrgId: "org1", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}
	resp, err := client.DeriveRunGrant(context.Background(), &mpv1.DeriveRunGrantRequest{
		ParentGrantId: parent.GetGrantId(), SessionKey: "run-1", OrgId: "org1", Purpose: "ai_run",
	})
	if err != nil {
		t.Fatal(err)
	}
	if resp.GetKind() != "run" || resp.GetParentGrantId() != parent.GetGrantId() {
		t.Fatalf("round trip = %#v", resp)
	}
}

func TestConfigFromEnv_MaxRunDomains(t *testing.T) {
	cfg, err := ConfigFromEnv(envOf(nil))
	if err != nil || cfg.MaxRunDomains != 8 {
		t.Fatalf("default = %d, %v", cfg.MaxRunDomains, err)
	}
	cfg, err = ConfigFromEnv(envOf(map[string]string{EnvMaxRunDomains: "3"}))
	if err != nil || cfg.MaxRunDomains != 3 {
		t.Fatalf("configured = %d, %v", cfg.MaxRunDomains, err)
	}
	cfg, err = ConfigFromEnv(envOf(map[string]string{EnvMaxRunDomains: "999"}))
	if err != nil || cfg.MaxRunDomains != 32 {
		t.Fatalf("clamped = %d, %v", cfg.MaxRunDomains, err)
	}
	for _, bad := range []string{"0", "-1", "x"} {
		if _, err := ConfigFromEnv(envOf(map[string]string{EnvMaxRunDomains: bad})); err == nil {
			t.Fatalf("%q accepted", bad)
		}
	}
}

// BROWSER_BROKER_MAX_RUN_DOMAINS reaches DeriveRunGrant: with the bound set to
// 2 through ConfigFromEnv, a 3-host explicit list is refused, and a 3-host
// parent can no longer be inherited implicitly but can be narrowed to 2.
func TestDeriveRunGrant_HonoursTheConfiguredMaxRunDomains(t *testing.T) {
	cfg, err := ConfigFromEnv(envOf(map[string]string{EnvMaxRunDomains: "2"}))
	if err != nil {
		t.Fatal(err)
	}
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	three := []string{"a.example.com", "b.example.com", "c.example.com"}
	parent := acquire(t, srv, &AcquireGrantRequest{AllowedDomains: three})

	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.AllowedDomains = three
	}))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("3-host explicit list under a bound of 2: %v, want InvalidArgument", err)
	}
	_, err = srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "browser-inherit"
	}))
	if status.Code(err) == codes.OK {
		t.Fatal("a 3-host parent was inherited under a bound of 2")
	}
	resp, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "browser-subset"
		r.AllowedDomains = three[:2]
	}))
	if err != nil || len(resp.GetAllowedDomains()) != 2 {
		t.Fatalf("2-host subset: %#v, %v", resp, err)
	}
}

// Run grants do not consume the owner's cap: with MaxActivePerOwner = 2, a
// user can still open a second browser session after starting several runs.
func TestDeriveRunGrant_RunsDoNotBlockANewSession(t *testing.T) {
	cfg := DefaultConfig()
	cfg.MaxActivePerOwner = 2
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	parent := acquire(t, srv, &AcquireGrantRequest{})
	for i := 0; i < 4; i++ {
		key := "browser-run-" + string(rune('a'+i))
		if _, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
			r.SessionKey = key
		})); err != nil {
			t.Fatalf("derive %d: %v", i, err)
		}
	}
	acquire(t, srv, &AcquireGrantRequest{SessionKey: "session2"})
}
