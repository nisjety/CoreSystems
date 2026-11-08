package http

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// R-13 PR 2: the billing-core credential block that docker-compose.yml really
// gives this service, read as written. The Control compose file sits outside the
// module (it is the system under test), so a missing file skips the test unless
// REQUIRE_CONTRACT is set, like org-core's retention_posture_contract_test.go.
const billingComposePath = "../../../docker-compose.yml"

var billingComposeVariable = regexp.MustCompile(`\$\{([A-Za-z0-9_]+)(?::\?[^}]*)?\}`)

// composeBillingRegistry returns the registry as billing-core would receive it
// (a distinct, startup-valid token substituted for every ${VARIABLE}), the parsed
// credentials, and the variable name each principal's token comes from.
func composeBillingRegistry(t *testing.T) (string, []serviceCredential, map[string]string) {
	t.Helper()
	data, err := os.ReadFile(billingComposePath)
	if err != nil {
		if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
			t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", billingComposePath)
		}
		t.Fatalf("read %s: %v", billingComposePath, err)
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) == serviceCredentialEnv+": >-" {
			if found >= 0 {
				t.Fatalf("%s appears more than once in %s", serviceCredentialEnv, billingComposePath)
			}
			found = index
		}
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("%s block not found in %s", serviceCredentialEnv, billingComposePath)
	}
	var declared []serviceCredential
	if err := json.Unmarshal([]byte(strings.TrimSpace(lines[found+1])), &declared); err != nil {
		t.Fatalf("decode compose %s: %v", serviceCredentialEnv, err)
	}
	variables := make(map[string]string, len(declared))
	resolved := make([]serviceCredential, 0, len(declared))
	for _, credential := range declared {
		match := billingComposeVariable.FindStringSubmatch(credential.Token)
		if match == nil || match[0] != credential.Token {
			t.Fatalf("principal %s token is not a single ${VARIABLE} reference: %q", credential.Principal, credential.Token)
		}
		variables[credential.Principal] = match[1]
		credential.Token = fmt.Sprintf("compose-contract-%s-0123456789abcdef", strings.ToLower(match[1]))
		resolved = append(resolved, credential)
	}
	raw, err := json.Marshal(resolved)
	if err != nil {
		t.Fatalf("marshal registry: %v", err)
	}
	return string(raw), resolved, variables
}

func TestComposeBillingRegistryPassesStartupValidation(t *testing.T) {
	raw, _, _ := composeBillingRegistry(t)
	if err := ValidateRequiredServiceCredentialRegistry(raw); err != nil {
		t.Fatalf("billing-core would refuse to start with the compose credential block: %v", err)
	}
}

// TestComposeQuarryEdgeBillingPrincipalIsReadOnly pins what Quarry's limits
// resolver may do in billing-core: read quotas and entitlements, never write
// usage, never create invoices or checkouts, never touch an allowance policy. The
// token comes from QUARRY_BILLING_CORE_SERVICE_TOKEN, the secrets-ledger name that
// scripts/run-control-plane.sh mints once and Ingestion's store mirrors.
func TestComposeQuarryEdgeBillingPrincipalIsReadOnly(t *testing.T) {
	_, credentials, variables := composeBillingRegistry(t)
	var quarry *serviceCredential
	for index := range credentials {
		if credentials[index].Principal == "quarry-edge" {
			quarry = &credentials[index]
		}
	}
	if quarry == nil {
		t.Fatal("quarry-edge principal missing from the compose billing-core registry (R-13 PR 2)")
	}
	got := append([]string(nil), quarry.Scopes...)
	sort.Strings(got)
	want := []string{"billing:entitlement:read:any", "billing:quota:read:any"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("quarry-edge billing-core scopes=%v want exactly %v", got, want)
	}
	for _, scope := range got {
		if !strings.Contains(scope, ":read:") {
			t.Fatalf("quarry-edge holds a non-read scope: %s", scope)
		}
	}
	if variables["quarry-edge"] != "QUARRY_BILLING_CORE_SERVICE_TOKEN" {
		t.Fatalf("quarry-edge token comes from %q, want QUARRY_BILLING_CORE_SERVICE_TOKEN", variables["quarry-edge"])
	}
	if quarry.Audience != serviceCredentialAudience {
		t.Fatalf("quarry-edge audience=%q", quarry.Audience)
	}
}

// Nobody else gained a Quarry-shaped grant: the usage writers are still exactly
// cost-core and integration-corev2, and no principal other than quarry-edge was
// added by R-13 PR 2.
func TestComposeBillingUsageWritersAreUnchangedByTheQuarryCredential(t *testing.T) {
	_, credentials, _ := composeBillingRegistry(t)
	var writers []string
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if scope == "billing:usage:write:any" {
				writers = append(writers, credential.Principal)
			}
		}
	}
	sort.Strings(writers)
	if strings.Join(writers, ",") != "cost-core,integration-corev2" {
		t.Fatalf("billing:usage:write:any holders=%v want exactly [cost-core integration-corev2]", writers)
	}
}
