package flagregistry

import (
	"bytes"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// natsAllowTokenFallback is the Control Plane's security-relaxing NATS token fallback. It is
// code-only: no Control Plane compose file names it. A compose entry would be a second source
// for the value each service reads, so the registry declares it by its readers instead.
const natsAllowTokenFallback = "NATS_ALLOW_TOKEN_FALLBACK"

// natsAllowTokenFallbackReader is one Control Plane file that reads the variable, with the
// expression that gates on it. The gate is pinned, so the purpose's claim about which literal
// turns the switch on is checked against the code.
type natsAllowTokenFallbackReader struct {
	path string
	gate string
}

var natsAllowTokenFallbackReaders = []natsAllowTokenFallbackReader{
	{"apps/Control Plane/user-core/internal/nats/runtime_auth.go", `os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1"`},
	{"apps/Control Plane/user-core/internal/clients/nats_auth_client.go", `os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1"`},
	{"apps/Control Plane/billing-core/internal/nats/runtime_auth.go", `os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1"`},
	{"apps/Control Plane/org-core/internal/nats/runtime_auth.go", `os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1"`},
	// The Control Plane session-core parses the value with getEnvAsBool, which also takes true.
	{"apps/Control Plane/session-core/internal/config/config.go", `getEnvAsBool("NATS_ALLOW_TOKEN_FALLBACK", false)`},
	{"apps/Control Plane/auth-core/src/nats/nats-credentials.ts", `env.NATS_ALLOW_TOKEN_FALLBACK !== '1'`},
}

// natsAllowTokenFallbackPurposeEnd ends the registry purpose. It names the literal that turns
// the switch on and the Control Plane session-core, the one reader that takes a wider set.
const natsAllowTokenFallbackPurposeEnd = "Only the literal 1 turns it on; the Control Plane session-core also takes true."

// TestNATSAllowTokenFallbackIsRegisteredAsCodeOnly pins the registry entry: a dark, default-off,
// control-plane code switch with no compose declarations, whose codePath is one of its readers.
func TestNATSAllowTokenFallbackIsRegisteredAsCodeOnly(t *testing.T) {
	f, ok := loadShipped(t).Flag(natsAllowTokenFallback)
	if !ok {
		t.Fatalf("%s is read by Control Plane services but has no registry entry", natsAllowTokenFallback)
	}
	if f.Kind != KindSwitch || f.Owner != PlaneControl || f.EffectiveStage() != StageDark {
		t.Errorf("%s: kind %q owner %q stage %q, want switch/control-plane/dark", natsAllowTokenFallback, f.Kind, f.Owner, f.EffectiveStage())
	}
	if f.Default != "0" {
		t.Errorf("%s: default %q, want %q", natsAllowTokenFallback, f.Default, "0")
	}
	if f.Source != SourceCode {
		t.Errorf("%s: source %q, want %q", natsAllowTokenFallback, f.Source, SourceCode)
	}
	if len(f.Declarations) != 0 {
		t.Errorf("%s: a code switch declares no compose entry, got %+v", natsAllowTokenFallback, f.Declarations)
	}
	if f.OrgOverridable || f.KillSwitch {
		t.Errorf("%s: must be neither orgOverridable nor a kill switch", natsAllowTokenFallback)
	}
	if !strings.Contains(f.Purpose, "Security-relaxing") {
		t.Errorf("%s: purpose must say the switch relaxes security: %q", natsAllowTokenFallback, f.Purpose)
	}
	if !strings.HasSuffix(f.Purpose, natsAllowTokenFallbackPurposeEnd) {
		t.Errorf("%s: purpose must end with %q, got %q", natsAllowTokenFallback, natsAllowTokenFallbackPurposeEnd, f.Purpose)
	}
	readers := make([]string, 0, len(natsAllowTokenFallbackReaders))
	for _, r := range natsAllowTokenFallbackReaders {
		readers = append(readers, r.path)
	}
	if !slices.Contains(readers, f.CodePath) {
		t.Errorf("%s: codePath %q is not one of its readers %v", natsAllowTokenFallback, f.CodePath, readers)
	}
}

// TestNATSAllowTokenFallbackReadersStillGate reads each reader and checks its gate is still in
// the file. A reader that changes its literal must change the purpose in the same edit.
func TestNATSAllowTokenFallbackReadersStillGate(t *testing.T) {
	for _, r := range natsAllowTokenFallbackReaders {
		data, err := os.ReadFile(repoFile(r.path))
		if err != nil {
			t.Errorf("%s: read reader: %v", r.path, err)
			continue
		}
		if !bytes.Contains(data, []byte(r.gate)) {
			t.Errorf("%s: no longer gates on %s", r.path, r.gate)
		}
	}
}

// TestNATSAllowTokenFallbackIsNotSetByComposeFiles fails if a Control Plane compose file names
// the variable on a line that is not a comment, since that would be a second source for it.
func TestNATSAllowTokenFallbackIsNotSetByComposeFiles(t *testing.T) {
	composes, err := filepath.Glob(repoFile("apps/Control Plane/docker-compose*.yml"))
	if err != nil || len(composes) == 0 {
		t.Fatalf("find the Control Plane compose files: %v (found %d)", err, len(composes))
	}
	named := regexp.MustCompile(`(?m)^[^#\n]*\b` + natsAllowTokenFallback + `\b`)
	for _, path := range composes {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read compose file: %v", err)
		}
		if named.Match(data) {
			t.Errorf("%s is named in %s; a compose entry would be a second source for the value the readers use", natsAllowTokenFallback, filepath.Base(path))
		}
	}
}
