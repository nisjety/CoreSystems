package main

import "testing"

func TestEnvOr(t *testing.T) {
	t.Setenv("QUARRY_TEST_ENVOR", "x")
	if got := envOr("QUARRY_TEST_ENVOR", "d"); got != "x" {
		t.Fatalf("envOr returned %q want %q", got, "x")
	}

	t.Setenv("QUARRY_TEST_ENVOR", "")
	if got := envOr("QUARRY_TEST_ENVOR", "d"); got != "d" {
		t.Fatalf("envOr fallback %q want %q", got, "d")
	}
}
