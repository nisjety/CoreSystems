package api

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// R-13 PR 2: the audit-core credential block that docker-compose.yml really gives
// this service, read as written. The Control compose file sits outside the module
// (it is the system under test), so a missing file skips the test unless
// REQUIRE_CONTRACT is set.
const auditComposePath = "../../../docker-compose.yml"

var auditComposeVariable = regexp.MustCompile(`\$\{([A-Za-z0-9_]+)(?::\?[^}]*)?\}`)

func composeAuditRegistry(t *testing.T) (string, []serviceCredential, map[string]string) {
	t.Helper()
	data, err := os.ReadFile(auditComposePath)
	if err != nil {
		if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
			t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", auditComposePath)
		}
		t.Fatalf("read %s: %v", auditComposePath, err)
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) == "AUDIT_CORE_SERVICE_CREDENTIALS: >-" {
			if found >= 0 {
				t.Fatalf("AUDIT_CORE_SERVICE_CREDENTIALS appears more than once in %s", auditComposePath)
			}
			found = index
		}
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("AUDIT_CORE_SERVICE_CREDENTIALS block not found in %s", auditComposePath)
	}
	var declared []serviceCredential
	if err := json.Unmarshal([]byte(strings.TrimSpace(lines[found+1])), &declared); err != nil {
		t.Fatalf("decode compose AUDIT_CORE_SERVICE_CREDENTIALS: %v", err)
	}
	variables := make(map[string]string, len(declared))
	resolved := make([]serviceCredential, 0, len(declared))
	for _, credential := range declared {
		match := auditComposeVariable.FindStringSubmatch(credential.Token)
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

func TestComposeAuditRegistryPassesStartupValidation(t *testing.T) {
	raw, _, _ := composeAuditRegistry(t)
	if err := ValidateRequiredServiceCredentialRegistry(raw); err != nil {
		t.Fatalf("audit-core would refuse to start with the compose credential block: %v", err)
	}
}

// TestComposeQuarryControlIsAnIngestionOnlyAuditWriter pins the R-13 PR 2 row:
// quarry-control writes audit events (crawl.robots_ignored, WS14.S7b) as a producer
// of the ingestion plane and can do nothing else in audit-core. The token comes
// from QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN, the secrets-ledger name that
// scripts/run-control-plane.sh mints once and Ingestion's store mirrors.
func TestComposeQuarryControlIsAnIngestionOnlyAuditWriter(t *testing.T) {
	_, credentials, variables := composeAuditRegistry(t)
	var quarry *serviceCredential
	for index := range credentials {
		if credentials[index].Principal == "quarry-control" {
			quarry = &credentials[index]
		}
	}
	if quarry == nil {
		t.Fatal("quarry-control principal missing from the compose audit-core registry (R-13 PR 2)")
	}
	if strings.Join(quarry.Scopes, ",") != "audit:write" {
		t.Fatalf("quarry-control scopes=%v want exactly [audit:write]", quarry.Scopes)
	}
	if strings.Join(quarry.Planes, ",") != "ingestion" {
		t.Fatalf("quarry-control planes=%v want exactly [ingestion]", quarry.Planes)
	}
	if variables["quarry-control"] != "QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN" {
		t.Fatalf("quarry-control token comes from %q, want QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN", variables["quarry-control"])
	}
}

// Audit writers are an explicit, short list. Adding one is a reviewed change, and
// every writer is pinned to the planes it may speak for.
func TestComposeAuditWritersAreExactlyTheReviewedPair(t *testing.T) {
	_, credentials, _ := composeAuditRegistry(t)
	var writers []string
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if scope == "audit:write" {
				writers = append(writers, credential.Principal+"@"+strings.Join(credential.Planes, "+"))
			}
		}
	}
	sort.Strings(writers)
	if strings.Join(writers, ",") != "integration-corev2@ingestion,quarry-control@ingestion" {
		t.Fatalf("audit:write holders=%v want exactly [integration-corev2@ingestion quarry-control@ingestion]", writers)
	}
}
