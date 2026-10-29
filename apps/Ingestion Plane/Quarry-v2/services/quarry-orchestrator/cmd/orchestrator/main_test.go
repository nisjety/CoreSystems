package main

import "testing"

func TestEnvOr(t *testing.T) {
	t.Setenv("QUARRY_ORCH_TEST_ENVOR", "value")
	if got := envOr("QUARRY_ORCH_TEST_ENVOR", "fallback"); got != "value" {
		t.Fatalf("envOr returned %q want value", got)
	}

	t.Setenv("QUARRY_ORCH_TEST_ENVOR", "")
	if got := envOr("QUARRY_ORCH_TEST_ENVOR", "fallback"); got != "fallback" {
		t.Fatalf("envOr returned %q want fallback", got)
	}
}
