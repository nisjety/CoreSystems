package api

import (
	"strings"
	"testing"
)

// Space and agent memory (EP-8 S6) are written by Session Core only, through its
// memory gate and with the audience stamp. capability-core's /api/v1/memory proxy
// writes agent_memory by direct SQL (D2-480), so its scope vocabulary is kept
// smaller on purpose: it can neither write nor list the two new kinds.
func TestCapabilityCoreScopeEnumCannotWriteSpace(t *testing.T) {
	for _, scope := range []string{"space", "agent", "SPACE", " space"} {
		if validMemoryScope(scope) {
			t.Fatalf("capability-core must not accept memory scope %q", scope)
		}
	}
	for _, scope := range []string{"run", "thread", "workspace", "session", "user", "org", "global"} {
		if !validMemoryScope(scope) {
			t.Fatalf("existing memory scope %q must stay writable", scope)
		}
	}
	for _, access := range []memoryReadAccess{memoryReadOrgOnly, memoryReadFull} {
		filter := memoryReadVisibilitySQL(access, "$2")
		if strings.Contains(filter, "'space'") || strings.Contains(filter, "'agent'") {
			t.Fatalf("the read filter must not list Space or agent rows: %s", filter)
		}
	}
}
