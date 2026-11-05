package server

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

type assistantTaskSites func(context.Context, string, string, []string) (sitepolicy.Resolution, error)

func (resolve assistantTaskSites) Resolve(ctx context.Context, org, user string, hosts []string) (sitepolicy.Resolution, error) {
	return resolve(ctx, org, user, hosts)
}

func TestAssistantTaskHonorsSiteCeilingAndKeepsExactScope(t *testing.T) {
	const host = "example.com"
	for _, tc := range []struct {
		name, effective, cap, source string
		code                         codes.Code
	}{
		{"new public site", "ask", "act", "default", codes.OK},
		{"existing act preference", "act", "act", "user_pref", codes.OK},
		{"read ceiling", "ask", "read", "default", codes.PermissionDenied},
		{"unknown ceiling", "ask", "", "default", codes.PermissionDenied},
		{"unknown source", "ask", "act", "", codes.PermissionDenied},
		{"read preference", "read", "act", "user_pref", codes.PermissionDenied},
		{"member denied", "deny", "act", "user_pref", codes.PermissionDenied},
		{"org denied", "deny", "none", "org_block", codes.PermissionDenied},
	} {
		t.Run(tc.name, func(t *testing.T) {
			resolver := assistantTaskSites(func(_ context.Context, org, user string, hosts []string) (sitepolicy.Resolution, error) {
				if org != "org1" || user != "user1" || len(hosts) != 1 || hosts[0] != host {
					t.Fatalf("unexpected policy query: %q %q %v", org, user, hosts)
				}
				return sitepolicy.Resolution{Decisions: []sitepolicy.Decision{{
					Host: host, Effective: tc.effective, Cap: tc.cap, Source: tc.source,
				}}}, nil
			})
			// Even when ordinary grants do not enforce site policy, this kind
			// always consults org-core.
			srv := newTestServer().WithOptions(Options{AssistantTaskGrantsEnabled: true, SiteResolver: resolver})
			parent, err := issue(srv, &AcquireGrantRequest{GrantKind: grantKindAssistantTask, AllowedDomains: []string{host}})
			if status.Code(err) != tc.code {
				t.Fatalf("grant code = %v, want %v (%v)", status.Code(err), tc.code, err)
			}
			if tc.code != codes.OK {
				return
			}
			if parent.GetKind() != "session" || len(parent.GetAllowedDomains()) != 1 || parent.GetAllowedDomains()[0] != host {
				t.Fatalf("unexpected session policy: %+v", parent)
			}
			if time.Until(parent.GetExpiresAt().AsTime()) > assistantTaskGrantTTL || !parent.GetExpiresAt().AsTime().Equal(parent.GetAbsoluteExpiresAt().AsTime()) {
				t.Fatalf("expected a short, fixed expiry: %+v", parent)
			}
			for _, runHost := range []string{host, "docs." + host} {
				child, err := srv.DeriveRunGrant(context.Background(), &DeriveRunGrantRequest{
					ParentGrantId: parent.GetGrantId(), SessionKey: "task", OrgId: "org1",
					Purpose: derivePurposeAIRun, AllowedDomains: []string{runHost},
				})
				if runHost == host {
					if err != nil || child.GetExpiresAt().AsTime().After(parent.GetExpiresAt().AsTime()) {
						t.Fatalf("same-host run should fit within the parent: %v %v", child, err)
					}
				} else if err == nil {
					t.Fatal("task grant widened to a subdomain")
				}
			}
		})
	}
}

func TestAssistantTaskRequiresOptInUserAndLivePolicy(t *testing.T) {
	req := &AcquireGrantRequest{GrantKind: grantKindAssistantTask, AllowedDomains: []string{"example.com"}}
	if _, err := issue(newTestServer(), req); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("disabled feature code = %v", status.Code(err))
	}
	service := testServerFor(grant.NewMemoryRepository(), authctx.Principal{
		OrganizationID: "org1", ActorID: "service:test", PrincipalType: "service",
	}).WithOptions(Options{AssistantTaskGrantsEnabled: true})
	if _, err := issue(service, req); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("non-user code = %v", status.Code(err))
	}
	enabled := newTestServer().WithOptions(Options{AssistantTaskGrantsEnabled: true})
	if _, err := issue(enabled, req); status.Code(err) != codes.Unavailable {
		t.Fatalf("missing policy code = %v", status.Code(err))
	}
	req.AllowedActions = []string{"respond_dialog"}
	if _, err := issue(enabled, req); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("sensitive task scope code = %v", status.Code(err))
	}
}
