package main

import "testing"

// TestUnsignedDecisionsAllowedIsTheExactLiteralTrue pins the switch that lets
// capability-core start without a decision evidence signer. It relaxes security,
// so only the exact literal "true" enables it: no trimming and no case folding.
func TestUnsignedDecisionsAllowedIsTheExactLiteralTrue(t *testing.T) {
	cases := map[string]bool{
		"true":   true,
		"":       false,
		"TRUE":   false,
		" true ": false,
		"1":      false,
		"yes":    false,
		"false":  false,
	}
	for value, want := range cases {
		t.Setenv("CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS", value)
		if got := unsignedDecisionsAllowed(); got != want {
			t.Errorf("CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS=%q: unsignedDecisionsAllowed() = %v, want %v", value, got, want)
		}
	}
}
