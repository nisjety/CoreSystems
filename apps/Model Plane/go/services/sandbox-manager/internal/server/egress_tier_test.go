package server

import (
	"context"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/protobuf/types/known/durationpb"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
)

// acquireSpaceLease runs one Space-scoped AcquireLease against a verifier that
// grants egressTier, and returns the response and the expectation the verifier
// was given.
func acquireSpaceLease(t *testing.T, s *Server, egressTier string) (*AcquireLeaseResponse, authz.CapabilityExpectation) {
	t.Helper()
	var got authz.CapabilityExpectation
	s.capabilityVerify = func(_ string, _ string, expect authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		got = expect
		return authz.VerifiedCapability{
			Claims:     authz.SpaceCapabilityClaims{BackendID: "backend-1", Egress: "registries"},
			EgressTier: egressTier,
		}, nil
	}
	s.backendID = "backend-1"
	resp, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "signed-token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	return resp, got
}

// Off, the response is unchanged and the verifier keeps its refusal rule.
func TestEgressTiersOffLeavesTheResponseUnsetAndTheRefusalRuleInPlace(t *testing.T) {
	s := newTestServer()
	resp, expect := acquireSpaceLease(t, s, authz.EgressTierRegistries)
	if resp.GetEgressTier() != mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_UNSPECIFIED {
		t.Fatalf("EgressTier = %v, want UNSPECIFIED while the switch is off", resp.GetEgressTier())
	}
	if expect.GrantEgressAtOff {
		t.Fatal("the verifier must keep refusing an ungranted egress claim while the switch is off")
	}
	l, err := s.leases.GetAny(context.Background(), resp.GetLeaseId(), "org-1", "user-1", "backend-1")
	if err != nil {
		t.Fatalf("GetAny: %v", err)
	}
	if l.EgressTierPermitted != authz.EgressTierOff {
		t.Fatalf("EgressTierPermitted = %q, want off while the switch is off", l.EgressTierPermitted)
	}
}

// On, the granted tier is recorded on the lease and reported back.
func TestEgressTiersOnRecordsAndReportsTheGrantedTier(t *testing.T) {
	for _, tc := range []struct {
		granted string
		want    mpv1.SandboxEgressTier
	}{
		{authz.EgressTierOff, mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_OFF},
		{authz.EgressTierRegistries, mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_REGISTRIES},
		{authz.EgressTierAllowlist, mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_ALLOWLIST},
	} {
		s := newTestServer().WithEgressTiers(true)
		resp, expect := acquireSpaceLease(t, s, tc.granted)
		if !expect.GrantEgressAtOff {
			t.Fatal("D-E2: with the switch on the verifier must grant an ungranted claim at off")
		}
		if resp.GetEgressTier() != tc.want {
			t.Fatalf("granted %q: EgressTier = %v, want %v", tc.granted, resp.GetEgressTier(), tc.want)
		}
		l, err := s.leases.GetAny(context.Background(), resp.GetLeaseId(), "org-1", "user-1", "backend-1")
		if err != nil {
			t.Fatalf("GetAny: %v", err)
		}
		if l.EgressTierPermitted != tc.granted {
			t.Fatalf("EgressTierPermitted = %q, want %q", l.EgressTierPermitted, tc.granted)
		}
	}
}

// 'all' is not storable in Wave 3, and an unknown word is never a wider reach.
func TestPermittedEgressTierNeverRecordsAllOrAnUnknownWord(t *testing.T) {
	for _, granted := range []string{authz.EgressTierAll, "everything", "", "Registries"} {
		if got := permittedEgressTier(true, granted); got != authz.EgressTierOff {
			t.Fatalf("permittedEgressTier(true, %q) = %q, want off", granted, got)
		}
	}
	if got := permittedEgressTier(false, authz.EgressTierAllowlist); got != authz.EgressTierOff {
		t.Fatalf("switch off must record off, got %q", got)
	}
	if got := egressTierProto("unknown"); got != mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_OFF {
		t.Fatalf("egressTierProto(unknown) = %v, want OFF", got)
	}
}

// A non-Space lease has no decision and so no egress.
func TestANonSpaceLeaseRecordsEgressOff(t *testing.T) {
	s := newTestServer().WithEgressTiers(true)
	resp, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	if resp.GetEgressTier() != mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_OFF {
		t.Fatalf("EgressTier = %v, want OFF", resp.GetEgressTier())
	}
}
