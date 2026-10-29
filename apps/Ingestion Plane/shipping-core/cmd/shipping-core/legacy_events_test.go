package main

import "testing"

func TestUnsignedShippingEventsRequireThreeIsolatedDevGates(t *testing.T) {
	for _, tc := range []struct {
		legacy, insecure, isolated string
		want                       bool
	}{
		{"", "", "", false},
		{"1", "1", "", false},
		{"1", "", "1", false},
		{"", "1", "1", false},
		{"1", "1", "1", true},
	} {
		if got := unverifiedLegacyEventsEnabled(tc.legacy, tc.insecure, tc.isolated); got != tc.want {
			t.Fatalf("gates (%q,%q,%q) = %v, want %v", tc.legacy, tc.insecure, tc.isolated, got, tc.want)
		}
	}
}
