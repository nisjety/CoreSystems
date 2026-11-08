package flagregistry

import (
	"bufio"
	"io"
	"regexp"
	"strings"
)

// conventionPattern is the naming convention from the flags-and-config ledger:
// <SCOPE>_ENABLED, *_MODE, *_CONFIRMED and *_ALLOW_*. *_SAMPLE_RATE is included
// for the one numeric knob that sits in the same compose blocks.
var conventionPattern = regexp.MustCompile(`^[A-Z][A-Z0-9_]*(_ENABLED|_MODE|_CONFIRMED|_SAMPLE_RATE|_ALLOW_[A-Z0-9_]+)$`)

// IsConventionName reports whether name follows the flag naming convention.
// The compose ratchet requires every such name set in compose to be registered
// or listed as unregistered.
func IsConventionName(name string) bool {
	return conventionPattern.MatchString(name)
}

// ComposeDecl is one interpolated environment entry in a compose file. Env is
// set in Service from the host variable Host, falling back to Default.
type ComposeDecl struct {
	Service string
	Env     string
	Host    string
	Default string
}

var (
	composeTopLevel = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_-]*:`)
	composeService  = regexp.MustCompile(`^  ([a-z0-9][a-z0-9-]*):\s*$`)
	composeEnv      = regexp.MustCompile(`^\s+([A-Z][A-Z0-9_]*):\s*\$\{([A-Z][A-Z0-9_]*):-([^}]*)\}\s*(#.*)?$`)
)

// ScanCompose returns the interpolated environment entries under the top-level
// services key. YAML anchors above services and every block after it are skipped.
func ScanCompose(r io.Reader) ([]ComposeDecl, error) {
	var decls []ComposeDecl
	inServices := false
	service := ""
	sc := bufio.NewScanner(r)
	for sc.Scan() {
		line := strings.TrimRight(sc.Text(), "\r")
		if composeTopLevel.MatchString(line) {
			if line == "services:" {
				inServices = true
				continue
			}
			if inServices {
				break
			}
			continue
		}
		if !inServices {
			continue
		}
		if m := composeService.FindStringSubmatch(line); m != nil {
			service = m[1]
			continue
		}
		if m := composeEnv.FindStringSubmatch(line); m != nil {
			decls = append(decls, ComposeDecl{Service: service, Env: m[1], Host: m[2], Default: m[3]})
		}
	}
	return decls, sc.Err()
}
