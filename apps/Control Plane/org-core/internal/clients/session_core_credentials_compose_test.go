package clients

import (
	"os"
	"strings"
	"testing"
)

// sessionCoreComposePath is apps/Control Plane/docker-compose.yml, seen from this package.
const sessionCoreComposePath = "../../../docker-compose.yml"

// sessionCoreCommittedRegistry is the SESSION_CORE_SERVICE_CREDENTIALS value as compose writes it at HEAD:
// the verevon-gateway entry alone. P2-cp-07 appended an org-core entry here, and the verifier rejected it
// (P2-cp-07-rel1). session-core's static list is not where the thread receipt is read: the receipt RPC is
// served by the Model Plane session-core, which never reads that list and needs session:retention:read on a
// verified Auth Core token. The entry did nothing for the feature and gave org-core a session read it does
// not need, so compose must keep this line byte for byte.
const sessionCoreCommittedRegistry = `[{"principal":"verevon-gateway","audience":"session-core","token":"${SESSION_CORE_SERVICE_TOKEN:?SESSION_CORE_SERVICE_TOKEN is required}","scopes":["sessions:read","sessions:write"]}]`

// sessionCoreRegistryLine returns the value line of SESSION_CORE_SERVICE_CREDENTIALS in compose, trimmed.
func sessionCoreRegistryLine(t *testing.T) string {
	t.Helper()
	raw, err := os.ReadFile(sessionCoreComposePath)
	if err != nil {
		t.Fatalf("read %s: %v", sessionCoreComposePath, err)
	}
	lines := strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) != "SESSION_CORE_SERVICE_CREDENTIALS: >-" {
			continue
		}
		if found >= 0 {
			t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS appears more than once in %s", sessionCoreComposePath)
		}
		found = index
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS block not found in %s", sessionCoreComposePath)
	}
	return strings.TrimSpace(lines[found+1])
}

// TestSessionCoreRegistryIsTheCommittedLine pins the restored block byte for byte. The static list that
// session-core loads at boot names verevon-gateway and nothing else, so org-core gets no session read from it.
// Its thread receipt is read over SessionRetentionService with a session-core token minted for org-core from
// its service API key, which is a separate path.
func TestSessionCoreRegistryIsTheCommittedLine(t *testing.T) {
	if line := sessionCoreRegistryLine(t); line != sessionCoreCommittedRegistry {
		t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS differs from the committed block (P2-cp-07-rel1):\n got %s\nwant %s", line, sessionCoreCommittedRegistry)
	}
}

// TestSessionCoreRegistryGivesOrgCoreNoSessionRead names the rule the byte pin enforces. An org-core credential
// in the static list would be a session read org-core does not need, and the receipt read does not use this list.
func TestSessionCoreRegistryGivesOrgCoreNoSessionRead(t *testing.T) {
	line := sessionCoreRegistryLine(t)
	if strings.Contains(line, `"principal":"org-core"`) {
		t.Fatal("session-core's static list must not name org-core: the thread receipt is served by the Model Plane session-core")
	}
}
