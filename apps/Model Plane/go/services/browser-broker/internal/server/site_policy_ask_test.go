package server

import (
	"context"
	"sort"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// sortedHosts returns a sorted copy of hosts, so a test can compare host lists
// without depending on the order they were built in.
func sortedHosts(hosts []string) []string {
	out := append([]string(nil), hosts...)
	sort.Strings(out)
	return out
}

// equalHosts reports whether two host lists hold the same hosts in the same order.
func equalHosts(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// refusalInfo returns the ErrorInfo a refusal carries, or nil when it carries none.
func refusalInfo(t *testing.T, err error) *errdetails.ErrorInfo {
	t.Helper()
	st, ok := status.FromError(err)
	if !ok {
		t.Fatalf("refusal is not a gRPC status: %v", err)
	}
	for _, detail := range st.Details() {
		if info, ok := detail.(*errdetails.ErrorInfo); ok {
			return info
		}
	}
	return nil
}

// splitHostList reads a comma-joined host list from ErrorInfo metadata, sorted.
func splitHostList(joined string) []string {
	if joined == "" {
		return nil
	}
	return sortedHosts(strings.Split(joined, ","))
}

// TestSplitByPolicyTable pins the three-way split at both levels. An ask host is
// its own slice whatever the level; deny, an unanswered host and a read host of an
// act grant are denied.
func TestSplitByPolicyTable(t *testing.T) {
	resolution := sitepolicy.Resolution{Decisions: []sitepolicy.Decision{
		{Host: "act.example", Effective: sitepolicy.EffectiveAct},
		{Host: "read.example", Effective: sitepolicy.EffectiveRead},
		{Host: "ask.example", Effective: sitepolicy.EffectiveAsk},
		{Host: "deny.example", Effective: sitepolicy.EffectiveDeny},
	}}
	domains := []string{"act.example", "read.example", "ask.example", "deny.example", "unanswered.example"}
	cases := []struct {
		name     string
		readOnly bool
		allowed  []string
		asked    []string
		denied   []string
	}{
		{
			name:    "act grant takes act hosts, asks for ask hosts, denies the rest",
			allowed: []string{"act.example"},
			asked:   []string{"ask.example"},
			denied:  []string{"read.example", "deny.example", "unanswered.example"},
		},
		{
			name:     "read grant also takes read hosts and still asks for ask hosts",
			readOnly: true,
			allowed:  []string{"act.example", "read.example"},
			asked:    []string{"ask.example"},
			denied:   []string{"deny.example", "unanswered.example"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			allowed, asked, denied := splitByPolicy(domains, resolution, tc.readOnly)
			if !equalHosts(allowed, tc.allowed) || !equalHosts(asked, tc.asked) || !equalHosts(denied, tc.denied) {
				t.Fatalf("split = allowed %v, asked %v, denied %v", allowed, asked, denied)
			}
		})
	}
}

// An enforce mint where every requested host is an ask is refused as an ask: the
// refusal names the hosts under ask_domains, grants nothing, writes nothing, and
// counts under the site_policy_ask reason. No host reaches the message.
func TestSitePolicyEnforceAllAskRefusesAsAskAndGrantsNothing(t *testing.T) {
	recs := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	srv := policyServer(store, userPrincipal(false), sitepolicy.ModeEnforce, &fakeSites{})
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com", "docs.example.com"}})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
	info := refusalInfo(t, err)
	if info == nil || info.GetReason() != siteAskReason {
		t.Fatalf("details = %v, want reason %s", status.Convert(err).Details(), siteAskReason)
	}
	if got := splitHostList(info.GetMetadata()["ask_domains"]); !equalHosts(got, []string{"docs.example.com", "example.com"}) {
		t.Fatalf("ask_domains = %v, want both hosts", got)
	}
	if _, named := info.GetMetadata()["denied_domains"]; named {
		t.Fatal("an all-ask refusal named denied domains")
	}
	if msg := status.Convert(err).Message(); msg != "browser site policy needs the member to decide for every requested domain" {
		t.Fatalf("message = %q", msg)
	}
	if store.writes != 0 {
		t.Fatalf("the refusal wrote %d grants", store.writes)
	}
	if len(recs.reasons) != 1 || recs.reasons[0] != "site_policy_ask" {
		t.Fatalf("denial reasons = %v, want [site_policy_ask]", recs.reasons)
	}
}

// The ask_domains a refusal names are bounded like denied_domains.
func TestSitePolicyAllAskNamesAtMostNineAskHosts(t *testing.T) {
	hosts := []string{
		"a.example.com", "b.example.com", "c.example.com", "d.example.com", "e.example.com",
		"f.example.com", "g.example.com", "h.example.com", "i.example.com", "j.example.com",
	}
	cfg := DefaultConfig()
	cfg.MaxSessionDomains = 16
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	srv.WithOptions(Options{SitePolicy: sitepolicy.ModeEnforce, SiteResolver: &fakeSites{}})
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: hosts})
	info := refusalInfo(t, err)
	if info == nil || info.GetReason() != siteAskReason {
		t.Fatalf("details = %v, want reason %s", status.Convert(err).Details(), siteAskReason)
	}
	if got := len(splitHostList(info.GetMetadata()["ask_domains"])); got != maxDeniedDomainsReported {
		t.Fatalf("ask_domains named %d hosts, want %d", got, maxDeniedDomainsReported)
	}
}

// A mixed refusal (no host allowed, some denied, some asks) stays BROWSER_SITE_DENIED
// and carries ask_domains too, so a caller can tell the two apart.
func TestSitePolicyMixedAskAndDenyStaysDeniedAndNamesBoth(t *testing.T) {
	recs := recordDenials(t)
	sites := &fakeSites{effective: map[string]string{"blocked.example.org": sitepolicy.EffectiveDeny}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"blocked.example.org", "example.com"}})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
	info := refusalInfo(t, err)
	if info == nil || info.GetReason() != siteDeniedReason {
		t.Fatalf("details = %v, want reason %s", status.Convert(err).Details(), siteDeniedReason)
	}
	if got := info.GetMetadata()["denied_domains"]; got != "blocked.example.org" {
		t.Fatalf("denied_domains = %q, want blocked.example.org", got)
	}
	if got := info.GetMetadata()["ask_domains"]; got != "example.com" {
		t.Fatalf("ask_domains = %q, want example.com", got)
	}
	if len(recs.reasons) != 1 || recs.reasons[0] != "site_policy" {
		t.Fatalf("denial reasons = %v, want [site_policy]", recs.reasons)
	}
}

// A pure denial is unchanged: BROWSER_SITE_DENIED naming the denied hosts, and no
// ask_domains key at all.
func TestSitePolicyPureDenialCarriesNoAskDomains(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{"blocked.example.org": sitepolicy.EffectiveDeny}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"blocked.example.org"}})
	info := refusalInfo(t, err)
	if info == nil || info.GetReason() != siteDeniedReason || info.GetMetadata()["denied_domains"] != "blocked.example.org" {
		t.Fatalf("details = %v, want reason %s naming blocked.example.org", status.Convert(err).Details(), siteDeniedReason)
	}
	if _, named := info.GetMetadata()["ask_domains"]; named {
		t.Fatal("a pure denial named ask domains")
	}
}

// When some hosts are allowed the grant narrows as before, and the hosts it drops
// include the ask hosts, in the order of the request.
func TestSitePolicyNarrowedGrantNamesItsAskHostsAsDropped(t *testing.T) {
	sites := &fakeSites{effective: map[string]string{
		"example.com":         sitepolicy.EffectiveAct,
		"blocked.example.org": sitepolicy.EffectiveDeny,
	}}
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeEnforce, sites)
	resp, err := issue(srv, &AcquireGrantRequest{
		AllowedDomains: []string{"blocked.example.org", "docs.example.com", "example.com"},
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if got := resp.GetAllowedDomains(); !equalHosts(got, []string{"example.com"}) {
		t.Fatalf("allowed = %v, want [example.com]", got)
	}
	if got := resp.GetDeniedDomains(); !equalHosts(sortedHosts(got), []string{"blocked.example.org", "docs.example.com"}) {
		t.Fatalf("denied = %v, want the deny host and the ask host", got)
	}
}

// Shadow never refuses on the verdict: an all-ask request is granted as asked and
// nothing is counted as a denial.
func TestSitePolicyShadowIssuesAnAllAskRequestAndCountsNoDenial(t *testing.T) {
	recs := recordDenials(t)
	srv := policyServer(grant.NewMemoryRepository(), userPrincipal(false), sitepolicy.ModeShadow, &fakeSites{})
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}})
	if err != nil {
		t.Fatalf("shadow refused an ask: %v", err)
	}
	if got := resp.GetAllowedDomains(); !equalHosts(got, []string{"example.com"}) || len(resp.GetDeniedDomains()) != 0 {
		t.Fatalf("shadow narrowed the grant: %+v", resp)
	}
	if len(recs.reasons) != 0 {
		t.Fatalf("shadow counted a denial: %v", recs.reasons)
	}
}
