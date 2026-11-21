package main

import "testing"

// O-18: COST_CORE_ACCEPT_API_KEY is read as the exact literal "true".
func TestAcceptAPIKeyEnabledIsTheStrictLiteral(t *testing.T) {
	read := func(value string) bool {
		return acceptAPIKeyEnabled(func(key string) string {
			if key != "COST_CORE_ACCEPT_API_KEY" {
				t.Fatalf("unexpected variable %q", key)
			}
			return value
		})
	}
	if !read("true") {
		t.Fatal("true did not enable it")
	}
	for _, value := range []string{"", "TRUE", "True", "1", "yes", " true", "true ", "false"} {
		if read(value) {
			t.Fatalf("%q enabled it", value)
		}
	}
}
