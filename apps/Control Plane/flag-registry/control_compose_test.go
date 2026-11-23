package flagregistry

import (
	"bufio"
	"bytes"
	"os"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// controlComposeFlags are the thirteen convention-named variables that the Control
// Plane compose sets from interpolated host variables. Until the ratchet scanned
// that file they had neither an entry nor an unregistered line, so each one is
// named here and must keep a Control Plane entry.
var controlComposeFlags = []string{
	"API_KEY_AGENT_EXCHANGE_ENABLED",
	"AUDIT_CHAIN_ENABLED",
	"AUDIT_CHAIN_PRUNE_ENABLED",
	"AUDIT_CHAIN_VERIFY_ENABLED",
	"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS",
	"AUDIT_SIEM_EXPORT_ENABLED",
	"DISPOSABLE_EMAIL_BLOCKLIST_ENABLED",
	"GRAFANA_ANONYMOUS_ENABLED",
	"ORG_CORE_KEYS_ENABLED",
	"ORG_DOMAIN_VERIFICATION_ENABLED",
	"ORG_SCIM_ENABLED",
	"ORG_SIEM_DESTINATIONS_ENABLED",
	"WEB_POLICY_CLAIM_ENABLED",
}

// TestControlPlaneComposeFlagsStayRegistered keeps the thirteen names registered.
// Each has a Control Plane entry with compose as its source, and none falls back
// to the unregistered baseline.
func TestControlPlaneComposeFlagsStayRegistered(t *testing.T) {
	reg := loadShipped(t)
	for _, name := range controlComposeFlags {
		f, ok := reg.Flag(name)
		if !ok {
			t.Errorf("%s is set by the Control Plane compose but has no registry entry", name)
			continue
		}
		if f.Owner != PlaneControl {
			t.Errorf("%s: owner is %s, want %s", name, f.Owner, PlaneControl)
		}
		if f.Source != SourceCompose {
			t.Errorf("%s: source is %s, want %s", name, f.Source, SourceCompose)
		}
		if slices.Contains(reg.Unregistered(), name) {
			t.Errorf("%s is listed as unregistered; its entry replaces that line", name)
		}
	}
}

// TestControlPlaneComposeScanNamesEachService pins the scanner on the file the
// ratchet now reads. The Control Plane compose indents services by four spaces, so
// a two-space scanner would name no service for any of its variables.
func TestControlPlaneComposeScanNamesEachService(t *testing.T) {
	f, err := os.Open(repoFile(controlPlaneCompose))
	if err != nil {
		t.Fatalf("open compose file: %v", err)
	}
	defer f.Close()
	decls, err := ScanCompose(f)
	if err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	for _, d := range decls {
		if d.Service == "" {
			t.Errorf("%s: scanned with no service; the service header was not recognised", d.Host)
		}
	}
	want := map[string]string{
		"AUDIT_CHAIN_ENABLED":                    "audit-core",
		"AUDIT_SIEM_EXPORT_ENABLED":              "audit-siem-exporter",
		"DISPOSABLE_EMAIL_BLOCKLIST_ENABLED":     "auth-core",
		"GRAFANA_ANONYMOUS_ENABLED":              "controlplane-grafana",
		"ORG_SCIM_ENABLED":                       "org-core",
		"USER_CORE_ORG_ERASURE_CONSUMER_ENABLED": "user-core",
		"WEB_POLICY_CLAIM_ENABLED":               "auth-core",
	}
	seen := make(map[string]bool)
	for _, d := range decls {
		service, ok := want[d.Host]
		if !ok {
			continue
		}
		seen[d.Host] = true
		if d.Service != service {
			t.Errorf("%s: scanned under service %q, want %q", d.Host, d.Service, service)
		}
	}
	for host := range want {
		if !seen[host] {
			t.Errorf("%s: not scanned from the Control Plane compose", host)
		}
	}
}

// productionControlCompose is the Control Plane production overlay. Compose layers it over
// controlPlaneCompose, so a variable the overlay does not write keeps the base default. The
// registry ratchet does not read it.
const productionControlCompose = "apps/Control Plane/docker-compose.production.yml"

// spaceHistoryWindowSwitches are the two D18/D21o switches. Both default on in the dev
// compose, so the production overlay must write each one as the literal false.
var spaceHistoryWindowSwitches = []string{
	"SPACE_MEMBER_HISTORY_WINDOWS",
	"SPACE_CANVAS_HISTORY_WINDOW",
}

// productionGatewayCompose is the Verevon v3 production overlay. It pins the gateway's copy of
// SPACE_MEMBER_HISTORY_WINDOWS, which the registry's onDefaultReason names. It indents by four
// spaces where the Control Plane overlay indents by two.
const productionGatewayCompose = "apps/Frontend Plane/verevonv3/docker-compose.production.yml"

// productionServiceEnv returns the literal environment entries that a production overlay
// writes for one service, keyed by variable name with the value as written. indent is the
// overlay's indent step: a service name sits at one step and an environment entry at three.
// Interpolated entries and deeper keys do not match.
func productionServiceEnv(t *testing.T, path, service string, indent int) map[string]string {
	t.Helper()
	step := strings.Repeat(" ", indent)
	productionServiceHeader := regexp.MustCompile(`^` + step + `([a-z0-9][a-z0-9-]*):\s*$`)
	productionEnvEntry := regexp.MustCompile(`^` + step + step + step + `([A-Z][A-Z0-9_]*):\s*"?([^"$#]*?)"?\s*(#.*)?$`)
	data, err := os.ReadFile(repoFile(path))
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	env := make(map[string]string)
	inServices, inService := false, false
	sc := bufio.NewScanner(bytes.NewReader(data))
	for sc.Scan() {
		line := strings.TrimRight(sc.Text(), "\r")
		if composeTopLevel.MatchString(line) {
			inServices = line == "services:"
			inService = false
			continue
		}
		if !inServices {
			continue
		}
		if m := productionServiceHeader.FindStringSubmatch(line); m != nil {
			inService = m[1] == service
			continue
		}
		if !inService {
			continue
		}
		if m := productionEnvEntry.FindStringSubmatch(line); m != nil {
			env[m[1]] = strings.TrimSpace(m[2])
		}
	}
	if err := sc.Err(); err != nil {
		t.Fatalf("scan %s: %v", path, err)
	}
	return env
}

// TestSpaceHistoryWindowSwitchesArePinnedOffInProduction pins the D21o production rule. Each
// switch defaults on for local dev, so the production overlay must write it as the literal
// false on user-core. The registry's onDefaultReason must also name that overlay as the place
// production is turned off, so the reason cannot claim an off that no file sets.
func TestSpaceHistoryWindowSwitchesArePinnedOffInProduction(t *testing.T) {
	reg := loadShipped(t)
	env := productionServiceEnv(t, productionControlCompose, "user-core", 2)
	if len(env) == 0 {
		t.Fatalf("no user-core environment read from %s; the scanner or the path is wrong", productionControlCompose)
	}
	for _, name := range spaceHistoryWindowSwitches {
		f, ok := reg.Flag(name)
		if !ok {
			t.Errorf("%s has no registry entry", name)
			continue
		}
		if f.Default != "true" {
			t.Errorf("%s: default is %q; the local-dev default is on, and production pins it off", name, f.Default)
		}
		got, ok := env[name]
		switch {
		case !ok:
			t.Errorf("%s is not written in the user-core environment of %s; production would inherit the dev default", name, productionControlCompose)
		case got != "false":
			t.Errorf("%s: %s writes %q, want the literal \"false\"", name, productionControlCompose, got)
		}
		if !strings.Contains(f.OnDefaultReason, "docker-compose.production.yml") {
			t.Errorf("%s: onDefaultReason does not name docker-compose.production.yml, where production is turned off", name)
		}
	}
}

// TestMemberHistoryWindowsGatewayIsPinnedOffInProduction holds the other half of the
// SPACE_MEMBER_HISTORY_WINDOWS onDefaultReason: the gateway declares the switch too, so the
// Verevon v3 production overlay must write it as the literal false on the gateway service.
func TestMemberHistoryWindowsGatewayIsPinnedOffInProduction(t *testing.T) {
	const name = "SPACE_MEMBER_HISTORY_WINDOWS"
	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Fatalf("%s has no registry entry", name)
	}
	if !strings.Contains(f.OnDefaultReason, "verevonv3 docker-compose.production.yml") {
		t.Errorf("%s: onDefaultReason does not name the verevonv3 production overlay that pins the gateway", name)
	}
	env := productionServiceEnv(t, productionGatewayCompose, "gateway", 4)
	if len(env) == 0 {
		t.Fatalf("no gateway environment read from %s; the scanner or the path is wrong", productionGatewayCompose)
	}
	switch got, ok := env[name]; {
	case !ok:
		t.Errorf("%s is not written in the gateway environment of %s; production would inherit the dev default", name, productionGatewayCompose)
	case got != "false":
		t.Errorf("%s: %s writes %q for the gateway, want the literal \"false\"", name, productionGatewayCompose, got)
	}
}
