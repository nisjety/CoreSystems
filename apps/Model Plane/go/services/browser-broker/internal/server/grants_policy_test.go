package server

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func userPrincipal(zdr bool) authctx.Principal {
	return authctx.Principal{
		OrganizationID: "org1", ActorID: "user1", PrincipalType: "user",
		ZeroDataRetention: zdr, RetentionPolicyPresent: true,
	}
}

func acquire(t *testing.T, srv *Server, req *AcquireGrantRequest) *AcquireGrantResponse {
	t.Helper()
	if req.OrgId == "" {
		req.OrgId = "org1"
	}
	if req.SessionKey == "" {
		req.SessionKey = "session1"
	}
	if len(req.AllowedDomains) == 0 {
		req.AllowedDomains = []string{"example.com"}
	}
	resp, err := srv.AcquireGrant(context.Background(), req)
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	return resp
}

func TestAcquireGrant_ZdrFromRequestOrToken_UsesEphemeralIdPrefix(t *testing.T) {
	store := grant.NewMemoryRepository()
	for _, tc := range []struct {
		name       string
		tokenZdr   bool
		requestZdr bool
		wantZdr    bool
	}{
		{"neither", false, false, false},
		{"request raises", false, true, true},
		{"token raises", true, false, true},
		{"both", true, true, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			srv := testServerFor(store, userPrincipal(tc.tokenZdr))
			resp := acquire(t, srv, &AcquireGrantRequest{Zdr: tc.requestZdr})
			if resp.GetZdr() != tc.wantZdr || strings.HasPrefix(resp.GetGrantId(), "z_") != tc.wantZdr {
				t.Fatalf("zdr=%v id=%q, want zdr=%v", resp.GetZdr(), resp.GetGrantId(), tc.wantZdr)
			}
			if resp.GetKind() != "session" {
				t.Fatalf("kind = %q, want session", resp.GetKind())
			}
		})
	}
}

func TestAcquireGrant_ZdrCannotBeLoweredByRequest(t *testing.T) {
	// A ZDR token with Zdr:false in the request still yields a ZDR grant.
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true))
	resp := acquire(t, srv, &AcquireGrantRequest{Zdr: false})
	if !resp.GetZdr() || !strings.HasPrefix(resp.GetGrantId(), "z_") {
		t.Fatalf("a ZDR principal must always get a ZDR grant: %#v", resp)
	}
}

func TestAcquireGrant_PerOwnerActiveCap(t *testing.T) {
	cfg := DefaultConfig()
	cfg.MaxActivePerOwner = 2
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }

	first := acquire(t, srv, &AcquireGrantRequest{})
	acquire(t, srv, &AcquireGrantRequest{})
	_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"example.com"},
	})
	if status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("third grant code = %v, want ResourceExhausted", status.Code(err))
	}
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: first.GetGrantId()}); err != nil {
		t.Fatal(err)
	}
	acquire(t, srv, &AcquireGrantRequest{})
}

func TestAcquireGrant_PolicyDeniesInternalSuffix(t *testing.T) {
	srv := newTestServer()
	for _, host := range []string{"metadata.google.internal", "db.corp.local", "svc.default.svc", "app.localhost"} {
		_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
			OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"example.com", host},
		})
		if status.Code(err) != codes.InvalidArgument {
			t.Fatalf("%s: code = %v, want InvalidArgument", host, status.Code(err))
		}
		if strings.Contains(err.Error(), host) {
			t.Fatalf("the error must not echo the host: %v", err)
		}
	}
}

func TestAcquireGrant_PolicyBoundsTheDomainCount(t *testing.T) {
	cfg := DefaultConfig()
	cfg.MaxSessionDomains = 2
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "s", AllowedDomains: []string{"a.example.com", "b.example.com", "c.example.com"},
	})
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestAcquireGrant_UsesTheConfiguredTTL(t *testing.T) {
	cfg := DefaultConfig()
	cfg.TTL = 45 * time.Second
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	resp := acquire(t, srv, &AcquireGrantRequest{})
	remaining := time.Until(resp.GetExpiresAt().AsTime())
	if remaining <= 40*time.Second || remaining > 45*time.Second {
		t.Fatalf("expires in %v, want about 45s", remaining)
	}
}

func TestValidateGrant_ReturnsKindOwnerAndZdr(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true))
	issued := acquire(t, srv, &AcquireGrantRequest{})
	got, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: issued.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	if got.GetKind() != "session" || got.GetOwnerId() != "user1" || !got.GetZdr() || !got.GetActive() {
		t.Fatalf("validate = %#v", got)
	}
}

func TestRevokeGrant_CascadesToSensitiveChildren(t *testing.T) {
	srv := newTestServer()
	parent := acquire(t, srv, &AcquireGrantRequest{})
	child, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"example.com"},
		AllowedActions: []string{"download_ref"}, AllowedFrameIds: []string{"frame-1"},
		ParentGrantId: parent.GetGrantId(),
	})
	if err != nil {
		t.Fatal(err)
	}
	if child.GetKind() != "sensitive_approval" {
		t.Fatalf("child kind = %q", child.GetKind())
	}
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: parent.GetGrantId(), Reason: "session_closed"}); err != nil {
		t.Fatal(err)
	}
	_, err = srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: child.GetGrantId()})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("child after parent revoke: code = %v, want FailedPrecondition", status.Code(err))
	}
}

// A session key with a control character (NUL included) or past the bound is
// refused up front: the durable store cannot hold it.
func TestAcquireGrant_RejectsUnprintableOrOversizedSessionKey(t *testing.T) {
	srv := NewServerWithConfig(grant.NewMemoryRepository(), DefaultConfig())
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	for name, key := range map[string]string{
		"nul": "s\x00k", "newline": "s\nk", "del": "s\x7fk", "oversized": strings.Repeat("k", maxSessionKeyBytes+1),
	} {
		_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
			OrgId: "org1", SessionKey: key, AllowedDomains: []string{"example.com"},
		})
		if status.Code(err) != codes.InvalidArgument {
			t.Errorf("%s: code = %v, want InvalidArgument", name, status.Code(err))
		}
	}
}

func TestMapErr_InvalidDomainPolicyIsInvalidArgument(t *testing.T) {
	for name, tc := range map[string]struct {
		in   error
		want codes.Code
	}{
		"invalid policy":  {grant.ErrInvalidDomainPolicy, codes.InvalidArgument},
		"wrapped invalid": {fmt.Errorf("x: %w", grant.ErrInvalidDomainPolicy), codes.InvalidArgument},
		"not grantable":   {grant.ErrDomainNotGrantable, codes.InvalidArgument},
		"too many grants": {grant.ErrTooManyGrants, codes.ResourceExhausted},
		"derive widens":   {grant.ErrDeriveWidensParent, codes.PermissionDenied},
		"not derivable":   {grant.ErrParentNotDerivable, codes.FailedPrecondition},
		"zdr posture":     {grant.ErrZdrPostureChanged, codes.FailedPrecondition},
		"needs explicit":  {grant.ErrDeriveNeedsExplicitDomains, codes.InvalidArgument},
		"derive conflict": {grant.ErrDeriveConflict, codes.AlreadyExists},
		"store down":      {grant.ErrStoreUnavailable, codes.Unavailable},
		"store down wrap": {fmt.Errorf("x: %w", grant.ErrStoreUnavailable), codes.Unavailable},
		"invalid text":    {grant.ErrInvalidGrantText, codes.InvalidArgument},
	} {
		if got := status.Code(mapErr(tc.in)); got != tc.want {
			t.Errorf("%s: %v, want %v", name, got, tc.want)
		}
	}
	if grantOutcome(grant.ErrTooManyGrants) != "too_many_grants" || grantOutcome(grant.ErrDomainNotGrantable) != "domain_not_grantable" {
		t.Fatal("telemetry outcomes missing")
	}
	if grantOutcome(grant.ErrStoreUnavailable) != "store_unavailable" || grantOutcome(grant.ErrInvalidGrantText) != "invalid_input" {
		t.Fatal("store telemetry outcomes missing")
	}
	// The Unavailable status never carries more than the fixed text.
	if msg := status.Convert(mapErr(fmt.Errorf("x: %w", grant.ErrStoreUnavailable))).Message(); msg != grant.ErrStoreUnavailable.Error() {
		t.Fatalf("unavailable message = %q", msg)
	}
}

func envOf(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func TestConfigFromEnv(t *testing.T) {
	cfg, err := ConfigFromEnv(envOf(nil))
	if err != nil || cfg.TTL != 15*time.Minute || cfg.MaxActivePerOwner != 20 || cfg.MaxSessionDomains != 8 {
		t.Fatalf("defaults = %#v, %v", cfg, err)
	}
	cfg, err = ConfigFromEnv(envOf(map[string]string{
		EnvGrantTTLSeconds: "5", EnvMaxSessionDomains: "999", EnvMaxActiveGrantsPerUser: "3",
	}))
	if err != nil || cfg.TTL != MinGrantTTL || cfg.MaxSessionDomains != 32 || cfg.MaxActivePerOwner != 3 {
		t.Fatalf("clamped = %#v, %v", cfg, err)
	}
	cfg, err = ConfigFromEnv(envOf(map[string]string{EnvGrantTTLSeconds: "86400"}))
	if err != nil || cfg.TTL != MaxGrantTTL {
		t.Fatalf("ttl clamp = %#v, %v", cfg, err)
	}
	for _, bad := range []string{"abc", "0", "-5", "1.5"} {
		for _, name := range []string{EnvGrantTTLSeconds, EnvMaxSessionDomains, EnvMaxActiveGrantsPerUser} {
			if _, err := ConfigFromEnv(envOf(map[string]string{name: bad})); err == nil || !strings.Contains(err.Error(), name) {
				t.Fatalf("%s=%q: error = %v", name, bad, err)
			}
		}
	}
}
