package server

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Q3-E3.1-b1-c: a fill_otp approval types into its grant_origin, so the org web
// policy on the approval's own token keeps that origin as it keeps the approval's
// hosts. The parent is minted before the org denies evil.example.com, so only the
// approval's token can refuse the origin.
func TestFillOtpOriginIsKeptByTheTokenWebPolicy(t *testing.T) {
	cases := []struct {
		name   string
		origin string
		code   codes.Code
	}{
		{"origin the org now denies is refused", "https://evil.example.com", codes.InvalidArgument},
		{"origin beside the denied host is granted", "https://docs.example.com", codes.OK},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			cfg := DefaultConfig()
			cfg.FillOtpEnabled = true
			cfg.WebPolicySnapshot = true
			srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
			srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
			parent := acquire(t, srv, &AcquireGrantRequest{})

			denied := userPrincipal(false)
			denied.WebPolicy = json.RawMessage(`{"v":1,"rev":"2","domains":{"deny":["evil.example.com"]}}`)
			srv.principal = func(context.Context) (authctx.Principal, error) { return denied, nil }
			_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
				OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"docs.example.com"},
				AllowedActions: []string{"fill_otp"}, AllowedFrameIds: []string{"frame-1"},
				ParentGrantId: parent.GetGrantId(), MailConnectionId: "mail-1", GrantOrigin: c.origin,
			})
			if status.Code(err) != c.code {
				t.Fatalf("fill_otp acquire for origin %s: code %v (%v), want %v", c.origin, status.Code(err), err, c.code)
			}
		})
	}
}
