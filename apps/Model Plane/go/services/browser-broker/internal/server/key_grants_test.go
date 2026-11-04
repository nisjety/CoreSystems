package server

import (
	"context"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// INNH-10: the broker side of API-key grants.

func keyCaller(keyID string) authctx.Principal {
	return authctx.Principal{
		OrganizationID: "org1", ActorID: "user1", PrincipalType: "api_key", KeyID: keyID,
		Scopes:           []string{"browser:agent"},
		KeyHostCeiling:   []string{"example.com", "docs.example.com", "other.example"},
		KeyBudgetCeiling: authctx.KeyBudget{MicroUSD: 1_000_000, MaxSteps: 50, MaxRuntimeS: 600},
	}
}

func keyServer(store grant.Repository, principal authctx.Principal, enabled bool) *Server {
	cfg := DefaultConfig()
	cfg.KeyGrantsEnabled = enabled
	srv := NewServerWithConfig(store, cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

func keyAcquireReq() *AcquireGrantRequest {
	return &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "run-1", Mode: "cloud", AllowedDomains: []string{"example.com"},
		BudgetMicroUsd: 500_000, BudgetMaxSteps: 20, BudgetMaxRuntimeS: 300, BudgetSource: "request",
	}
}

func TestKeyGrants_DefaultConfigIsOff(t *testing.T) {
	if DefaultConfig().KeyGrantsEnabled {
		t.Fatal("key grants must default off")
	}
	cfg, err := ConfigFromEnv(envOf(map[string]string{}))
	if err != nil || cfg.KeyGrantsEnabled {
		t.Fatalf("unset switch: enabled=%v err=%v", cfg.KeyGrantsEnabled, err)
	}
	for raw, want := range map[string]bool{
		"1": true, "true": true, "on": true, "TRUE": true, " on ": true,
		"0": false, "false": false, "off": false, "yes": false, "2": false, "tru": false, "": false,
	} {
		cfg, err := ConfigFromEnv(envOf(map[string]string{EnvKeyGrantsEnabled: raw}))
		if err != nil || cfg.KeyGrantsEnabled != want {
			t.Fatalf("%q: enabled=%v err=%v, want %v", raw, cfg.KeyGrantsEnabled, err, want)
		}
	}
}

// Switch off: every api_key call is PermissionDenied, whatever it asks.
func TestKeyGrants_SwitchOffRefusesEveryKeyCall(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := keyServer(store, keyCaller("key-1"), false)
	ctx := context.Background()
	if _, err := srv.AcquireGrant(ctx, keyAcquireReq()); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("acquire code = %v, want PermissionDenied", status.Code(err))
	}
	if _, err := srv.RevokeGrant(ctx, &RevokeGrantRequest{GrantId: "g"}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("revoke code = %v, want PermissionDenied", status.Code(err))
	}
}

// A key never renews, derives, or asks for human_browse or an approval, on
// the handlers too (Authorize already refuses them).
func TestKeyGrants_HumanOnlyPowersAreRefusedAtTheHandler(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"})
	parent, err := user.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "s1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}
	srv := keyServer(store, keyCaller("key-1"), true)
	ctx := context.Background()
	if _, err := srv.RenewGrant(ctx, &RenewGrantRequest{GrantId: parent.GetGrantId(), OrgId: "org1"}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("renew code = %v", status.Code(err))
	}
	if _, err := srv.DeriveRunGrant(ctx, deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) { r.OrgId = "org1" })); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("derive code = %v", status.Code(err))
	}
	human := keyAcquireReq()
	human.GrantKind, human.AllowedDomains = "human_browse", nil
	human.BudgetMicroUsd, human.BudgetMaxSteps, human.BudgetMaxRuntimeS, human.BudgetSource = 0, 0, 0, ""
	if _, err := srv.AcquireGrant(ctx, human); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("human_browse code = %v", status.Code(err))
	}
	approval := keyAcquireReq()
	approval.AllowedActions, approval.AllowedFrameIds, approval.ParentGrantId = []string{"download_ref"}, []string{"f"}, parent.GetGrantId()
	if _, err := srv.AcquireGrant(ctx, approval); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("approval code = %v", status.Code(err))
	}
	// The owner's ordinary grant is not the key's: NotFound, never revoked.
	if _, err := srv.RevokeGrant(ctx, &RevokeGrantRequest{GrantId: parent.GetGrantId()}); status.Code(err) != codes.NotFound {
		t.Fatalf("revoke of the owner's grant code = %v, want NotFound", status.Code(err))
	}
	if _, err := user.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: parent.GetGrantId()}); err != nil {
		t.Fatalf("the owner's grant was touched: %v", err)
	}
}
