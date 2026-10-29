package planeclient

import (
	"path/filepath"
	"regexp"
)

var ingestionCoreID = regexp.MustCompile(`^[a-z][a-z0-9-]{1,63}$`)
var ingestionOrganizationID = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)

// IngestionBearer reopens a protected deployment file for each outbound call.
// This is a local audience/identity binding assertion, not signature verification:
// Integration Core must independently verify and authorize the service token.
// The returned bearer must never be included in an HTTP response or log.
func IngestionBearer(core, organizationID, path string) (string, error) {
	if !ingestionCoreID.MatchString(core) || !ingestionOrganizationID.MatchString(organizationID) || !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return "", unavailable("ingestion", "missing_credential")
	}
	client := Client{cfg: Config{Core: core, OrganizationID: organizationID}}
	return client.bearer(path, "ingestion", "integration:read")
}
