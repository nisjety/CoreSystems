package http

import "strings"

// WS04-S3: scope of the people-directory reads, registered through the P-UC1
// seam (the rule hook runs for every internal path, not only spaces paths, and
// only after the core space table did not match). Every method on the prefix
// maps to directory:read:self, which ends in :self and so requires a verified
// gateway delegation; only GET routes exist, anything else is a 404/405 after
// authentication. Without this rule the path would fall through to
// users:read:self.
const directoryPathPrefix = "/api/v1/internal/directory/"

func init() {
	registerSpaceScopeRule(func(_, path string) (string, bool) {
		if strings.HasPrefix(path, directoryPathPrefix) {
			return directoryReadScope, true
		}
		return "", false
	})
}
