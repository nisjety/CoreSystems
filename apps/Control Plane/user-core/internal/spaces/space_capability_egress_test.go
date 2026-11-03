package spaces

import (
	"fmt"
	"slices"
	"strings"
	"testing"
	"time"
)

// EP-9 E2: egress tiers in the Space capability decision, built behind
// SPACE_SANDBOX_EGRESS_TIERS. These tests call the tiered issuer directly; the
// v1 issuer's behaviour is pinned by space_capability_decision_test.go.

func tieredEgressFixture() (PersonalThreadDecisionEvidence, SpaceCapabilityIntent, time.Time) {
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	return evidence, validSpaceCapabilityIntent(), time.Date(2026, time.November, 3, 10, 0, 0, 0, time.UTC)
}

func egressPermissions(decision Decision) []string {
	var out []string
	for _, permission := range decision.Permissions {
		if strings.HasPrefix(permission, "space:egress") {
			out = append(out, permission)
		}
	}
	return out
}

func TestTieredDecisionGivesTheLegacyClaimNoEgressPermission(t *testing.T) {
	evidence, intent, now := tieredEgressFixture()
	ceiling := SandboxEgressPolicy{Tier: SandboxEgressAllowlist, Allowlist: []string{"pypi.org"}}
	decision, grant, err := IssueTieredSpaceCapabilityDecision(evidence, intent, ceiling, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueTieredSpaceCapabilityDecision: %v", err)
	}
	if got := egressPermissions(decision); len(got) != 0 {
		t.Fatalf("disabled_by_default earned egress permissions %v", got)
	}
	if grant.Tier != SandboxEgressOff || len(grant.Allowlist) != 0 {
		t.Fatalf("grant = %+v, want off with no hosts", grant)
	}
	if decision.ActionSchemaHash != spaceCapabilitySchemaV2 {
		t.Fatalf("schema = %q, want v2", decision.ActionSchemaHash)
	}
}

func TestTieredDecisionSignsTheLowerOfClaimAndCeiling(t *testing.T) {
	cases := []struct {
		name    string
		claim   string
		ceiling string
		want    []string
	}{
		{"registries claim, ceiling off", SandboxEgressRegistries, SandboxEgressOff, nil},
		{"registries claim, ceiling registries", SandboxEgressRegistries, SandboxEgressRegistries, []string{"space:egress:registries"}},
		{"registries claim, ceiling allowlist", SandboxEgressRegistries, SandboxEgressAllowlist, []string{"space:egress:registries"}},
		{"allowlist claim, ceiling registries", SandboxEgressAllowlist, SandboxEgressRegistries, []string{"space:egress:registries"}},
		{"allowlist claim, ceiling allowlist", SandboxEgressAllowlist, SandboxEgressAllowlist, []string{"space:egress:allowlist"}},
		{"all claim, ceiling allowlist", SandboxEgressAll, SandboxEgressAllowlist, []string{"space:egress:allowlist"}},
		{"off claim, ceiling allowlist", SandboxEgressOff, SandboxEgressAllowlist, nil},
		{"unknown ceiling counts as off", SandboxEgressAllowlist, "everything", nil},
		{"a stored all ceiling is never signed", SandboxEgressAll, SandboxEgressAll, nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evidence, intent, now := tieredEgressFixture()
			intent.Egress = tc.claim
			ceiling := SandboxEgressPolicy{Tier: tc.ceiling, Allowlist: []string{"files.example.org"}}
			decision, _, err := IssueTieredSpaceCapabilityDecision(evidence, intent, ceiling, "decision-1", "nonce-1", now)
			if err != nil {
				t.Fatalf("IssueTieredSpaceCapabilityDecision: %v", err)
			}
			if got := egressPermissions(decision); !slices.Equal(got, tc.want) {
				t.Fatalf("egress permissions = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestTieredAllowlistGrantBindsTheListIntoTheDigest(t *testing.T) {
	evidence, intent, now := tieredEgressFixture()
	intent.Egress = SandboxEgressAllowlist
	ceiling := SandboxEgressPolicy{Tier: SandboxEgressAllowlist, Allowlist: []string{"a.example.org", "b.example.org"}}
	decision, grant, err := IssueTieredSpaceCapabilityDecision(evidence, intent, ceiling, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueTieredSpaceCapabilityDecision: %v", err)
	}
	if grant.Tier != SandboxEgressAllowlist || !slices.Equal(grant.Allowlist, ceiling.Allowlist) {
		t.Fatalf("grant = %+v, want the ceiling's list at allowlist", grant)
	}
	signed := EgressAllowlistDigest(grant.Allowlist)
	if decision.PayloadDigest != spaceCapabilityPayloadDigestFor(evidence, intent, spaceCapabilitySchemaV2, &signed) {
		t.Fatal("payload digest does not bind the granted allow-list")
	}
	other := EgressAllowlistDigest([]string{"a.example.org"})
	if decision.PayloadDigest == spaceCapabilityPayloadDigestFor(evidence, intent, spaceCapabilitySchemaV2, &other) {
		t.Fatal("a different allow-list produced the same payload digest")
	}
	if decision.PayloadDigest == spaceCapabilityPayloadDigest(evidence, intent) {
		t.Fatal("the v2 digest equals the v1 digest")
	}
	if EgressAllowlistDigest([]string{"ab", "c"}) == EgressAllowlistDigest([]string{"a", "bc"}) {
		t.Fatal("the allow-list digest is not length-prefixed")
	}
}

func TestTieredAllowlistGrantWithAnEmptyListIsOff(t *testing.T) {
	evidence, intent, now := tieredEgressFixture()
	intent.Egress = SandboxEgressAllowlist
	decision, grant, err := IssueTieredSpaceCapabilityDecision(evidence, intent, SandboxEgressPolicy{Tier: SandboxEgressAllowlist}, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueTieredSpaceCapabilityDecision: %v", err)
	}
	if got := egressPermissions(decision); len(got) != 0 || grant.Tier != SandboxEgressOff {
		t.Fatalf("empty allow-list granted %v / %+v", got, grant)
	}
}

func TestTieredDecisionNeverGrantsEgressUnderZDR(t *testing.T) {
	evidence, intent, now := tieredEgressFixture()
	evidence.Privacy.ZeroDataRetention = true
	intent.Egress = SandboxEgressAllowlist
	ceiling := SandboxEgressPolicy{Tier: SandboxEgressAllowlist, Allowlist: []string{"pypi.org"}}
	decision, grant, err := IssueTieredSpaceCapabilityDecision(evidence, intent, ceiling, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("a ZDR Space must still get a capability decision: %v", err)
	}
	if got := egressPermissions(decision); len(got) != 0 || grant.Tier != SandboxEgressOff || len(grant.Allowlist) != 0 {
		t.Fatalf("ZDR granted egress: %v / %+v", got, grant)
	}
	if !decision.ZeroDataRetention {
		t.Fatal("the decision must carry the ZDR flag")
	}
}

func TestTieredDecisionStillRefusesAnUnknownClaimAndKeepsTheAuthorityFloor(t *testing.T) {
	evidence, intent, now := tieredEgressFixture()
	intent.Egress = "outbound_anything"
	if _, _, err := IssueTieredSpaceCapabilityDecision(evidence, intent, SandboxEgressPolicy{Tier: SandboxEgressOff}, "decision-1", "nonce-1", now); err == nil {
		t.Fatal("an unknown egress claim was signed")
	}
	evidence, intent, now = tieredEgressFixture()
	evidence.SandboxCapabilityEntitled = false
	if _, _, err := IssueTieredSpaceCapabilityDecision(evidence, intent, SandboxEgressPolicy{Tier: SandboxEgressOff}, "decision-1", "nonce-1", now); err == nil {
		t.Fatal("a Space without the sandbox entitlement got a tiered decision")
	}
}

func TestTierClaimsAreRefusedWhileTheSwitchIsOff(t *testing.T) {
	_, intent, _ := tieredEgressFixture()
	intent.Egress = SandboxEgressRegistries
	if err := intent.Validate(); err == nil {
		t.Fatal("the v1 vocabulary accepted a tier claim")
	}
	if err := intent.ValidateWithEgressTiers(true); err != nil {
		t.Fatalf("the tiered vocabulary refused registries: %v", err)
	}
}

func TestSandboxEgressPolicyNormalize(t *testing.T) {
	got, err := SandboxEgressPolicy{Tier: SandboxEgressAllowlist, Allowlist: []string{"B.Example.org.", "a.example.org", "b.example.org"}}.Normalize()
	if err != nil {
		t.Fatalf("Normalize: %v", err)
	}
	if !slices.Equal(got.Allowlist, []string{"a.example.org", "b.example.org"}) {
		t.Fatalf("allow-list = %v, want canonical, sorted, de-duplicated", got.Allowlist)
	}
	if _, err := (SandboxEgressPolicy{Tier: SandboxEgressAll}).Normalize(); err == nil {
		t.Fatal("all was storable")
	}
	if _, err := (SandboxEgressPolicy{Tier: "Registries"}).Normalize(); err == nil {
		t.Fatal("an unknown tier spelling was storable")
	}
	tooMany := make([]string, maxSandboxEgressAllowlistHosts+1)
	for i := range tooMany {
		tooMany[i] = fmt.Sprintf("host%d.example.org", i)
	}
	if _, err := (SandboxEgressPolicy{Tier: SandboxEgressAllowlist, Allowlist: tooMany}).Normalize(); err == nil {
		t.Fatal("more than 32 hosts were storable")
	}
}

func TestCanonicalEgressHostRefusesNonHosts(t *testing.T) {
	for _, raw := range []string{
		"", "localhost", "10.0.0.1", "169.254.169.254", "::1", "[::1]", "127.1",
		"https://pypi.org", "pypi.org:443", "pypi.org/simple", "*.pypi.org",
		"-bad.example.org", "bad-.example.org", "a..example.org", "bücher.example",
		strings.Repeat("a", 64) + ".example.org",
	} {
		if host, err := CanonicalEgressHost(raw); err == nil {
			t.Errorf("CanonicalEgressHost(%q) = %q, want refusal", raw, host)
		}
	}
	for raw, want := range map[string]string{
		"PyPI.org":                  "pypi.org",
		" files.pythonhosted.org. ": "files.pythonhosted.org",
		"xn--bcher-kva.example":     "xn--bcher-kva.example",
	} {
		if got, err := CanonicalEgressHost(raw); err != nil || got != want {
			t.Errorf("CanonicalEgressHost(%q) = %q, %v; want %q", raw, got, err, want)
		}
	}
}
