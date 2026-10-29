package main

import "testing"

func TestModelActionViewVerifierFromEnvFailsClosedWhenControlPublicKeyIsMissing(t *testing.T) {
	if _, err := modelActionViewVerifierFromEnv(func(string) string { return "" }); err == nil {
		t.Fatal("model action view verifier accepted empty Control deployment configuration")
	}
}
