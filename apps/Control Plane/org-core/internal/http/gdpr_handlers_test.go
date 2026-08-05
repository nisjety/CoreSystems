package http

import (
	"os"
	"strings"
	"testing"
)

// readRepoSource reads a Go source file relative to this test package. Used to
// statically assert the GDPR proc calls are parameterized.
func readRepoSource(t *testing.T, path string) string {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return string(b)
}

// TestErasureFanoutSubjects pins the cross-plane erasure contract. Subscribers
// (Model Plane run-history/conversations, Data Plane documents) depend on these
// exact subject names, so a rename here is a breaking change and must be
// deliberate.
func TestErasureFanoutSubjects(t *testing.T) {
	audit, fanout := erasureFanoutSubjects()
	if audit != "verevon.audit.v2.control.org-core.erasure" {
		t.Errorf("audit subject = %q, want verevon.audit.v2.control.org-core.erasure", audit)
	}
	if fanout != "verevon.gdpr.erasure.requested" {
		t.Errorf("fan-out subject = %q, want verevon.gdpr.erasure.requested", fanout)
	}
}

// TestConfirmedErasure pins the irreversible-erasure confirm gate: hard delete
// must proceed ONLY when confirm:true is supplied.
func TestConfirmedErasure(t *testing.T) {
	if confirmedErasure(true) != true {
		t.Error("confirmedErasure(true) must allow erasure")
	}
	if confirmedErasure(false) != false {
		t.Error("confirmedErasure(false) must block erasure")
	}
}

// TestPlatformRoleIsAdmin covers the platform-admin override used to gate
// erasure for callers who are not org owners.
func TestPlatformRoleIsAdmin(t *testing.T) {
	tests := []struct {
		name string
		role string
		want bool
	}{
		{"empty", "", false},
		{"member", "member", false},
		{"owner is org-level not platform", "owner", false},
		{"admin", "admin", true},
		{"superadmin", "superadmin", true},
		{"mixed csv", "user,admin", true},
		{"whitespace + case", "  SuperAdmin ", true},
		{"viewer", "viewer", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := platformRoleIsAdmin(tt.role); got != tt.want {
				t.Errorf("platformRoleIsAdmin(%q) = %v, want %v", tt.role, got, tt.want)
			}
		})
	}
}

// TestErasureSQLIsParameterized guards against SQL injection: the GDPR proc
// calls in the repository must bind the org id as a parameter ($1), never
// interpolate it into the query string (no fmt.Sprintf / string concat of ids).
func TestErasureSQLIsParameterized(t *testing.T) {
	const repoPath = "../org/repository.go"
	data := readRepoSource(t, repoPath)

	// The three proc calls must appear with bound parameters.
	wantParameterized := []string{
		"gdpr_hard_delete_organization($1)",
		"soft_delete_organization($1)",
		"purge_old_deleted_organizations($1)",
	}
	for _, want := range wantParameterized {
		if !strings.Contains(data, want) {
			t.Errorf("repository.go missing parameterized proc call %q", want)
		}
	}

	// Defensive: no string-formatted proc invocation (e.g. building the call
	// with Sprintf and the org id) should ever appear.
	for _, bad := range []string{
		"gdpr_hard_delete_organization(' +",
		"fmt.Sprintf(\"SELECT gdpr_hard_delete_organization",
		"fmt.Sprintf(\"SELECT soft_delete_organization",
	} {
		if strings.Contains(data, bad) {
			t.Errorf("repository.go contains non-parameterized proc call %q (SQL injection risk)", bad)
		}
	}
}
