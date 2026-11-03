package authz

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"testing"
)

// EP-9 E2: the verifier accepts Control's v2 (egress tier) decision beside v1,
// returns the granted tier, and refuses unknown tiers and every grant the v2
// rules forbid.

func egressTestVerifier(t *testing.T) (*SpaceCapabilityVerifier, ed25519.PrivateKey) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	return verifier, private
}

func v2Decision(permissions ...string) func(*spaceCapabilityDecision) {
	return func(d *spaceCapabilityDecision) {
		d.ActionSchemaHash = spaceCapabilitySchemaV2
		d.Permissions = append([]string{"space:sandbox:use"}, permissions...)
	}
}

func TestVerifyAcceptsV2TierGrantsAndReturnsTheTier(t *testing.T) {
	cases := []struct {
		name       string
		claim      string
		allowlist  []string
		permission []string
		want       string
	}{
		{"legacy claim, nothing granted", "disabled_by_default", nil, nil, EgressTierOff},
		{"off claim, nothing granted", EgressTierOff, nil, nil, EgressTierOff},
		{"registries granted", EgressTierRegistries, nil, []string{"space:egress:registries"}, EgressTierRegistries},
		{"allowlist claim granted registries", EgressTierAllowlist, nil, []string{"space:egress:registries"}, EgressTierRegistries},
		{"allowlist granted with its list", EgressTierAllowlist, []string{"pypi.org"}, []string{"space:egress:allowlist"}, EgressTierAllowlist},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			verifier, private := egressTestVerifier(t)
			claims := validCapabilityClaims()
			claims.Egress = tc.claim
			claims.EgressAllowlist = tc.allowlist
			token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, v2Decision(tc.permission...))
			got, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest())
			if err != nil {
				t.Fatalf("valid v2 decision refused: %v", err)
			}
			if got.EgressTier != tc.want {
				t.Fatalf("EgressTier = %q, want %q", got.EgressTier, tc.want)
			}
		})
	}
}

func TestVerifyRefusesV2EgressTheRulesForbid(t *testing.T) {
	cases := []struct {
		name      string
		claim     string
		allowlist []string
		mutate    func(*spaceCapabilityDecision)
	}{
		{"unknown tier permission", EgressTierRegistries, nil, v2Decision("space:egress:everything")},
		{"off is not a grantable tier", EgressTierRegistries, nil, v2Decision("space:egress:off")},
		{"two tier permissions", EgressTierAllowlist, []string{"pypi.org"}, v2Decision("space:egress:registries", "space:egress:allowlist")},
		{"unknown claim", "outbound_anything", nil, v2Decision()},
		{"bare permission in v2", EgressTierRegistries, nil, v2Decision("space:egress")},
		{"grant above the claim", EgressTierRegistries, nil, v2Decision("space:egress:allowlist")},
		{"egress-capable claim with no grant", EgressTierRegistries, nil, v2Decision()},
		{"allowlist grant without a list", EgressTierAllowlist, nil, v2Decision("space:egress:allowlist")},
		{"a list without an allowlist grant", EgressTierRegistries, []string{"pypi.org"}, v2Decision("space:egress:registries")},
		{"egress under ZDR", EgressTierRegistries, nil, func(d *spaceCapabilityDecision) {
			v2Decision("space:egress:registries")(d)
			d.ZeroDataRetention = true
		}},
		{"unknown schema", EgressTierOff, nil, func(d *spaceCapabilityDecision) {
			d.ActionSchemaHash = "sha256:space-capability-profile-v3"
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			verifier, private := egressTestVerifier(t)
			claims := validCapabilityClaims()
			claims.Egress = tc.claim
			claims.EgressAllowlist = tc.allowlist
			token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, tc.mutate)
			if _, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest()); err == nil {
				t.Fatal("expected rejection")
			}
		})
	}
}

func TestVerifyRefusesAnAllowListAlteredAfterSigning(t *testing.T) {
	verifier, private := egressTestVerifier(t)
	claims := validCapabilityClaims()
	claims.Egress = EgressTierAllowlist
	claims.EgressAllowlist = []string{"pypi.org"}
	token, _ := signedCapabilityToken(t, private, "key-1", claims, v2Decision("space:egress:allowlist"))
	claims.EgressAllowlist = []string{"pypi.org", "attacker.example"}
	_, tamperedJSON := signedCapabilityToken(t, private, "key-1", claims, nil)
	if _, err := verifier.Verify(token, tamperedJSON, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("an allow-list changed in transit was accepted")
	}
}

func TestVerifyKeepsTheV1EgressRuleAndRefusesV2FieldsInV1(t *testing.T) {
	verifier, private := egressTestVerifier(t)
	claims := validCapabilityClaims()
	token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, nil)
	got, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest())
	if err != nil {
		t.Fatalf("v1 decision refused: %v", err)
	}
	if got.EgressTier != EgressTierOff {
		t.Fatalf("v1 EgressTier = %q, want off", got.EgressTier)
	}

	tierInV1, tierClaims := signedCapabilityToken(t, private, "key-1", claims, func(d *spaceCapabilityDecision) {
		d.Permissions = []string{"space:sandbox:use", "space:egress:registries"}
	})
	if _, err := verifier.Verify(tierInV1, tierClaims, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("a v1 decision carrying a tier permission was accepted")
	}

	listClaims := validCapabilityClaims()
	listClaims.EgressAllowlist = []string{"pypi.org"}
	listInV1, listJSON := signedCapabilityToken(t, private, "key-1", listClaims, nil)
	if _, err := verifier.Verify(listInV1, listJSON, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("a v1 decision carrying an allow-list v1 does not bind was accepted")
	}
}
