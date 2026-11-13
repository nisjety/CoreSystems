package flagregistry

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// The plaintext loopback opt-in for user-core's run action authority client is a code switch.
// user-core reads it from its env_file (./user-core/.env), and no compose file sets it. A compose
// environment entry would override that env_file value, because environment: wins over env_file:,
// so the opt-in would turn off without a word. The registry therefore declares it by its reader,
// and this test pins that no compose file sets it.
func TestSessionRunLoopbackSwitchIsRegisteredAsCodeOnly(t *testing.T) {
	const name = "CONTROL_ALLOW_INSECURE_SESSION_RUN_AUTHORITY_LOOPBACK"
	const codePath = "apps/Control Plane/user-core/cmd/server/main.go"

	f, ok := loadShipped(t).Flag(name)
	if !ok {
		t.Fatalf("%s is read by user-core but has no registry entry", name)
	}
	if f.Kind != KindSwitch || f.Owner != PlaneControl || f.EffectiveStage() != StageDark {
		t.Errorf("%s: kind %q owner %q stage %q, want switch/control-plane/dark", name, f.Kind, f.Owner, f.EffectiveStage())
	}
	if f.Default != "false" {
		t.Errorf("%s: default %q, want false", name, f.Default)
	}
	if f.Source != SourceCode || f.CodePath != codePath {
		t.Errorf("%s: source %q codePath %q, want %q read from %s", name, f.Source, f.CodePath, SourceCode, codePath)
	}
	if len(f.Declarations) != 0 {
		t.Errorf("%s: a code switch declares no compose entry, got %+v", name, f.Declarations)
	}
	if !f.DevOnly {
		t.Errorf("%s: must be devOnly, a development escape hatch", name)
	}
	if f.OrgOverridable || f.KillSwitch {
		t.Errorf("%s: must be neither orgOverridable nor a kill switch", name)
	}
	if !strings.Contains(f.Purpose, "Security-relaxing") {
		t.Errorf("%s: purpose must say the switch relaxes security: %q", name, f.Purpose)
	}
	if want := "Only the literal true turns it on."; !strings.HasSuffix(f.Purpose, want) {
		t.Errorf("%s: purpose must end with %q, got %q", name, want, f.Purpose)
	}

	// Every Control Plane compose file, the base stack and each override, must leave it unset.
	composes, err := filepath.Glob(repoFile("apps/Control Plane/docker-compose*.yml"))
	if err != nil || len(composes) == 0 {
		t.Fatalf("find the Control Plane compose files: %v (found %d)", err, len(composes))
	}
	setInCompose := regexp.MustCompile(`(?m)^\s*` + regexp.QuoteMeta(name) + `\s*:`)
	for _, path := range composes {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read compose file: %v", err)
		}
		if setInCompose.Match(data) {
			t.Errorf("%s is set in %s; environment: would override the env_file value user-core reads", name, filepath.Base(path))
		}
	}
}
