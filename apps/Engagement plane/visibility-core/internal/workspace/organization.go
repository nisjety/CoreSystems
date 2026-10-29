package workspace

import (
	"encoding/base64"
	"encoding/json"
	"io"
	"os"
	"strings"
)

// This is a binding assertion, NOT authentication. Integration Core verifies
// the JWT signature/audience/expiry independently. We additionally refuse an
// operator's valid-but-wrong-org token before requesting private mappings/data.
func boundToOrganization(path, expected string) bool {
	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, 8193))
	if err != nil || len(raw) > 8192 {
		return false
	}
	parts := strings.Split(strings.TrimSpace(string(raw)), ".")
	if len(parts) != 3 {
		return false
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return false
	}
	var claims struct {
		OrganizationID string `json:"org_id"`
	}
	return json.Unmarshal(payload, &claims) == nil && expected != "" && claims.OrganizationID == expected
}
