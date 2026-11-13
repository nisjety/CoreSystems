package flagregistry

import (
	"bufio"
	"bytes"
	"maps"
	"os"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// controlComposeLiteral matches a compose environment entry that writes its value out,
// such as PPROF_ENABLED: "false". A value with a $ is interpolated and is read by
// ScanCompose instead, so this pattern does not match it.
var controlComposeLiteral = regexp.MustCompile(`^\s+([A-Z][A-Z0-9_]*):\s*"?([^"$#]*?)"?\s*(#.*)?$`)

// controlSwitchUnregistered names the switches the Control Plane compose sets that the
// registry does not describe yet, each with the reason. A name stays here only while the
// compose sets it as a switch. Registering it, or removing it from the compose, takes it out.
var controlSwitchUnregistered = map[string]string{
	"DRAGONFLY_ENABLED":                `Set as the literal "true" in several service blocks, which the interpolated scan cannot declare. The config loaders read an unset value as off, so a registry entry needs a codeDefault and a driftReason.`,
	"GF_USERS_ALLOW_SIGN_UP":           `Grafana's own sign-up switch, set as the literal "false" in the Control Plane compose. Grafana reads it, so no repository code names it.`,
	"NATS_SHARED_ALLOW_TOKEN_FALLBACK": `Set as the literal "0" in several service blocks, which the interpolated scan cannot declare. A security-relaxing shared-bus token fallback read by four config loaders, so one registry entry would need an owner decision on its codePath.`,
	"PPROF_ENABLED":                    `Set as the literal "false" in several service blocks, which the interpolated scan cannot declare. The pprof debug servers of org-core, billing-core and user-core each read it.`,
}

// controlNotSwitches names compose values that read as switch words but are not switches,
// each with the reason.
var controlNotSwitches = map[string]string{
	"GOMAXPROCS":                 "The Go runtime thread cap, set to 1 in several service blocks. A number, not a switch.",
	"MODEL_PLANE_V2_ROLLOUT_PCT": "session-core's Model Plane v2 rollout percentage, 0 to 100. A number, not a switch.",
}

// controlEntry is one environment key the Control Plane compose sets. value is the
// interpolated default, or the literal text when the compose writes it out.
type controlEntry struct {
	name         string
	value        string
	interpolated bool
}

// switchLike reports whether the entry is a switch. An interpolated name that follows the
// naming convention is one, as the convention ratchet already treats it. Any other entry is
// one when its value reads in the canonical switch vocabulary, so a switch outside the
// convention cannot hide.
func (e controlEntry) switchLike() bool {
	if e.interpolated && IsConventionName(e.name) {
		return true
	}
	return isSwitchWord(e.value)
}

// isSwitchWord reports whether value is a non-empty switch in the canonical vocabulary that
// ParseSwitch reads. 0 and 1 count, so a numeric knob that defaults to 0 or 1 is listed in
// controlNotSwitches.
func isSwitchWord(value string) bool {
	if strings.TrimSpace(value) == "" {
		return false
	}
	_, err := ParseSwitch(value)
	return err == nil
}

// scanControlComposeEntries returns every environment key the Control Plane compose sets in
// a service. Interpolated entries come from ScanCompose, the scanner the registry already
// reads declarations with. Literal entries are read here, so a switch the compose writes out
// cannot hide from the ratchet.
func scanControlComposeEntries(t *testing.T) []controlEntry {
	t.Helper()
	data, err := os.ReadFile(repoFile(controlPlaneCompose))
	if err != nil {
		t.Fatalf("read compose file: %v", err)
	}
	decls, err := ScanCompose(bytes.NewReader(data))
	if err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	var entries []controlEntry
	for _, d := range decls {
		entries = append(entries, controlEntry{name: d.Host, value: d.Default, interpolated: true})
	}
	inServices := false
	sc := bufio.NewScanner(bytes.NewReader(data))
	for sc.Scan() {
		line := strings.TrimRight(sc.Text(), "\r")
		if composeTopLevel.MatchString(line) {
			inServices = line == "services:"
			continue
		}
		if !inServices {
			continue
		}
		if m := controlComposeLiteral.FindStringSubmatch(line); m != nil {
			entries = append(entries, controlEntry{name: m[1], value: strings.TrimSpace(m[2])})
		}
	}
	if err := sc.Err(); err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	return entries
}

// TestControlComposeSwitchesAreRegisteredOrExplained is the ratchet for the Control Plane
// compose. The convention ratchet sees only interpolated names that follow the convention.
// This one sees every switch the compose sets, interpolated or literal, and requires each to
// be registered, or listed in controlSwitchUnregistered with a reason.
func TestControlComposeSwitchesAreRegisteredOrExplained(t *testing.T) {
	reg := loadShipped(t)
	set := make(map[string]bool)
	switches := make(map[string]bool)
	for _, e := range scanControlComposeEntries(t) {
		set[e.name] = true
		if _, ok := controlNotSwitches[e.name]; ok {
			continue
		}
		if e.switchLike() {
			switches[e.name] = true
		}
	}
	for _, name := range slices.Sorted(maps.Keys(switches)) {
		if _, ok := reg.Flag(name); ok {
			continue
		}
		if controlSwitchUnregistered[name] != "" {
			continue
		}
		t.Errorf("%s is a switch set in the Control Plane compose but has no registry entry; register it, or list it in controlSwitchUnregistered with a reason", name)
	}
	for _, name := range slices.Sorted(maps.Keys(controlSwitchUnregistered)) {
		if strings.TrimSpace(controlSwitchUnregistered[name]) == "" {
			t.Errorf("%s: controlSwitchUnregistered needs a reason", name)
		}
		if _, ok := reg.Flag(name); ok {
			t.Errorf("%s is registered; remove it from controlSwitchUnregistered", name)
		}
		if !switches[name] {
			t.Errorf("%s is no longer a switch set in the Control Plane compose; remove it from controlSwitchUnregistered", name)
		}
	}
	for _, name := range slices.Sorted(maps.Keys(controlNotSwitches)) {
		if strings.TrimSpace(controlNotSwitches[name]) == "" {
			t.Errorf("%s: controlNotSwitches needs a reason", name)
		}
		if !set[name] {
			t.Errorf("%s is not set in the Control Plane compose; remove it from controlNotSwitches", name)
		}
	}
}

// TestControlComposeScanReadsLiteralAndInterpolatedSwitches pins the scan the ratchet relies
// on: a literal switch is read as literal, an interpolated switch as interpolated, and a
// literal word that is not a switch is not one.
func TestControlComposeScanReadsLiteralAndInterpolatedSwitches(t *testing.T) {
	entries := scanControlComposeEntries(t)
	cases := []struct {
		name         string
		interpolated bool
		value        string
		switchLike   bool
	}{
		{"PPROF_ENABLED", false, "false", true},
		{"SPACE_POLICY_ADMIN_WRITES", true, "false", true},
		{"AUDIT_CHAIN_PRUNE_DRYRUN", true, "0", true},
		{"GIN_MODE", false, "release", false},
	}
	for _, c := range cases {
		idx := slices.IndexFunc(entries, func(e controlEntry) bool {
			return e.name == c.name && e.interpolated == c.interpolated
		})
		if idx < 0 {
			t.Errorf("%s (interpolated %v) is not read from the Control Plane compose", c.name, c.interpolated)
			continue
		}
		e := entries[idx]
		if e.value != c.value {
			t.Errorf("%s: read value %q, want %q", c.name, e.value, c.value)
		}
		if e.switchLike() != c.switchLike {
			t.Errorf("%s: switchLike %v, want %v", c.name, e.switchLike(), c.switchLike)
		}
	}
}
