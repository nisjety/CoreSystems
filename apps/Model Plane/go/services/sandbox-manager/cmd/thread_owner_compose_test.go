package main

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
)

// The checks below pin the Compose half of the thread owner check (P2-go-06-rel1). The
// check mints its session-core token from auth-core:3011, and auth-core is reachable only
// over inter-plane-bus. A sandbox-manager that joins only the plane's own default network
// cannot mint that token, so with SANDBOX_MANAGER_THREAD_OWNER_CHECK on every thread-scoped
// call fails closed with UNAVAILABLE. Like letta-bridge's Compose test, these read the file
// as text: a service block is cut out by its two-space header, and its networks by indent.

// serviceBlockInCompose returns the lines of one top-level service in a Compose file: from
// its "  <service>:" header up to the next sibling service or top-level key.
func serviceBlockInCompose(compose, service string) (string, bool) {
	header := "  " + service + ":"
	var block []string
	found := false
	for _, line := range strings.Split(strings.ReplaceAll(compose, "\r\n", "\n"), "\n") {
		if !found {
			found = line == header
			continue
		}
		if endsServiceBlock(line) {
			break
		}
		block = append(block, line)
	}
	return strings.Join(block, "\n"), found
}

// endsServiceBlock reports whether line opens a sibling service (two spaces of indent) or a
// top-level key (no indent). Blank lines and comments never end a block.
func endsServiceBlock(line string) bool {
	body := strings.TrimLeft(line, " ")
	if body == "" || strings.HasPrefix(body, "#") {
		return false
	}
	indent := len(line) - len(body)
	return indent == 0 || indent == 2
}

// networksOfServiceBlock returns the names a service block declares under its networks:
// key. The key sits at four spaces of indent and each network name at six, which is the
// block form docker-compose.yml uses. Anything indented deeper (aliases) is not read.
func networksOfServiceBlock(block string) []string {
	var names []string
	inNetworks := false
	for _, line := range strings.Split(block, "\n") {
		body := strings.TrimLeft(line, " ")
		indent := len(line) - len(body)
		if !inNetworks {
			inNetworks = indent == 4 && body == "networks:"
			continue
		}
		if body == "" || strings.HasPrefix(body, "#") {
			continue
		}
		if indent <= 4 {
			break
		}
		if indent == 6 {
			name, _, _ := strings.Cut(body, ":")
			names = append(names, strings.TrimSpace(name))
		}
	}
	return names
}

// switchDefaultInServiceBlock returns the default a service block gives one environment
// switch: the default in ${NAME:-default}, or a literal value, with quotes and any trailing
// comment dropped. present is false when the block does not set the switch at all.
func switchDefaultInServiceBlock(block, name string) (string, bool) {
	for _, line := range strings.Split(block, "\n") {
		if !strings.HasPrefix(line, "      ") || strings.HasPrefix(line, "       ") {
			continue
		}
		key, value, ok := strings.Cut(strings.TrimSpace(line), ":")
		if !ok || key != name {
			continue
		}
		if i := strings.Index(value, " #"); i >= 0 {
			value = value[:i]
		}
		value = strings.Trim(strings.TrimSpace(value), `"'`)
		if !strings.HasPrefix(value, "${") || !strings.HasSuffix(value, "}") {
			return value, true
		}
		inner := value[len("${") : len(value)-len("}")]
		if _, def, ok := strings.Cut(inner, ":-"); ok {
			return def, true
		}
		if _, def, ok := strings.Cut(inner, "-"); ok {
			return def, true
		}
		return "", true
	}
	return "", false
}

// sandboxManagerBlockInCompose reads the Model Plane Compose file and returns the
// sandbox-manager service block.
func sandboxManagerBlockInCompose(t *testing.T) string {
	t.Helper()
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	composePath := filepath.Join(filepath.Dir(sourceFile), "..", "..", "..", "..", "deploy", "docker-compose.yml")
	raw, err := os.ReadFile(composePath)
	if err != nil {
		t.Fatalf("read Model Plane Compose file: %v", err)
	}
	block, found := serviceBlockInCompose(string(raw), "sandbox-manager")
	if !found {
		t.Fatal("docker-compose.yml has no sandbox-manager service")
	}
	return block
}

// TestSandboxManagerJoinsInterPlaneBus pins the network half of the fix: sandbox-manager
// keeps its own default network and also joins inter-plane-bus, where auth-core is.
func TestSandboxManagerJoinsInterPlaneBus(t *testing.T) {
	networks := networksOfServiceBlock(sandboxManagerBlockInCompose(t))
	for _, want := range []string{"default", "inter-plane-bus"} {
		if !slices.Contains(networks, want) {
			t.Fatalf("sandbox-manager networks = %q, must include %q: auth-core:3011 is reachable only over inter-plane-bus, so with SANDBOX_MANAGER_THREAD_OWNER_CHECK on the token mint fails and every thread-scoped call is refused as UNAVAILABLE", networks, want)
		}
	}
}

// TestSandboxManagerThreadOwnerSwitchDefaultsOff keeps the switch-off path where it is: if
// the Compose file sets SANDBOX_MANAGER_THREAD_OWNER_CHECK, its default is not the literal
// "true". Unset, the Go reader sees an empty value and the check stays off.
func TestSandboxManagerThreadOwnerSwitchDefaultsOff(t *testing.T) {
	value, present := switchDefaultInServiceBlock(sandboxManagerBlockInCompose(t), "SANDBOX_MANAGER_THREAD_OWNER_CHECK")
	if present && value == "true" {
		t.Fatal("docker-compose.yml defaults SANDBOX_MANAGER_THREAD_OWNER_CHECK to true: the thread owner check must default off")
	}
}

// TestComposeBlockReadersOnFixtures proves the readers on fixtures, so the file checks above
// cannot pass through a reader that reads nothing: the fixed shape reads as both networks, a
// block without the key reads as none, and a neighbouring service's networks do not leak in.
func TestComposeBlockReadersOnFixtures(t *testing.T) {
	fixed := `services:
  sandbox-manager:
    environment:
      SANDBOX_MANAGER_THREAD_OWNER_CHECK: ${SANDBOX_MANAGER_THREAD_OWNER_CHECK:-false}
    networks:
      default: {}
      inter-plane-bus: {}
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8086/healthz"]
  browser-broker:
    networks:
      default: {}
      inter-plane-bus:
        aliases:
          - browser-broker
`
	block, found := serviceBlockInCompose(fixed, "sandbox-manager")
	if !found {
		t.Fatal("fixture has no sandbox-manager block")
	}
	if got := networksOfServiceBlock(block); !slices.Equal(got, []string{"default", "inter-plane-bus"}) {
		t.Fatalf("fixed shape reads as networks %q", got)
	}
	if value, present := switchDefaultInServiceBlock(block, "SANDBOX_MANAGER_THREAD_OWNER_CHECK"); !present || value != "false" {
		t.Fatalf("fixed shape reads the switch default as (%q, %v)", value, present)
	}

	bare := `services:
  sandbox-manager:
    environment:
      LOG_LEVEL: info
  browser-broker:
    networks:
      inter-plane-bus: {}
`
	block, found = serviceBlockInCompose(bare, "sandbox-manager")
	if !found {
		t.Fatal("fixture has no sandbox-manager block")
	}
	if got := networksOfServiceBlock(block); len(got) != 0 {
		t.Fatalf("a block without a networks key reads as %q: a neighbour's networks leaked in", got)
	}
	if _, present := switchDefaultInServiceBlock(block, "SANDBOX_MANAGER_THREAD_OWNER_CHECK"); present {
		t.Fatal("a block without the switch reads the switch as present")
	}
}
