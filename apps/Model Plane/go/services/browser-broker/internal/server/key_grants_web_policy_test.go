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

// WS7.K: an API-key grant is an ordinary allow-list grant, so the org web
// policy on the key's token narrows its hosts and the grant stores the snapshot
// (field 20) exactly as an ordinary grant does. The key's host ceiling still
// holds for the hosts that are granted.

// webPolicyKey is the key caller whose token carries raw as its web_policy
// claim (no claim when raw is empty) and the given host ceiling (the default
// ceiling when nil).
func webPolicyKey(raw string, ceiling []string) authctx.Principal {
	p := keyCaller("key-1")
	if raw != "" {
		p.WebPolicy = json.RawMessage(raw)
	}
	if ceiling != nil {
		p.KeyHostCeiling = ceiling
	}
	return p
}

// keyWebPolicyServer is a key server with BROWSER_BROKER_WEB_POLICY_SNAPSHOT
// set to on, acting as principal p.
func keyWebPolicyServer(on bool, p authctx.Principal) *Server {
	cfg := DefaultConfig()
	cfg.KeyGrantsEnabled = true
	cfg.WebPolicySnapshot = on
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return p, nil }
	return srv
}

func TestKeyGrants_WebPolicyNarrowsAndIsStoredLikeAnOrdinaryGrant(t *testing.T) {
	cases := []struct {
		name  string
		on    bool
		raw   string
		hosts []string
		code  codes.Code
		snap  bool
	}{
		{"allow narrows and stores the snapshot", true, `{"v":1,"rev":"1","domains":{"allow":["docs.example.com"]}}`, []string{"docs.example.com"}, codes.OK, true},
		{"deny refuses the mint", true, `{"v":1,"rev":"1","domains":{"deny":["example.com"]}}`, nil, codes.InvalidArgument, false},
		{"unreadable claim refuses the mint", true, `{"v":2,"rev":"1"}`, nil, codes.InvalidArgument, false},
		{"no claim mints unchanged", true, "", []string{"example.com"}, codes.OK, false},
		{"switch off ignores the claim", false, `{"v":1,"rev":"1","domains":{"allow":["docs.example.com"]}}`, []string{"example.com"}, codes.OK, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			srv := keyWebPolicyServer(c.on, webPolicyKey(c.raw, nil))
			ctx := context.Background()
			resp, err := srv.AcquireGrant(ctx, keyAcquireReq())
			if status.Code(err) != c.code {
				t.Fatalf("acquire code %v (%v), want %v", status.Code(err), err, c.code)
			}
			if c.code != codes.OK {
				return
			}
			if got := resp.GetAllowedDomains(); !slices.Equal(got, c.hosts) {
				t.Fatalf("acquire allowed_domains = %v, want %v", got, c.hosts)
			}
			if (resp.GetWebPolicy() != nil) != c.snap {
				t.Fatalf("acquire web_policy = %#v, want a snapshot: %v", resp.GetWebPolicy(), c.snap)
			}
			v, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: resp.GetGrantId()})
			if err != nil {
				t.Fatalf("validate as the key: %v", err)
			}
			if !slices.Equal(v.GetAllowedDomains(), c.hosts) || (v.GetWebPolicy() != nil) != c.snap {
				t.Fatalf("validate = %#v, want hosts %v and a snapshot: %v", v, c.hosts, c.snap)
			}
		})
	}
}

// The host ceiling is checked on the requested hosts and again on the hosts the
// org policy leaves: a narrowed subdomain outside the ceiling is refused (OD-6),
// and a requested host outside it is refused, not dropped by the allow rule.
func TestKeyGrants_WebPolicyKeepsTheHostCeiling(t *testing.T) {
	allowDocs := `{"v":1,"rev":"1","domains":{"allow":["docs.example.com"]}}`
	cases := []struct {
		name    string
		ceiling []string
		asked   []string
		hosts   []string
		reason  string
	}{
		{"narrowed host inside the ceiling is granted", []string{"example.com", "docs.example.com"}, []string{"example.com"}, []string{"docs.example.com"}, ""},
		{"narrowed subdomain outside the ceiling is refused", []string{"example.com"}, []string{"example.com"}, nil, reasonKeyHostCeiling},
		{"requested host outside the ceiling is refused, not dropped", []string{"example.com", "docs.example.com"}, []string{"docs.example.com", "evil.example"}, nil, reasonKeyHostCeiling},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			srv := keyWebPolicyServer(true, webPolicyKey(allowDocs, c.ceiling))
			req := keyAcquireReq()
			req.AllowedDomains = c.asked
			resp, err := srv.AcquireGrant(context.Background(), req)
			if c.reason != "" {
				if status.Code(err) != codes.PermissionDenied || keyReason(err) != c.reason {
					t.Fatalf("code %v reason %q (%v), want PermissionDenied %q", status.Code(err), keyReason(err), err, c.reason)
				}
				return
			}
			if err != nil || !slices.Equal(resp.GetAllowedDomains(), c.hosts) {
				t.Fatalf("acquire allowed_domains = %v (%v), want %v", resp.GetAllowedDomains(), err, c.hosts)
			}
		})
	}
}
