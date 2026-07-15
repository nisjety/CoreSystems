package testfixture

import "testing"

func TestValidateLifecycleTargetFailsClosed(t *testing.T) {
	validID := "0123456789abcdef0123456789abcdef0123456789abcdef"
	if err := ValidateLifecycleTarget(
		"postgres://fixture:secret@127.0.0.1:5432/org_lifecycle?sslmode=disable",
		"org_lifecycle",
		validID,
	); err != nil {
		t.Fatalf("valid disposable target rejected: %v", err)
	}

	unsafe := []struct {
		dsn       string
		database  string
		fixtureID string
	}{
		{"postgres://fixture:secret@db.internal:5432/org_lifecycle", "org_lifecycle", validID},
		{"postgres://fixture:secret@127.0.0.1:5432/controlplane", "org_lifecycle", validID},
		{"postgres://fixture:secret@127.0.0.1:5432/org_lifecycle", "org_lifecycle", "missing"},
	}
	for _, candidate := range unsafe {
		if err := ValidateLifecycleTarget(candidate.dsn, candidate.database, candidate.fixtureID); err == nil {
			t.Fatalf("unsafe lifecycle target accepted: %+v", candidate)
		}
	}
}
