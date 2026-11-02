package server

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// fakeSites answers from a fixed map; a host not in it is ask, as org-core answers
// a member with no live choice.
type fakeSites struct {
	effective map[string]string
	err       error
	calls     int
	lastUser  string
}

func (f *fakeSites) Resolve(_ context.Context, _, userID string, hosts []string) (sitepolicy.Resolution, error) {
	f.calls++
	f.lastUser = userID
	if f.err != nil {
		return sitepolicy.Resolution{}, f.err
	}
	var out sitepolicy.Resolution
	for _, host := range hosts {
		effective, ok := f.effective[host]
		if !ok {
			effective = sitepolicy.EffectiveAsk
		}
		out.Decisions = append(out.Decisions, sitepolicy.Decision{Host: host, Effective: effective})
	}
	return out, nil
}

func policyServer(store grant.Repository, principal authctx.Principal, mode sitepolicy.Mode, sites sitepolicy.Resolver) *Server {
	return testServerFor(store, principal).WithOptions(Options{SitePolicy: mode, SiteResolver: sites})
}

func issue(srv *Server, req *AcquireGrantRequest) (*AcquireGrantResponse, error) {
	if req.OrgId == "" {
		req.OrgId = "org1"
	}
	if req.SessionKey == "" {
		req.SessionKey = "session1"
	}
	return srv.AcquireGrant(context.Background(), req)
}

func TestSitePolicyOffMakesNoCallAndGrantsAsRequested(t *testing.T) {
	sites := &fakeSites{}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeOff, sites)
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "docs.example.com"}})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if sites.calls != 0 {
		t.Fatalf("off made %d org-core calls", sites.calls)
	}
	if len(resp.GetAllowedDomains()) != 2 || len(resp.GetDeniedDomains()) != 0 || resp.GetReadOnly() {
		t.Fatalf("grant = %+v, want both hosts and no narrowing", resp)
	}
}

func TestSitePolicyShadowGrantsAsRequestedButReportsNothingDenied(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeShadow, sites)
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "docs.example.com"}})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if sites.calls != 1 {
		t.Fatalf("shadow calls = %d, want 1", sites.calls)
	}
	if len(resp.GetAllowedDomains()) != 2 || len(resp.GetDeniedDomains()) != 0 {
		t.Fatalf("shadow narrowed the grant: %+v", resp)
	}
}

func TestSitePolicyEnforceKeepsActHostsAndNamesTheDropped(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct, "blocked.example.org": sitepolicy.EffectiveDeny}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "docs.example.com", "blocked.example.org"}})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if got := resp.GetAllowedDomains(); len(got) != 1 || got[0] != "example.com" {
		t.Fatalf("allowed = %v, want [example.com]", got)
	}
	if got := resp.GetDeniedDomains(); len(got) != 2 || got[0] != "blocked.example.org" || got[1] != "docs.example.com" {
		t.Fatalf("denied = %v", got)
	}
	if sites.lastUser != "user1" {
		t.Fatalf("org-core asked for user %q, want the verified member", sites.lastUser)
	}
}

func TestSitePolicyEnforceRefusesWhenNothingIsAllowed(t *testing.T) {
	// Ask is the default for a host with no live choice: enforce refuses it.
	sites := &fakeSites{}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
	var info *errdetails.ErrorInfo
	for _, detail := range status.Convert(err).Details() {
		if typed, ok := detail.(*errdetails.ErrorInfo); ok {
			info = typed
		}
	}
	if info == nil || info.GetReason() != siteDeniedReason || info.GetMetadata()["denied_domains"] != "example.com" {
		t.Fatalf("details = %v, want reason %s naming example.com", status.Convert(err).Details(), siteDeniedReason)
	}
}

func TestSitePolicyReadRequestTakesReadAndActHostsAsReadOnly(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{
		"example.com":      sitepolicy.EffectiveAct,
		"docs.example.com": sitepolicy.EffectiveRead,
	}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(true), sitepolicy.ModeEnforce, sites)
	resp, err := issue(srv, &AcquireGrantRequest{
		AllowedDomains: []string{"example.com", "docs.example.com"}, ReadOnly: true,
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if len(resp.GetAllowedDomains()) != 2 || !resp.GetReadOnly() {
		t.Fatalf("grant = %+v, want both hosts read-only", resp)
	}
	validated, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant: %v", err)
	}
	if !validated.GetReadOnly() {
		t.Fatal("ValidateGrant lost read_only")
	}
}

func TestSitePolicyActRequestDropsReadOnlyHosts(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{
		"example.com":      sitepolicy.EffectiveAct,
		"docs.example.com": sitepolicy.EffectiveRead,
	}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "docs.example.com"}})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if got := resp.GetAllowedDomains(); len(got) != 1 || got[0] != "example.com" || resp.GetReadOnly() {
		t.Fatalf("grant = %+v, want act host only", resp)
	}
}

func TestSitePolicyEnforceFailsClosedWhenOrgCoreIsDown(t *testing.T) {
	sites := &fakeSites{err: sitepolicy.ErrUnavailable}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}})
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("code = %v, want Unavailable", status.Code(err))
	}
}

func TestSitePolicyShadowIssuesWhenOrgCoreIsDown(t *testing.T) {
	sites := &fakeSites{err: sitepolicy.ErrUnavailable}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeShadow, sites)
	if _, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}}); err != nil {
		t.Fatalf("shadow refused on an outage: %v", err)
	}
}

func TestSitePolicyEnforceWithoutAResolverFailsClosed(t *testing.T) {
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, nil)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}})
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("code = %v, want Unavailable", status.Code(err))
	}
}

func TestDeniedDomainsNamedAreCappedAtNine(t *testing.T) {
	hosts := []string{
		"a.example.com", "b.example.com", "c.example.com", "d.example.com", "e.example.com",
		"f.example.com", "g.example.com", "h.example.com", "i.example.com", "j.example.com",
	}
	sites := &fakeSites{effective: map[string]string{"zz.example.com": sitepolicy.EffectiveAct}}
	cfg := DefaultConfig()
	cfg.MaxSessionDomains = 16
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	srv.WithOptions(Options{SitePolicy: sitepolicy.ModeEnforce, SiteResolver: sites})
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: append([]string{"zz.example.com"}, hosts...)})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if got := len(resp.GetDeniedDomains()); got != maxDeniedDomainsReported {
		t.Fatalf("denied named %d hosts, want %d", got, maxDeniedDomainsReported)
	}
}

func TestReadOnlyIsRefusedForADurableGrant(t *testing.T) {
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeOff, nil)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestReadOnlyIsRefusedOnASensitiveApproval(t *testing.T) {
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(true), sitepolicy.ModeOff, nil)
	_, err := issue(srv, &AcquireGrantRequest{
		AllowedDomains: []string{"example.com"}, ReadOnly: true,
		AllowedActions: []string{"respond_dialog"}, ParentGrantId: "parent",
	})
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestReadOnlyPropagatesToDerivedRunGrant(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := policyServer(store, userPrincipal(true), sitepolicy.ModeOff, nil)
	parent, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	run, err := srv.DeriveRunGrant(context.Background(), &DeriveRunGrantRequest{
		ParentGrantId: parent.GetGrantId(), SessionKey: "run1", OrgId: "org1", Purpose: derivePurposeAIRun,
	})
	if err != nil {
		t.Fatalf("DeriveRunGrant: %v", err)
	}
	validated, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: run.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant: %v", err)
	}
	if !validated.GetReadOnly() {
		t.Fatal("a run derived from a read-only grant is not read-only")
	}
}

func TestRequestedLifetimeIsDefaultedFloorChecksAndCapped(t *testing.T) {
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeOff, nil)
	cases := []struct {
		name    string
		seconds int32
		want    time.Duration
		code    codes.Code
	}{
		{"zero is the configured default", 0, DefaultGrantTTL, codes.OK},
		{"a request inside the range is honoured", 120, 2 * time.Minute, codes.OK},
		{"a request over the cap is clamped", 7200, DefaultMaxRequestTTL, codes.OK},
		{"a request under the minimum is refused", 5, 0, codes.InvalidArgument},
		{"a negative request is refused", -1, 0, codes.InvalidArgument},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			before := time.Now()
			resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, TtlSeconds: tc.seconds})
			if status.Code(err) != tc.code {
				t.Fatalf("code = %v, want %v (%v)", status.Code(err), tc.code, err)
			}
			if tc.code != codes.OK {
				return
			}
			lifetime := resp.GetExpiresAt().AsTime().Sub(before)
			if lifetime < tc.want-time.Minute || lifetime > tc.want+time.Minute {
				t.Fatalf("lifetime = %v, want about %v", lifetime, tc.want)
			}
		})
	}
}
