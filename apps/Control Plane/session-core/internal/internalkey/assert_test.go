package internalkey

import (
	"testing"
)

func TestValidate(t *testing.T) {
	tests := []struct {
		name     string
		value    string
		wantKind ProblemKind
		wantOK   bool
	}{
		{name: "empty", value: "", wantKind: ProblemMissing},
		{name: "placeholder_test", value: "test", wantKind: ProblemPlaceholder},
		{name: "placeholder_test_key", value: "test-key-abc", wantKind: ProblemPlaceholder},
		{name: "placeholder_change_me_underscore", value: "change-me_secret_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", wantKind: ProblemPlaceholder},
		{name: "placeholder_your_prefix", value: "your-internal-secret-here-replace-me-with-real-key", wantKind: ProblemPlaceholder},
		{name: "too_short", value: "short-but-not-a-known-place", wantKind: ProblemTooShort},
		{name: "ok_64_hex", value: "11604143a90303a16869372de84b493a8742d45c51e4142554640f3d0266965f", wantOK: true},
		{name: "ok_exactly_32", value: "01234567890123456789012345678901", wantOK: true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			p := Validate("ENV", tc.value)
			if tc.wantOK {
				if p != nil {
					t.Fatalf("want OK, got problem %+v", p)
				}
				return
			}
			if p == nil {
				t.Fatalf("want kind=%s, got nil", tc.wantKind)
			}
			if p.Kind != tc.wantKind {
				t.Fatalf("want kind=%s, got %s", tc.wantKind, p.Kind)
			}
		})
	}
}

func TestResolve(t *testing.T) {
	t.Setenv("FIRST", "")
	t.Setenv("SECOND", " value-from-second ")
	t.Setenv("THIRD", "value-from-third")

	name, value := Resolve("FIRST", "SECOND", "THIRD")
	if name != "SECOND" {
		t.Fatalf("want SECOND, got %s", name)
	}
	if value != "value-from-second" {
		t.Fatalf("want trimmed value, got %q", value)
	}
}

func TestResolveAllEmpty(t *testing.T) {
	t.Setenv("A", "")
	t.Setenv("B", "")
	name, value := Resolve("A", "B")
	if name != "" || value != "" {
		t.Fatalf("want empty, got name=%q value=%q", name, value)
	}
}

func TestAssertFromEnv_OK(t *testing.T) {
	t.Setenv("PRIMARY", "11604143a90303a16869372de84b493a8742d45c51e4142554640f3d0266965f")
	r := AssertFromEnv("PRIMARY", "FALLBACK")
	if !r.OK {
		t.Fatalf("want OK, got %+v", r.Problem)
	}
	if r.Resolved != "PRIMARY" {
		t.Fatalf("want resolved=PRIMARY, got %s", r.Resolved)
	}
}

func TestAssertFromEnv_FallbackToSecondary(t *testing.T) {
	t.Setenv("PRIMARY", "")
	t.Setenv("FALLBACK", "11604143a90303a16869372de84b493a8742d45c51e4142554640f3d0266965f")
	r := AssertFromEnv("PRIMARY", "FALLBACK")
	if !r.OK {
		t.Fatalf("want OK, got %+v", r.Problem)
	}
	if r.Resolved != "FALLBACK" {
		t.Fatalf("want resolved=FALLBACK, got %s", r.Resolved)
	}
}

func TestAssertFromEnv_Placeholder(t *testing.T) {
	t.Setenv("PRIMARY", "test")
	r := AssertFromEnv("PRIMARY", "FALLBACK")
	if r.OK {
		t.Fatalf("want failure on placeholder, got OK")
	}
	if r.Problem.Kind != ProblemPlaceholder {
		t.Fatalf("want kind=placeholder, got %s", r.Problem.Kind)
	}
}

func TestAssertFromEnv_Missing(t *testing.T) {
	t.Setenv("PRIMARY", "")
	t.Setenv("FALLBACK", "")
	r := AssertFromEnv("PRIMARY", "FALLBACK")
	if r.OK {
		t.Fatalf("want failure when both missing")
	}
	if r.Problem.Kind != ProblemMissing {
		t.Fatalf("want kind=missing, got %s", r.Problem.Kind)
	}
	if r.Problem.EnvVar != "PRIMARY" {
		t.Fatalf("want EnvVar=PRIMARY (first listed), got %s", r.Problem.EnvVar)
	}
}

func TestIsProduction(t *testing.T) {
	t.Setenv("GIN_MODE", "release")
	t.Setenv("ENV", "")
	if !IsProduction() {
		t.Fatalf("want production for GIN_MODE=release")
	}
	t.Setenv("GIN_MODE", "")
	t.Setenv("ENV", "production")
	if !IsProduction() {
		t.Fatalf("want production for ENV=production")
	}
	t.Setenv("GIN_MODE", "")
	t.Setenv("ENV", "development")
	if IsProduction() {
		t.Fatalf("want NOT production for ENV=development")
	}
}
