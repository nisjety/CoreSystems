package users

import (
	"os"
	"strings"
	"testing"
)

// TestGDPRSubjectContract pins the audit + cross-plane fan-out subjects.
// Subscribers (audit-core; Model Plane run-history/conversations; Data Plane
// documents) depend on these exact names — a rename is a breaking change.
func TestGDPRSubjectContract(t *testing.T) {
	cases := map[string]string{
		ErasureAuditSubject:      "verevon.audit.v2.control.user-core.erasure",
		DSARExportAuditSubject:   "verevon.audit.v2.control.user-core.dsar_export",
		GDPRErasureFanoutSubject: "verevon.gdpr.erasure.requested",
	}
	for got, want := range cases {
		if got != want {
			t.Errorf("subject = %q, want %q", got, want)
		}
	}
}

// TestDSARDisclosureVerbatim pins the Art. 15 CP-only scope notice word-for-word.
// The export must always disclose it covers Control-Plane data ONLY and that
// Model/Data-Plane purge happens via the cross-plane fan-out — it must never
// imply "exported/erased everywhere". Changing this copy is a deliberate act that
// must update this test in the same change.
func TestDSARDisclosureVerbatim(t *testing.T) {
	want := []string{
		"Control Plane export: profile + org memberships + API key metadata.",
		"Audit events for this subject are retained by audit-core (verevon.audit.v2.control.user-core.*).",
		"Model Plane run history / conversations and Data Plane documents are purged/exported via the verevon.gdpr.erasure.requested fan-out (follow-up subscribers).",
	}
	if len(DSARControlPlaneDisclosure) != len(want) {
		t.Fatalf("disclosure has %d notes, want %d", len(DSARControlPlaneDisclosure), len(want))
	}
	for i, w := range want {
		if DSARControlPlaneDisclosure[i] != w {
			t.Errorf("disclosure[%d] = %q, want %q", i, DSARControlPlaneDisclosure[i], w)
		}
	}
}

// TestHardEraseRequiresAuthPool proves the auth-DB dependency is explicit: with
// no AUTH_DATABASE_URL pool wired, hard erase/anonymize fail loudly rather than
// silently skipping auth-side data.
func TestHardEraseRequiresAuthPool(t *testing.T) {
	svc := &Service{} // authPool nil
	if _, err := svc.HardEraseUser(t.Context(), "u_1", "u_1", "self", "org-1"); err == nil {
		t.Error("HardEraseUser must error when authPool is nil")
	} else if !strings.Contains(err.Error(), "AUTH_DATABASE_URL") {
		t.Errorf("error should mention AUTH_DATABASE_URL, got %v", err)
	}
	if _, err := svc.AnonymizeUser(t.Context(), "u_1", "u_1", "self", "org-1"); err == nil {
		t.Error("AnonymizeUser must error when authPool is nil")
	}
}

// TestEraseRejectsEmptyID covers the input-validation guard.
func TestEraseRejectsEmptyID(t *testing.T) {
	svc := &Service{}
	if _, err := svc.HardEraseUser(t.Context(), "", "u_1", "self", "org-1"); err == nil {
		t.Error("HardEraseUser must reject empty id")
	}
	if _, err := svc.AnonymizeUser(t.Context(), "  ", "u_1", "self", "org-1"); err == nil {
		t.Error("AnonymizeUser must reject blank id")
	}
	if _, err := svc.BuildDSARExport(t.Context(), ""); err == nil {
		t.Error("BuildDSARExport must reject empty id")
	}
}

// TestDSARExportOmitsSecrets statically asserts the DSAR export never surfaces
// the password hash or raw API key material — only metadata.
func TestDSARExportOmitsSecrets(t *testing.T) {
	b, err := os.ReadFile("gdpr.go")
	if err != nil {
		t.Fatalf("read gdpr.go: %v", err)
	}
	src := string(b)
	for _, secret := range []string{"PasswordHash", "password_hash", "KeyHash", "key_hash"} {
		if strings.Contains(src, secret) {
			t.Errorf("DSAR export source references secret field %q; must export metadata only", secret)
		}
	}
}
