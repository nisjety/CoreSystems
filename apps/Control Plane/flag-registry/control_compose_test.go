package flagregistry

import (
	"os"
	"slices"
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
