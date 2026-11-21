package http

import "net/http"

// F-f10-18: scope of the skill-write-decision route, registered through the
// P-UC1 seam. spaces:skill-write:issue is its own scope, held only by the BFF
// principal (verevon-gateway) that calls it, and is registered as a delegated
// scope: a service credential alone never proves the end user. It is not
// spaces:issue, so a deployment can enable the memory and thread issuers
// without enabling Space skill writes. Without this rule the unmapped POST would
// fall through to users:write:self.
const spaceSkillWriteIssueScope = "spaces:skill-write:issue"

func init() {
	registerSpaceDelegatedScopePrefix(spaceSkillWriteIssueScope)
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && path == "/api/v1/internal/spaces/skill-write-decision" {
			return spaceSkillWriteIssueScope, true
		}
		return "", false
	})
}
