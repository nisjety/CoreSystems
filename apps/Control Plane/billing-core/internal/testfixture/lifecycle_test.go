package testfixture

import "testing"

func TestValidateLifecycleTargetFailsClosed(t *testing.T) {
	validID := "0123456789abcdef0123456789abcdef0123456789abcdef"
	if err := ValidateLifecycleTarget(
		"postgres://fixture:secret@localhost:5432/billing_lifecycle?sslmode=disable",
		"billing_lifecycle",
		validID,
	); err != nil {
		t.Fatalf("valid disposable target rejected: %v", err)
	}

	unsafe := []struct {
		dsn       string
		database  string
		fixtureID string
	}{
		{"postgres://fixture:secret@billing-db:5432/billing_lifecycle", "billing_lifecycle", validID},
		{"postgres://fixture:secret@localhost:5432/controlplane", "billing_lifecycle", validID},
		{"postgres://fixture:secret@localhost:5432/billing_lifecycle", "billing_lifecycle", "placeholder"},
	}
	for _, candidate := range unsafe {
		if err := ValidateLifecycleTarget(candidate.dsn, candidate.database, candidate.fixtureID); err == nil {
			t.Fatalf("unsafe lifecycle target accepted: %+v", candidate)
		}
	}
}
