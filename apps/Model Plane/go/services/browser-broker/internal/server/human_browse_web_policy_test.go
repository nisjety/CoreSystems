package server

import (
	"context"
	"encoding/json"
	"slices"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// WS7.K: a human_browse grant carries the org web-policy snapshot of the person's
// token (field 20), so an attach child derived from it passes the snapshot's deny
// rules, exactly as an ordinary grant's children do.

// humanWebPolicyServer is a human_browse server with BROWSER_BROKER_WEB_POLICY_
// SNAPSHOT set to on, acting as principal p.
func humanWebPolicyServer(on bool, p authctx.Principal) *Server {
	return humanServer(grant.NewMemoryRepository(), p, func(c *Config) { c.WebPolicySnapshot = on })
}

func TestHumanBrowse_CarriesTheOrgWebPolicyToItsAttachChildren(t *testing.T) {
	owner := userPrincipal(false)
	owner.WebPolicy = json.RawMessage(`{"v":1,"rev":"2","domains":{"deny":["evil.example.com"]}}`)
	srv := humanWebPolicyServer(true, owner)
	ctx := context.Background()
	parent := acquireHuman(t, srv)
	if got := parent.GetWebPolicy().GetDomainsDeny(); !slices.Equal(got, []string{"evil.example.com"}) {
		t.Fatalf("human_browse acquire web_policy deny = %v, want the org rule", got)
	}
	v, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: parent.GetGrantId()})
	if err != nil || !slices.Equal(v.GetWebPolicy().GetDomainsDeny(), []string{"evil.example.com"}) {
		t.Fatalf("validate human_browse = %#v, %v; want the snapshot", v.GetWebPolicy(), err)
	}
	// The attach is asked with a token that carries no claim, so a refusal can only
	// come from the snapshot the parent stores and the child copies.
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	// A grant covers the subdomains of each host it names, so an attach child may
	// not name the denied host or a host that covers it.
	for _, c := range []struct{ key, host string }{{"named", "evil.example.com"}, {"covering", "example.com"}} {
		req := attachReq(parent.GetGrantId(), "run-1", c.host)
		req.SessionKey = "attach-" + c.key
		if _, err := srv.DeriveRunGrant(ctx, req); status.Code(err) != codes.InvalidArgument {
			t.Fatalf("attach %s %s: code = %v (%v), want InvalidArgument", c.key, c.host, status.Code(err), err)
		}
	}
	beside := attachReq(parent.GetGrantId(), "run-1", "docs.example.com")
	beside.SessionKey = "attach-beside"
	a, err := srv.DeriveRunGrant(ctx, beside)
	if err != nil || !slices.Equal(a.GetAllowedDomains(), []string{"docs.example.com"}) ||
		!slices.Equal(a.GetWebPolicy().GetDomainsDeny(), []string{"evil.example.com"}) {
		t.Fatalf("attach beside the denied host = %#v, %v", a, err)
	}
}

// With BROWSER_BROKER_WEB_POLICY_SNAPSHOT off (the default) nothing is read: the
// grant carries no snapshot and an attach to a host the claim denies is granted.
func TestHumanBrowse_WebPolicyIsDarkWhenTheSwitchIsOff(t *testing.T) {
	owner := userPrincipal(false)
	owner.WebPolicy = json.RawMessage(`{"v":1,"rev":"2","domains":{"deny":["evil.example.com"]}}`)
	srv := humanWebPolicyServer(false, owner)
	parent := acquireHuman(t, srv)
	if parent.GetWebPolicy() != nil {
		t.Fatalf("switch off: acquire web_policy = %#v, want none", parent.GetWebPolicy())
	}
	if _, err := srv.DeriveRunGrant(context.Background(), attachReq(parent.GetGrantId(), "run-1", "evil.example.com")); err != nil {
		t.Fatalf("switch off: attach to a host the claim denies: %v, want it granted (nothing is read)", err)
	}
}

// An unreadable web_policy claim refuses the human_browse mint, as it refuses an
// ordinary grant (InvalidArgument web_policy_unreadable).
func TestHumanBrowse_UnreadableWebPolicyRefusesTheMint(t *testing.T) {
	owner := userPrincipal(false)
	owner.WebPolicy = json.RawMessage(`{"v":2,"rev":"1"}`)
	srv := humanWebPolicyServer(true, owner)
	if _, err := srv.AcquireGrant(context.Background(), humanReq()); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("unreadable claim: code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
}
